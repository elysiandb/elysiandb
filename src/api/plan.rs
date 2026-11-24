use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration as StdDuration, Instant};

use crate::datastore::{
    Bitset, Doc, Engine, INDEX_BITMAP, INDEX_FILTER, INDEX_REFS, INDEX_SORT, IndexSpec, IndexView,
    KEY_BOOL, KEY_DATE, KEY_ELEMENT, KEY_LOWER, KEY_NUMBER, KEY_PRESENT, KEY_RAW, RefPath,
    SORT_RANK_MISSING, append_escaped, append_number, append_terminated, append_time, decode_map,
    key_payload, ref_prefix,
};
use crate::docpath::{self, SortValue};
use crate::globals;
use crate::gocompat::json::{Map, Value};
use crate::gocompat::sort as gosort;
use crate::gocompat::strconv;
use crate::gocompat::strings;
use crate::gocompat::time::{Duration, Time};
use crate::metrics;
use crate::query::{FilterNode, Filters, Ops, Sort};
use crate::sync::{Condvar, Mutex};

use super::filter::{
    day, filters_match_entity, is_comparison_operator, parse_date, search_matches_entity,
    search_pattern,
};
use super::guard::{self, Guard, GuardError};
use super::include::{EntityReader, apply_includes_with, read_entity_by_id_func};
use super::query::match_filter_node;
use super::search_raw::{lowered_search_pattern, search_bytes_lowered};
use super::sort::{get_sort_nested_value, sort_step};
use super::storage::{db, is_core_entity_type};

const MAX_INDEXES_PER_ENTITY: usize = 64;

pub struct ListQuery<'a, 'g> {
    pub entity: &'a [u8],
    pub limit: i64,
    pub offset: i64,
    pub sort_field: &'a [u8],
    pub sort_asc: bool,
    pub sorts: Vec<Sort>,
    pub filters: Option<&'a Filters>,
    pub node: Option<&'a FilterNode>,
    pub search: &'a [u8],
    pub includes: &'a [u8],
    pub count_only: bool,
    pub snapshot: Option<crate::datastore::Snapshot>,
    pub guard: Option<&'g Guard>,
    pub each: Option<&'a mut dyn FnMut(Doc) -> bool>,
}

impl Default for ListQuery<'_, '_> {
    fn default() -> Self {
        ListQuery {
            entity: b"",
            limit: 0,
            offset: 0,
            sort_field: b"",
            sort_asc: false,
            sorts: Vec::new(),
            filters: None,
            node: None,
            search: b"",
            includes: b"",
            count_only: false,
            snapshot: None,
            guard: None,
            each: None,
        }
    }
}

#[derive(Default)]
pub struct ListResult<'a> {
    snap: Option<crate::datastore::Snapshot>,
    guard: Option<&'a Guard>,
    pub docs: Vec<Doc>,
    pub total: usize,
    pub err: Option<GuardError>,
}

type Keep<'k> = &'k mut dyn FnMut(&[u8]) -> bool;

pub fn snapshot(entity: &[u8]) -> Option<crate::datastore::Snapshot> {
    db().map(|e| e.indexed_snapshot(entity))
}

const DECODED_OVERHEAD: i64 = 4;

impl ListResult<'_> {
    pub fn snapshot(&self) -> Option<&crate::datastore::Snapshot> {
        self.snap.as_ref()
    }

    pub fn ids(&self) -> Vec<Vec<u8>> {
        self.docs.iter().map(|d| d.id().into_owned()).collect()
    }

    pub fn decode(&self) -> Vec<Map> {
        let mut out = Vec::with_capacity(self.docs.len());
        for (i, d) in self.docs.iter().enumerate() {
            let Ok(raw) = d.bytes() else {
                continue;
            };

            if guard::charge(self.guard, raw.len() as i64 * DECODED_OVERHEAD).is_err() {
                return Vec::new();
            }

            if i % GUARD_INTERVAL == GUARD_INTERVAL - 1 && guard::check(self.guard).is_err() {
                return Vec::new();
            }

            if let Ok(Some(doc)) = decode_map(&raw) {
                out.push(doc);
            }
        }

        out
    }

    pub(crate) fn reader(&self) -> EntityReader<'static> {
        include_reader()
    }
}

pub(crate) fn include_reader() -> EntityReader<'static> {
    if let Some(f) = read_entity_by_id_func() {
        return Box::new(move |entity, id| f(entity, id));
    }

    new_doc_reader()
}

fn doc_key(entity: &[u8], id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(entity.len() + 1 + id.len());
    key.extend_from_slice(entity);
    key.push(0);
    key.extend_from_slice(id);
    key
}

fn new_doc_reader() -> EntityReader<'static> {
    let mut snaps: HashMap<Vec<u8>, crate::datastore::Snapshot> = HashMap::new();
    let mut cache: HashMap<Vec<u8>, Option<Arc<[u8]>>> = HashMap::new();

    Box::new(move |entity, id| {
        let key = doc_key(entity, id);
        if let Some(b) = cache.get(&key) {
            return b.as_deref().and_then(decode_map_or_nil);
        }

        let e = db()?;
        let snap = snaps
            .entry(entity.to_vec())
            .or_insert_with(|| e.snapshot(entity));

        let Some(doc) = snap.get(id) else {
            cache.insert(key, None);
            return None;
        };

        let Ok(b) = doc.bytes() else {
            return None;
        };

        cache.insert(key, Some(b.clone()));

        decode_map_or_nil(&b)
    })
}

#[derive(Clone, Default)]
pub(crate) struct Candidates {
    pub(crate) all: bool,
    pub(crate) set: Bitset,
    pub(crate) exact: Option<Bitset>,
}

fn universe() -> Candidates {
    Candidates {
        all: true,
        set: Bitset::default(),
        exact: None,
    }
}

fn restricted(set: Bitset) -> Candidates {
    Candidates {
        all: false,
        set,
        exact: None,
    }
}

fn and_exact(a: &Option<Bitset>, b: &Option<Bitset>) -> Option<Bitset> {
    let (Some(ea), Some(eb)) = (a, b) else {
        return None;
    };

    let mut out = ea.clone();
    out.and(eb);
    Some(out)
}

fn intersect(a: Candidates, b: Candidates) -> Candidates {
    let exact = and_exact(&a.exact, &b.exact);
    if a.all && b.all {
        return Candidates {
            all: true,
            set: Bitset::default(),
            exact,
        };
    }

    if a.all {
        return Candidates {
            all: false,
            set: b.set,
            exact,
        };
    }

    if b.all {
        return Candidates {
            all: false,
            set: a.set,
            exact,
        };
    }

    let mut set = a.set;
    set.and(&b.set);

    Candidates {
        all: false,
        set,
        exact,
    }
}

fn or_exact(a: Option<Bitset>, b: Option<Bitset>) -> Option<Bitset> {
    match (a, b) {
        (None, None) => None,
        (None, Some(b)) => Some(b),
        (Some(a), None) => Some(a),
        (Some(mut a), Some(b)) => {
            a.or(&b);
            Some(a)
        }
    }
}

fn union(a: Candidates, b: Candidates) -> Candidates {
    let exact = or_exact(a.exact, b.exact);
    if a.all || b.all {
        return Candidates {
            all: true,
            set: Bitset::default(),
            exact,
        };
    }

    let mut set = a.set;
    set.or(&b.set);

    Candidates {
        all: false,
        set,
        exact,
    }
}

pub(crate) struct Planner {
    pub(crate) e: Arc<Engine>,
    pub(crate) snap: crate::datastore::Snapshot,
    pub(crate) refs: Vec<RefPath>,
    pub(crate) inc: bool,
    pub(crate) depth: usize,
}

fn with_suffix(path: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(path.len() + suffix.len());
    out.extend_from_slice(path);
    out.extend_from_slice(suffix);
    out
}

fn under(path: &[u8], base: &[u8]) -> bool {
    path.len() > base.len() && path.starts_with(base) && path[base.len()] == b'.'
}

impl Planner {
    pub(crate) fn new(e: Arc<Engine>, snap: crate::datastore::Snapshot, inc: bool) -> Planner {
        let refs = snap.refs().to_vec();
        Planner {
            e,
            snap,
            refs,
            inc,
            depth: 0,
        }
    }

    pub(crate) fn affected_by_includes(&self, path: &[u8]) -> bool {
        if !self.inc {
            return false;
        }

        self.crosses_reference(path)
    }

    fn crosses_reference(&self, path: &[u8]) -> bool {
        for r in &self.refs {
            if path == with_suffix(&r.path, b".id") || path == with_suffix(&r.path, b".@entity") {
                continue;
            }

            if path == r.path.as_slice() || under(path, &r.path) {
                return true;
            }
        }

        false
    }

    pub(crate) fn index(&mut self, spec: &IndexSpec) -> Option<IndexView> {
        if let Some(view) = self.snap.index(spec) {
            return Some(view);
        }

        let registered = self.snap.has_index(spec);
        let names = self.e.index_names(self.snap.name()).map_or(0, |n| n.len());
        if ((!registered && names < MAX_INDEXES_PER_ENTITY) || self.snap.index_dormant(spec))
            && !self.snap.is_empty()
        {
            let name = self.snap.name().to_vec();
            if self.e.ensure_index(&name, spec) {
                self.snap = self.e.indexed_snapshot(&name);
                return self.snap.index(spec);
            }
        }

        None
    }

    fn filter_index(&mut self, field: &[u8]) -> Option<IndexView> {
        if let Some(view) = self.snap.index(&IndexSpec::new(INDEX_BITMAP, field)) {
            return Some(view);
        }

        self.index(&IndexSpec::new(INDEX_FILTER, field))
    }

    pub(crate) fn leaf(&mut self, field: &[u8], ops: &Ops) -> Candidates {
        if ops.is_empty() {
            return universe();
        }

        if self.affected_by_includes(field) {
            return self.through_references(field, ops);
        }

        let Some(view) = self.filter_index(field) else {
            return universe();
        };

        let mut result: Option<Candidates> = None;
        for (op, cmp) in ops {
            let c = op_candidates(&view, op, cmp, ops);
            result = Some(match result {
                None => c,
                Some(r) => intersect(r, c),
            });
        }
        let mut result = result.unwrap_or_else(universe);

        if ops.len() > 1 {
            result.exact = None;
        }

        if let Some(refined) = super::evaluate::evaluate_leaf(&view, ops, &result) {
            return refined;
        }

        result
    }

    fn through_references(&mut self, field: &[u8], ops: &Ops) -> Candidates {
        if self.depth >= MAX_REFERENCE_DEPTH {
            return universe();
        }

        let mut path: &[u8] = b"";
        for r in &self.refs {
            if under(field, &r.path) && (path.is_empty() || r.path.len() < path.len()) {
                path = &r.path;
            }
        }
        let path = path.to_vec();

        let rest: &[u8] = if under(field, &path) {
            &field[path.len() + 1..]
        } else {
            field
        };
        if path.is_empty() || rest.is_empty() || rest == b"id" || rest == b"@entity" {
            return universe();
        }
        let rest = rest.to_vec();

        let Some(refs_view) = self.index(&IndexSpec::new(INDEX_REFS, b"".as_slice())) else {
            return universe();
        };

        let mut result = Bitset::default();
        match self.filter_index(field) {
            Some(direct) => {
                let key = [KEY_PRESENT];
                collect(&direct, &key, &after_term(&key), &mut result, None);
            }
            None => return universe(),
        }

        let refs = self.refs.clone();
        for r in &refs {
            if r.path != path || is_core_entity_type(&r.target) {
                continue;
            }

            let target_snap = self.e.indexed_snapshot(&r.target);
            let mut target = Planner {
                e: self.e.clone(),
                refs: target_snap.refs().to_vec(),
                snap: target_snap,
                inc: true,
                depth: self.depth + 1,
            };
            let matched = target.leaf(&rest, ops);
            if matched.all {
                return universe();
            }

            let prefix = ref_prefix(&path, &r.target);
            matched.set.for_each(|seq| {
                let Some(d) = target.snap.by_seq(seq) else {
                    return true;
                };

                let mut key = prefix.clone();
                append_terminated(&mut key, &d.id());
                refs_view.scan_prefix(&key, |_, source| {
                    result.set(source);
                    true
                });

                true
            });
        }

        restricted(result)
    }

    fn estimate(&mut self, field: &[u8], ops: &Ops) -> i64 {
        if ops.len() != 1 || self.affected_by_includes(field) {
            return i64::MAX;
        }

        let Some(cmp) = ops.get(b"eq".as_slice()) else {
            return i64::MAX;
        };
        if has_wildcard(cmp) {
            return i64::MAX;
        }

        let Some(view) = self.filter_index(field) else {
            return i64::MAX;
        };

        let mut n: i64 = 0;
        let mut count = |term: &[u8]| {
            n += view.rank(&after_term(term)) - view.rank(term);
        };

        count(&exact_of(KEY_LOWER, &strings::to_lower(cmp)));
        count(&exact_of(KEY_ELEMENT, first_part(cmp)));
        if let Ok(f) = strconv::parse_float(cmp, 64) {
            count(&number_key(f));
        }
        count(&[KEY_BOOL, bool_byte(cmp)]);
        if parse_date(cmp).is_some() {
            return i64::MAX;
        }

        n
    }

    pub(crate) fn rest_filters(&mut self, filters: &Filters) -> Candidates {
        if filters.is_empty() {
            return universe();
        }

        let mut order: Vec<(&[u8], &Ops, i64)> = Vec::with_capacity(filters.len());
        for (field, ops) in filters {
            let mut size = i64::MAX;
            if filters.len() > 1 {
                size = self.estimate(field, ops);
            }
            order.push((field, ops, size));
        }
        gosort::slice_stable(&mut order, |a, b| {
            if a.2 != b.2 {
                return a.2 < b.2;
            }
            a.0 < b.0
        });

        let mut result = self.leaf(order[0].0, order[0].1);
        for &(field, ops, size) in &order[1..] {
            if !result.all {
                let pending = result.set.count() as i64;
                if pending <= VERIFY_DIRECTLY
                    || (size != i64::MAX
                        && size > SELECTIVE_MINIMUM
                        && size / SELECTIVE_RATIO > pending)
                {
                    result.exact = None;
                    continue;
                }
            }
            let next = self.leaf(field, ops);
            result = intersect(result, next);
        }

        result
    }

    pub(crate) fn node(&mut self, n: &FilterNode) -> Candidates {
        struct Frame<'a> {
            children: &'a [FilterNode],
            is_and: bool,
            next: usize,
            acc: Option<Candidates>,
        }

        let mut stack: Vec<Frame<'_>> = Vec::new();
        let mut cur = n;
        loop {
            let mut result = if let Some(leaf) = &cur.leaf {
                if leaf.is_empty() {
                    universe()
                } else {
                    self.rest_filters(leaf)
                }
            } else if !cur.and.is_empty() {
                stack.push(Frame {
                    children: &cur.and,
                    is_and: true,
                    next: 0,
                    acc: None,
                });
                cur = &cur.and[0];
                continue;
            } else if !cur.or.is_empty() {
                stack.push(Frame {
                    children: &cur.or,
                    is_and: false,
                    next: 0,
                    acc: None,
                });
                cur = &cur.or[0];
                continue;
            } else {
                Candidates {
                    all: false,
                    set: Bitset::default(),
                    exact: Some(Bitset::default()),
                }
            };

            loop {
                let Some(top) = stack.last_mut() else {
                    return result;
                };

                top.acc = Some(match top.acc.take() {
                    None => result,
                    Some(acc) if top.is_and => intersect(acc, result),
                    Some(acc) => union(acc, result),
                });
                top.next += 1;
                if top.next < top.children.len() {
                    cur = &top.children[top.next];
                    break;
                }

                let Some(done) = stack.pop() else {
                    return universe();
                };
                result = done.acc.unwrap_or_else(universe);
            }
        }
    }

    pub(crate) fn sort_path_crosses_reference(&self, field: &[u8]) -> bool {
        self.crosses_reference(field)
    }
}

const MAX_REFERENCE_DEPTH: usize = 4;

pub(crate) fn unescape_payload(key: &[u8]) -> Cow<'_, [u8]> {
    let body = &key[1..key.len() - 6];
    if !body.contains(&0) {
        return Cow::Borrowed(body);
    }

    let mut out = Vec::with_capacity(body.len());
    let mut i = 0;
    while i < body.len() {
        if body[i] == 0 && i + 1 < body.len() && body[i + 1] == 0xff {
            out.push(0);
            i += 2;
            continue;
        }
        out.push(body[i]);
        i += 1;
    }

    Cow::Owned(out)
}

fn prefix_of(kind: u8, s: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    append_escaped(&mut out, s);
    out
}

fn exact_of(kind: u8, s: &[u8]) -> Vec<u8> {
    let mut out = vec![kind];
    append_terminated(&mut out, s);
    out
}

fn number_key(f: f64) -> Vec<u8> {
    let mut out = vec![KEY_NUMBER];
    append_number(&mut out, f);
    out
}

fn date_key(t: &Time) -> Vec<u8> {
    let mut out = vec![KEY_DATE];
    append_time(&mut out, t);
    out
}

pub(crate) fn collect(
    view: &IndexView,
    lo: &[u8],
    hi: &[u8],
    set: &mut Bitset,
    keep: Option<Keep<'_>>,
) {
    view.collect(Some(lo), Some(hi), set, keep);
}

fn after_term(term: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(term.len() + 5);
    out.extend_from_slice(term);
    out.extend_from_slice(&[0xff; 5]);
    out
}

pub(crate) fn kind_bounds(kind: u8) -> (Vec<u8>, Vec<u8>) {
    (vec![kind], vec![kind + 1])
}

fn all_comparison_dates(ops: &Ops) -> bool {
    ops.iter()
        .all(|(op, cmp)| is_comparison_operator(op) && parse_date(cmp).is_some())
}

fn day_start(t: &Time) -> Time {
    t.truncate(day())
}

fn has_wildcard(s: &[u8]) -> bool {
    s.iter().any(|&c| c == b'*' || c == b'\\')
}

fn first_part(s: &[u8]) -> &[u8] {
    s.split(|&c| c == b',').next().unwrap_or_default()
}

fn bool_byte(cmp: &[u8]) -> u8 {
    u8::from(cmp == b"true" || cmp == b"1")
}

fn bytes_contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.is_empty() || haystack.windows(needle.len()).any(|w| w == needle)
}

fn op_candidates(view: &IndexView, op: &[u8], cmp: &[u8], ops: &Ops) -> Candidates {
    let mut exact = Bitset::default();
    let mut maybe = Bitset::default();

    let add_date_range = |maybe: &mut Bitset, lo: Option<&Time>, hi: Option<&Time>| {
        let (mut l, mut h) = kind_bounds(KEY_DATE);
        if let Some(lo) = lo {
            l = date_key(lo);
        }
        if let Some(hi) = hi {
            h = date_key(hi);
        }
        collect(view, &l, &h, maybe, None);
    };

    match op {
        b"eq" => {
            let lower = strings::to_lower(cmp);
            let literal = match lower.iter().position(|&c| c == b'*' || c == b'\\') {
                Some(i) => &lower[..i],
                None => &lower[..],
            };

            if literal.len() == lower.len() {
                let term = exact_of(KEY_LOWER, &lower);
                collect(view, &term, &after_term(&term), &mut exact, None);
            } else {
                let prefix = prefix_of(KEY_LOWER, literal);
                let (_, hi) = kind_bounds(KEY_LOWER);
                view.scan_range(Some(&prefix), Some(&hi), |key, seq| {
                    if !key.starts_with(&prefix) {
                        return false;
                    }
                    if docpath::match_glob(cmp, &unescape_payload(key)) {
                        exact.set(seq);
                    }
                    true
                });
            }

            if let Ok(f) = strconv::parse_float(cmp, 64) {
                let key = number_key(f);
                collect(view, &key, &after_term(&key), &mut exact, None);
            }

            let bool_key = [KEY_BOOL, bool_byte(cmp)];
            collect(view, &bool_key, &after_term(&bool_key), &mut exact, None);

            if let Some((t, _)) = parse_date(cmp)
                && all_comparison_dates(ops)
            {
                let lo = day_start(&t);
                let hi = lo.add(day());
                add_date_range(&mut maybe, Some(&lo), Some(&hi));
            }

            let term = exact_of(KEY_ELEMENT, first_part(cmp));
            collect(view, &term, &after_term(&term), &mut maybe, None);
        }
        b"lt" | b"lte" | b"gt" | b"gte" => {
            if let Ok(f) = strconv::parse_float(cmp, 64) {
                let key = number_key(f);
                let (mut lo, mut hi) = kind_bounds(KEY_NUMBER);
                match op {
                    b"lt" => hi = key,
                    b"lte" => hi = after_term(&key),
                    b"gt" => lo = after_term(&key),
                    _ => lo = key,
                }
                collect(view, &lo, &hi, &mut exact, None);
            }

            let term = exact_of(KEY_RAW, cmp);
            let (mut lo, mut hi) = kind_bounds(KEY_RAW);
            match op {
                b"lt" => hi = term,
                b"lte" => hi = after_term(&term),
                b"gt" => lo = after_term(&term),
                _ => lo = term,
            }
            collect(view, &lo, &hi, &mut maybe, None);

            if let Some((t, _)) = parse_date(cmp)
                && all_comparison_dates(ops)
            {
                let d = day_start(&t);
                if op == b"gt" || op == b"gte" {
                    add_date_range(&mut maybe, Some(&d), None);
                } else {
                    let next = d.add(day());
                    add_date_range(&mut maybe, None, Some(&next));
                }
            }
        }
        b"contains" => {
            let needle = strings::to_lower(cmp);
            let (lo, hi) = kind_bounds(KEY_LOWER);
            let mut keep = |key: &[u8]| bytes_contains(&unescape_payload(key), &needle);
            collect(view, &lo, &hi, &mut exact, Some(&mut keep));

            let term = exact_of(KEY_ELEMENT, cmp);
            collect(view, &term, &after_term(&term), &mut exact, None);
        }
        b"any" => {
            for v in cmp.split(|&c| c == b',') {
                let term = exact_of(KEY_ELEMENT, v);
                collect(view, &term, &after_term(&term), &mut exact, None);
            }
        }
        b"all" => {
            let term = exact_of(KEY_ELEMENT, first_part(cmp));
            collect(view, &term, &after_term(&term), &mut maybe, None);
        }
        _ => return universe(),
    }

    let mut set = exact.clone();
    set.or(&maybe);

    Candidates {
        all: false,
        set,
        exact: Some(exact),
    }
}

pub fn eq_key(filters: &Filters) -> Option<(Vec<u8>, Vec<Vec<u8>>)> {
    let mut best: Option<&[u8]> = None;
    for (field, ops) in filters {
        let Some(cmp) = ops.get(b"eq".as_slice()) else {
            continue;
        };
        if ops.len() != 1 || field.contains(&b'.') || has_wildcard(cmp) {
            continue;
        }

        if parse_date(cmp).is_some() {
            continue;
        }

        if best.is_none_or(|b| field.as_slice() < b) {
            best = Some(field);
        }
    }

    let best = best?;
    let cmp = &filters[best][b"eq".as_slice()];
    let mut terms = vec![
        exact_of(KEY_LOWER, &strings::to_lower(cmp)),
        exact_of(KEY_ELEMENT, first_part(cmp)),
    ];

    if let Ok(f) = strconv::parse_float(cmp, 64) {
        terms.push(number_key(f));
    }

    terms.push(vec![KEY_BOOL, bool_byte(cmp)]);

    Some((best.to_vec(), terms))
}

const SELECTIVE_RATIO: i64 = 16;
const SELECTIVE_MINIMUM: i64 = 4096;
const VERIFY_DIRECTLY: i64 = 256;

pub(crate) fn node_is_everything(n: &FilterNode) -> bool {
    n.leaf.as_ref().is_some_and(|l| l.is_empty())
}

fn effective_sorts(q: &ListQuery<'_, '_>) -> Vec<Sort> {
    if q.sorts.is_empty() && !q.sort_field.is_empty() {
        return vec![Sort::new(q.sort_field, q.sort_asc)];
    }

    q.sorts.clone()
}

struct Verifier<'q, 'g> {
    needs_match: bool,
    cand: &'q Candidates,
    filters: Option<&'q Filters>,
    node: Option<&'q FilterNode>,
    search: &'q [u8],
    guard: Option<&'g Guard>,
    heavy: bool,
    search_on_bytes: bool,
    only_search: bool,
    lowered_search: Vec<u8>,
    eval_includes: &'q [u8],
    reader: EntityReader<'static>,
    verified: usize,
    expired: bool,
    started: Instant,
    release: Option<LaneSlot>,
}

impl Verifier<'_, '_> {
    fn match_fields(&self, doc: &Map) -> bool {
        if let Some(node) = self.node
            && !match_filter_node(node, doc)
        {
            return false;
        }

        self.filters
            .is_none_or(|f| f.is_empty() || filters_match_entity(doc, f))
    }

    fn matches(&self, doc: &Map) -> bool {
        self.match_fields(doc)
            && (self.search.is_empty() || search_matches_entity(doc, self.search))
    }

    fn verify(&mut self, d: &Doc) -> bool {
        if !self.needs_match || self.expired {
            return !self.expired;
        }

        let seq = d.seq();
        if !self.cand.all && !self.cand.set.has(seq) {
            return false;
        }

        if self.cand.exact.as_ref().is_some_and(|e| e.has(seq)) {
            return true;
        }

        if self.verified == 0 {
            self.started = Instant::now();
        }

        self.verified += 1;
        if self.verified.is_multiple_of(GUARD_INTERVAL) {
            if self.heavy && self.release.is_none() && self.started.elapsed() >= HEAVY_LANE_AFTER {
                match acquire_heavy_lane(self.guard) {
                    Err(_) => {
                        self.expired = true;
                        return false;
                    }
                    Ok(r) => self.release = Some(r),
                }
            }

            if guard::check(self.guard).is_err() {
                self.expired = true;
                return false;
            }
        }

        let mut fields_only = false;
        if self.search_on_bytes
            && let Ok(raw) = d.bytes()
        {
            let (found, ok) = search_bytes_lowered(&raw, &self.lowered_search);
            if ok {
                if !found || self.only_search {
                    return found;
                }
                fields_only = true;
            }
        }

        let Ok(doc) = d.decode() else {
            return false;
        };

        let mut docs = [doc];
        if !self.eval_includes.is_empty() {
            apply_includes_with(&mut docs, self.eval_includes, &mut self.reader);
        }

        if fields_only {
            return self.match_fields(&docs[0]);
        }

        self.matches(&docs[0])
    }
}

pub fn plan<'g>(mut q: ListQuery<'_, 'g>) -> ListResult<'g> {
    let Some(e) = db() else {
        return ListResult::default();
    };

    let snap = q
        .snapshot
        .take()
        .unwrap_or_else(|| e.indexed_snapshot(q.entity));

    let mut p = Planner::new(e.clone(), snap, !q.includes.is_empty());
    let mut each = q.each.take();
    if each.is_none()
        && let Some((docs, total)) = p.composite_page(&q)
    {
        return ListResult {
            snap: Some(p.snap.clone()),
            guard: q.guard,
            docs,
            total,
            err: None,
        };
    }

    let has_filter = q.filters.is_some_and(|f| !f.is_empty());
    let has_node = q.node.is_some_and(|n| !node_is_everything(n));
    let needs_match = has_filter || has_node || !q.search.is_empty();

    let mut cand = universe();
    let everything_exact = !needs_match;
    if has_node {
        if let Some(node) = q.node {
            cand = p.node(node);
        }
    } else if has_filter && let Some(filters) = q.filters {
        cand = p.rest_filters(filters);
    }

    if !q.search.is_empty() {
        let search_affected = p.inc && !p.refs.is_empty();
        if !search_affected
            && let Some(set) = e.search_candidates(q.entity, &search_pattern(q.search))
        {
            cand = intersect(cand, restricted(set));
        }
        cand.exact = None;
    }

    let snap = p.snap.clone();

    let mut eval_includes: &[u8] = b"";
    if p.inc {
        if let Some(filters) = q.filters {
            for field in filters.keys() {
                if p.affected_by_includes(field) {
                    eval_includes = q.includes;
                }
            }
        }
        if !q.search.is_empty() && !p.refs.is_empty() {
            eval_includes = q.includes;
        }
    }

    let mut res = ListResult {
        snap: Some(snap.clone()),
        guard: q.guard,
        docs: Vec::new(),
        total: 0,
        err: None,
    };

    let heavy = needs_match && heavy_plan(&snap, &cand, eval_includes);
    let search_on_bytes = !q.search.is_empty() && eval_includes.is_empty();
    let only_search = search_on_bytes && !has_filter && !has_node;

    let mut v = Verifier {
        needs_match,
        cand: &cand,
        filters: q.filters,
        node: q.node,
        search: q.search,
        guard: q.guard,
        heavy,
        search_on_bytes,
        only_search,
        lowered_search: lowered_search_pattern(q.search),
        eval_includes,
        reader: include_reader(),
        verified: 0,
        expired: false,
        started: Instant::now(),
        release: None,
    };

    if let Some(each) = each.as_mut() {
        p.order_for(&q, &cand, needs_match, &mut |d| v.verify(d), &mut |d| {
            each(d)
        });
    } else if q.count_only
        && q.sorts.is_empty()
        && q.sort_field.is_empty()
        && q.limit == 0
        && q.offset == 0
    {
        res.total = count_matches(&snap, &cand, needs_match, everything_exact, &mut |d| {
            v.verify(d)
        });
    } else if let Some((docs, total)) = if needs_match { None } else { p.direct_page(&q) } {
        res.docs = docs;
        res.total = total;
    } else {
        let (docs, total) = paginate(
            |emit| p.order_for(&q, &cand, needs_match, &mut |d| v.verify(d), emit),
            q.offset,
            q.limit,
            q.count_only,
        );
        res.docs = docs;
        res.total = total;
    }

    let expired = v.expired;
    drop(v);

    if expired || guard::err(q.guard).is_some() {
        res.err = Some(guard::err(q.guard).unwrap_or(GuardError::Deadline));
        res.docs = Vec::new();
        res.total = 0;
    }

    res
}

const HEAVY_CANDIDATES: usize = 2048;
pub(crate) const GUARD_INTERVAL: usize = 64;
const DEFAULT_QUEUE_PER_WORKER: i64 = 4;
const HOOK_QUEUE_PER_WORKER: i64 = 32;
const DEFAULT_QUEUE_TIMEOUT: Duration = Duration(5 * Duration::SECOND.0);
const HEAVY_LANE_POLL_INTERVAL: StdDuration = StdDuration::from_millis(20);
const HEAVY_LANE_AFTER: StdDuration = StdDuration::from_millis(5);
const HEAVY_ENVELOPE: i64 = 8 << 20;

pub struct Slots {
    capacity: usize,
    taken: Mutex<usize>,
    freed: Condvar,
}

impl Slots {
    fn try_take(&self) -> bool {
        let mut taken = self.taken.lock();
        if *taken < self.capacity {
            *taken += 1;
            return true;
        }

        false
    }

    fn take_until(&self, until: Instant) -> bool {
        let mut taken = self.taken.lock();
        loop {
            if *taken < self.capacity {
                *taken += 1;
                return true;
            }

            let (next, timed_out) = self.freed.wait_until(taken, until);
            taken = next;
            if timed_out {
                if *taken < self.capacity {
                    *taken += 1;
                    return true;
                }
                return false;
            }
        }
    }

    fn release(&self) {
        let mut taken = self.taken.lock();
        *taken = taken.saturating_sub(1);
        self.freed.notify_one();
    }
}

pub struct LaneSlot(Option<Arc<Slots>>);

impl Drop for LaneSlot {
    fn drop(&mut self) {
        if let Some(slots) = self.0.take() {
            slots.release();
        }
    }
}

struct Lane {
    slots: Mutex<Option<Arc<Slots>>>,
    waiting: AtomicI64,
    size_fn: fn() -> usize,
    bit: u32,
    per_worker: i64,
}

static QUERIES: Lane = Lane {
    slots: Mutex::new(None),
    waiting: AtomicI64::new(0),
    size_fn: heavy_lane_size,
    bit: 1,
    per_worker: DEFAULT_QUEUE_PER_WORKER,
};

static HOOKS: Lane = Lane {
    slots: Mutex::new(None),
    waiting: AtomicI64::new(0),
    size_fn: hook_lane_size,
    bit: 2,
    per_worker: HOOK_QUEUE_PER_WORKER,
};

fn hook_lane_size() -> usize {
    1.max((3 * globals::procs()).div_ceil(8))
}

pub fn heavy_lane_size() -> usize {
    if let Some(cfg) = globals::config()
        && cfg.api.query.workers > 0
    {
        return cfg.api.query.workers as usize;
    }

    1.max(globals::procs() / 2)
}

fn queue_limit(size: usize, per_worker: i64) -> i64 {
    if let Some(cfg) = globals::config() {
        if cfg.api.query.max_queued > 0 {
            return cfg.api.query.max_queued;
        }
        if cfg.api.query.max_queued < 0 {
            return -1;
        }
    }

    size as i64 * per_worker
}

pub(crate) fn heavy_queue_timeout() -> Duration {
    if let Some(cfg) = globals::config() {
        if cfg.api.query.queue_timeout_ms > 0 {
            return Duration(
                cfg.api
                    .query
                    .queue_timeout_ms
                    .wrapping_mul(Duration::MILLISECOND.0),
            );
        }
        if cfg.api.query.queue_timeout_ms < 0 {
            return Duration(-1);
        }
    }

    DEFAULT_QUEUE_TIMEOUT
}

pub fn queue_timeout() -> Duration {
    heavy_queue_timeout()
}

pub fn heavy_waiting() -> i64 {
    QUERIES.waiting.load(Ordering::SeqCst) + HOOKS.waiting.load(Ordering::SeqCst)
}

impl Lane {
    fn current(&self) -> Arc<Slots> {
        let size = (self.size_fn)();
        let mut slots = self.slots.lock();
        match slots.as_ref() {
            Some(s) if s.capacity == size => s.clone(),
            _ => {
                let s = Arc::new(Slots {
                    capacity: size,
                    taken: Mutex::new(0),
                    freed: Condvar::new(),
                });
                *slots = Some(s.clone());
                s
            }
        }
    }

    fn acquire(&self, guard: Option<&Guard>) -> Result<LaneSlot, GuardError> {
        if guard.is_some_and(|g| g.laned) {
            return Ok(LaneSlot(None));
        }

        let slots = self.current();

        guard::reserve(guard, HEAVY_ENVELOPE)?;

        let first = guard.is_none_or(|g| g.enter_lane(self.bit));

        if slots.try_take() {
            metrics::heavy_query(StdDuration::ZERO);
            return Ok(LaneSlot(Some(slots)));
        }

        let patient = guard.is_none_or(|g| g.admitted);
        let limit = queue_limit(slots.capacity, self.per_worker);
        if first && !patient && limit >= 0 && self.waiting.load(Ordering::SeqCst) >= limit {
            metrics::heavy_rejected();
            return Err(guard::reject(guard));
        }

        struct Waiting<'a>(Option<&'a AtomicI64>);
        impl Drop for Waiting<'_> {
            fn drop(&mut self) {
                if let Some(w) = self.0 {
                    w.fetch_sub(1, Ordering::SeqCst);
                }
            }
        }
        let _waiting = if first {
            self.waiting.fetch_add(1, Ordering::SeqCst);
            Waiting(Some(&self.waiting))
        } else {
            Waiting(None)
        };

        let start = Instant::now();
        let mut timeout = heavy_queue_timeout();
        if patient {
            timeout = Duration(-1);
        }

        let mut tick = start + HEAVY_LANE_POLL_INTERVAL;
        loop {
            if slots.take_until(tick) {
                metrics::heavy_query(start.elapsed());
                return Ok(LaneSlot(Some(slots)));
            }

            let now = Instant::now();
            while tick <= now {
                tick += HEAVY_LANE_POLL_INTERVAL;
            }

            if let Err(err) = guard::check(guard) {
                metrics::heavy_query(start.elapsed());
                return Err(err);
            }

            if timeout.0 >= 0 && start.elapsed() >= StdDuration::from_nanos(timeout.0 as u64) {
                metrics::heavy_rejected();
                return Err(guard::reject(guard));
            }
        }
    }
}

fn heavy_plan(snap: &crate::datastore::Snapshot, cand: &Candidates, eval_includes: &[u8]) -> bool {
    if !eval_includes.is_empty() {
        return snap.len() > HEAVY_CANDIDATES / 8;
    }

    if cand.all {
        return snap.len() > HEAVY_CANDIDATES;
    }

    let mut maybe = cand.set.count() as i64;
    if let Some(exact) = &cand.exact {
        maybe -= exact.count() as i64;
    }

    maybe > HEAVY_CANDIDATES as i64
}

pub fn acquire_hook_lane(guard: Option<&Guard>) -> Result<LaneSlot, GuardError> {
    HOOKS.acquire(guard)
}

fn acquire_heavy_lane(guard: Option<&Guard>) -> Result<LaneSlot, GuardError> {
    QUERIES.acquire(guard)
}

pub fn deadline_from_config() -> Option<Instant> {
    if let Some(cfg) = globals::config()
        && cfg.api.query.timeout_ms > 0
    {
        return Some(Instant::now() + StdDuration::from_millis(cfg.api.query.timeout_ms as u64));
    }

    None
}

impl Planner {
    fn direct_page(&mut self, q: &ListQuery<'_, '_>) -> Option<(Vec<Doc>, usize)> {
        let sorts = effective_sorts(q);
        if sorts.len() > 1 || q.offset < 0 {
            return None;
        }

        let snap = self.snap.clone();
        let total = snap.len();
        let mut docs: Vec<Doc> = Vec::new();
        let limit = q.limit;
        let take = |d: Doc, docs: &mut Vec<Doc>| {
            docs.push(d);
            limit <= 0 || (docs.len() as i64) < limit
        };

        if sorts.is_empty() {
            snap.ascend(q.offset, |d| take(d, &mut docs));
            return Some((docs, total));
        }

        if !self.refs.is_empty() && self.sort_path_crosses_reference(&sorts[0].field) {
            return None;
        }

        let view = self.index(&IndexSpec::new(INDEX_SORT, sorts[0].field.clone()))?;

        let snap = self.snap.clone();
        walk_sort_index_from(&view, sorts[0].ascending, q.offset, &mut |seq| {
            let Some(d) = snap.by_seq(seq) else {
                return true;
            };
            take(d, &mut docs)
        });

        Some((docs, snap.len()))
    }

    fn order_for(
        &mut self,
        q: &ListQuery<'_, '_>,
        cand: &Candidates,
        needs_match: bool,
        verify: &mut dyn FnMut(&Doc) -> bool,
        emit: &mut dyn FnMut(Doc) -> bool,
    ) {
        let mut snap = self.snap.clone();
        let sorts = effective_sorts(q);

        if sorts.is_empty() {
            if !needs_match {
                snap.ascend(0, emit);
                return;
            }

            iterate_candidates(&snap, cand, &mut |d| {
                if !verify(&d) {
                    return true;
                }
                emit(d)
            });
            return;
        }

        let primary = &sorts[0];
        let sort_affected =
            !self.refs.is_empty() && self.sort_path_crosses_reference(&primary.field);
        if !sort_affected
            && let Some(view) = self.index(&IndexSpec::new(INDEX_SORT, primary.field.clone()))
        {
            snap = self.snap.clone();
            let mut accept = |seq: u32| -> Option<Doc> {
                if needs_match && !cand.all && !cand.set.has(seq) {
                    return None;
                }

                if !needs_match || cand.exact.as_ref().is_some_and(|e| e.has(seq)) {
                    return Some(snap.indexed(seq));
                }

                let d = snap.by_seq(seq)?;
                if needs_match && !verify(&d) {
                    return None;
                }

                Some(d)
            };

            if sorts.len() == 1 {
                walk_sort_index(&view, primary.ascending, &mut |seq| match accept(seq) {
                    None => true,
                    Some(d) => emit(d),
                });
                return;
            }

            walk_sort_groups(&view, primary.ascending, &mut |seqs| {
                let docs: Vec<Doc> = seqs.iter().filter_map(|&seq| accept(seq)).collect();
                for d in sort_group(docs, &sorts[1..]) {
                    if !emit(d) {
                        return false;
                    }
                }
                true
            });
            return;
        }

        let mut reader = include_reader();
        let mut rows: Vec<(Doc, Map)> = Vec::new();
        iterate_candidates(&snap, cand, &mut |d| {
            if needs_match && !verify(&d) {
                return true;
            }

            let Ok(doc) = d.decode() else {
                return true;
            };

            let mut docs = [doc];
            if sort_affected {
                apply_includes_with(&mut docs, b"all", &mut reader);
            }
            let [doc] = docs;
            rows.push((d, doc));

            true
        });

        let order = sorted_order(&rows, &sorts);
        let mut rows: Vec<Option<Doc>> = rows.into_iter().map(|(d, _)| Some(d)).collect();
        for i in order {
            if let Some(d) = rows[i].take()
                && !emit(d)
            {
                return;
            }
        }
    }
}

fn sorted_order(rows: &[(Doc, Map)], sorts: &[Sort]) -> Vec<usize> {
    let vals: Vec<Vec<SortValue<'_, Value>>> = rows
        .iter()
        .map(|(_, doc)| {
            sorts
                .iter()
                .map(|s| get_sort_nested_value(doc, &s.field))
                .collect()
        })
        .collect();

    let mut order: Vec<usize> = (0..rows.len()).collect();
    gosort::slice_stable(&mut order, |&i, &j| {
        for (k, s) in sorts.iter().enumerate() {
            if let Some(less) = sort_step(&vals[i][k], &vals[j][k], s.ascending) {
                return less;
            }
        }

        false
    });

    order
}

fn sort_group(docs: Vec<Doc>, sorts: &[Sort]) -> Vec<Doc> {
    if docs.len() < 2 || sorts.is_empty() {
        return docs;
    }

    let mut rows: Vec<(Doc, Map)> = Vec::with_capacity(docs.len());
    for d in docs {
        let Ok(data) = d.decode() else {
            continue;
        };
        rows.push((d, data));
    }

    let order = sorted_order(&rows, sorts);
    let mut rows: Vec<Option<Doc>> = rows.into_iter().map(|(d, _)| Some(d)).collect();

    order.into_iter().filter_map(|i| rows[i].take()).collect()
}

fn count_matches(
    snap: &crate::datastore::Snapshot,
    cand: &Candidates,
    needs_match: bool,
    everything_exact: bool,
    verify: &mut dyn FnMut(&Doc) -> bool,
) -> usize {
    if !needs_match || everything_exact {
        return snap.len();
    }

    if !cand.all
        && let Some(exact) = &cand.exact
    {
        let mut set = cand.set.clone();
        set.and_not(exact);
        if set.count() == 0 {
            return exact.count();
        }
    }

    let mut n = 0;
    iterate_candidates(snap, cand, &mut |d| {
        if verify(&d) {
            n += 1;
        }
        true
    });

    n
}

fn iterate_candidates(
    snap: &crate::datastore::Snapshot,
    cand: &Candidates,
    f: &mut dyn FnMut(Doc) -> bool,
) {
    if cand.all || cand.set.count() * 16 > snap.len() {
        snap.ascend(0, f);
        return;
    }

    cand.set.for_each(|seq| {
        let Some(d) = snap.by_seq(seq) else {
            return true;
        };
        f(d)
    });
}

fn paginate(
    src: impl FnOnce(&mut dyn FnMut(Doc) -> bool),
    offset: i64,
    limit: i64,
    count_all: bool,
) -> (Vec<Doc>, usize) {
    let offset = offset.max(0);

    let mut docs: Vec<Doc> = Vec::new();
    let mut n: i64 = 0;
    src(&mut |d| {
        if n >= offset && (limit <= 0 || (docs.len() as i64) < limit) {
            docs.push(d);
        }
        n += 1;

        count_all || limit <= 0 || (docs.len() as i64) < limit || n < offset
    });

    (docs, n as usize)
}

fn group_end(view: &IndexView, i: i64) -> i64 {
    let Some((key, _)) = view.at(i) else {
        return i;
    };

    view.rank(&after_term(key_payload(&key))) - 1
}

fn group_start(view: &IndexView, i: i64) -> i64 {
    let Some((key, _)) = view.at(i) else {
        return i;
    };

    view.rank(key_payload(&key))
}

fn emit_range(view: &IndexView, start: i64, end: i64, f: &mut dyn FnMut(u32) -> bool) -> bool {
    let mut i = start;
    let mut stopped = false;
    view.ascend_at(start, |_, seq| {
        if i > end {
            return false;
        }
        i += 1;
        if !f(seq) {
            stopped = true;
            return false;
        }
        true
    });

    !stopped
}

pub(crate) fn walk_sort_index_from(
    view: &IndexView,
    ascending: bool,
    mut skip: i64,
    f: &mut dyn FnMut(u32) -> bool,
) {
    let missing_start = view.rank(&[SORT_RANK_MISSING]);
    let total = view.len();

    if ascending {
        view.ascend_at(skip, |_, seq| f(seq));
        return;
    }

    let mut end = missing_start - 1;
    while end >= 0 {
        let start = group_start(view, end);
        let size = end - start + 1;
        if skip >= size {
            skip -= size;
            end = start - 1;
            continue;
        }

        if !emit_range(view, start + skip, end, f) {
            return;
        }

        skip = 0;
        end = start - 1;
    }

    if missing_start + skip < total {
        view.ascend_at(missing_start + skip, |_, seq| f(seq));
    }
}

fn walk_sort_index(view: &IndexView, ascending: bool, f: &mut dyn FnMut(u32) -> bool) {
    walk_sort_index_from(view, ascending, 0, f);
}

fn walk_sort_groups(view: &IndexView, ascending: bool, f: &mut dyn FnMut(&[u32]) -> bool) {
    let missing_start = view.rank(&[SORT_RANK_MISSING]);
    let total = view.len();

    let mut group = |start: i64, end: i64| -> bool {
        let mut seqs: Vec<u32> = Vec::with_capacity((end - start + 1).max(0) as usize);
        emit_range(view, start, end, &mut |seq| {
            seqs.push(seq);
            true
        });
        f(&seqs)
    };

    if ascending {
        let mut i = 0;
        while i < missing_start {
            let end = group_end(view, i);
            if !group(i, end) {
                return;
            }
            i = end + 1;
        }
    } else {
        let mut end = missing_start - 1;
        while end >= 0 {
            let start = group_start(view, end);
            if !group(start, end) {
                return;
            }
            end = start - 1;
        }
    }

    if missing_start < total {
        group(missing_start, total - 1);
    }
}

fn decode_map_or_nil(b: &[u8]) -> Option<Map> {
    decode_map(b).ok().flatten()
}

pub type SeqFilter = Box<dyn Fn(u32) -> bool + Send + Sync>;

impl Planner {
    fn stored_leaf(&mut self, field: &[u8], ops: &Ops) -> Option<SeqFilter> {
        if self.affected_by_includes(field) {
            return None;
        }

        let view = self.filter_index(field)?;

        let mut present = Bitset::default();
        let key = [KEY_PRESENT];
        collect(&view, &key, &after_term(&key), &mut present, None);

        let c = self.leaf(field, ops);
        if c.all {
            return None;
        }

        let set = c.set;
        Some(Box::new(move |seq| !present.has(seq) || set.has(seq)))
    }

    fn stored_fields(&mut self, filters: &Filters) -> Option<SeqFilter> {
        let mut parts: Vec<SeqFilter> = Vec::new();
        for (field, ops) in filters {
            if let Some(f) = self.stored_leaf(field, ops) {
                parts.push(f);
            }
        }

        if parts.is_empty() {
            return None;
        }

        Some(Box::new(move |seq| parts.iter().all(|f| f(seq))))
    }

    fn stored_node(&mut self, n: &FilterNode) -> Option<SeqFilter> {
        struct Frame<'a> {
            children: &'a [FilterNode],
            is_and: bool,
            next: usize,
            parts: Vec<SeqFilter>,
        }

        let mut stack: Vec<Frame<'_>> = Vec::new();
        let mut cur = n;
        loop {
            let mut result: Option<SeqFilter> = if let Some(leaf) = &cur.leaf {
                self.stored_fields(leaf)
            } else {
                let (children, is_and) = if cur.and.is_empty() {
                    (cur.or.as_slice(), false)
                } else {
                    (cur.and.as_slice(), true)
                };
                if children.is_empty() {
                    Some(Box::new(|_| false))
                } else {
                    stack.push(Frame {
                        children,
                        is_and,
                        next: 0,
                        parts: Vec::new(),
                    });
                    cur = &children[0];
                    continue;
                }
            };

            loop {
                let Some(top) = stack.last_mut() else {
                    return result;
                };

                match result.take() {
                    None if !top.is_and => {
                        stack.pop();
                        continue;
                    }
                    None => {}
                    Some(f) => top.parts.push(f),
                }

                top.next += 1;
                if top.next < top.children.len() {
                    cur = &top.children[top.next];
                    break;
                }

                let done = stack.pop()?;
                if done.parts.is_empty() {
                    result = None;
                    continue;
                }

                let parts = done.parts;
                let is_and = done.is_and;
                result = Some(Box::new(move |seq| {
                    for f in &parts {
                        if f(seq) != is_and {
                            return !is_and;
                        }
                    }
                    is_and
                }));
            }
        }
    }
}

pub fn stored_candidates(
    snap: Option<&crate::datastore::Snapshot>,
    filters: Option<&Filters>,
    node: Option<&FilterNode>,
    includes: &[u8],
) -> Option<SeqFilter> {
    let e = db()?;
    let snap = snap?;

    let mut p = Planner::new(e, snap.clone(), !includes.is_empty());
    if let Some(node) = node {
        return p.stored_node(node);
    }

    p.stored_fields(filters?)
}
