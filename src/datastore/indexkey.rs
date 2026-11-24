use crate::docpath::{self, SortValue};
use crate::gocompat::fmt::{self as gofmt, Arg};
use crate::gocompat::json::{Map, Value};
use crate::gocompat::strconv;
use crate::gocompat::strings;
use crate::gocompat::time::Time;
use crate::json_struct;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IndexKind(pub u8);

pub const INDEX_FILTER: IndexKind = IndexKind(b'f');
pub const INDEX_SORT: IndexKind = IndexKind(b's');
pub const INDEX_REFS: IndexKind = IndexKind(b'r');
pub const INDEX_SEARCH: IndexKind = IndexKind(b'g');
pub const INDEX_BITMAP: IndexKind = IndexKind(b'b');
pub const INDEX_COMPOSITE: IndexKind = IndexKind(b'c');

#[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct IndexSpec {
    pub kind: IndexKind,
    pub field: Vec<u8>,
}

impl IndexSpec {
    pub fn new(kind: IndexKind, field: impl Into<Vec<u8>>) -> IndexSpec {
        IndexSpec {
            kind,
            field: field.into(),
        }
    }

    pub fn derived(&self) -> bool {
        self.kind == INDEX_BITMAP || self.kind == INDEX_COMPOSITE
    }

    pub fn name(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.field.len() + 2);
        out.push(self.kind.0);
        out.push(b':');
        out.extend_from_slice(&self.field);
        out
    }
}

pub fn parse_index_name(name: &[u8]) -> Option<IndexSpec> {
    if name.len() < 2 || name[1] != b':' {
        return None;
    }

    let kind = IndexKind(name[0]);
    match kind {
        INDEX_FILTER | INDEX_SORT | INDEX_REFS | INDEX_SEARCH | INDEX_BITMAP | INDEX_COMPOSITE => {
            Some(IndexSpec::new(kind, &name[2..]))
        }
        _ => None,
    }
}

pub const KEY_NUMBER: u8 = b'n';
pub const KEY_DATE: u8 = b'd';
pub const KEY_LOWER: u8 = b'l';
pub const KEY_RAW: u8 = b'r';
pub const KEY_BOOL: u8 = b'b';
pub const KEY_ELEMENT: u8 = b'a';
pub const KEY_PRESENT: u8 = b'p';

pub const SORT_RANK_NUMBER: u8 = 0;
pub const SORT_RANK_TIME: u8 = 1;
pub const SORT_RANK_STRING: u8 = 2;
pub const SORT_RANK_BOOL: u8 = 3;
pub const SORT_RANK_OTHER: u8 = 4;
pub const SORT_RANK_MISSING: u8 = 5;

pub fn append_escaped(dst: &mut Vec<u8>, s: &[u8]) {
    for &c in s {
        if c == 0 {
            dst.push(0);
            dst.push(0xff);
            continue;
        }
        dst.push(c);
    }
}

pub fn append_terminated(dst: &mut Vec<u8>, s: &[u8]) {
    append_escaped(dst, s);
    dst.push(0);
    dst.push(1);
}

pub fn terminated(prefix: u8, s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len() + 3);
    out.push(prefix);
    append_terminated(&mut out, s);
    out
}

pub fn append_number(dst: &mut Vec<u8>, f: f64) {
    let f = if f == 0.0 { 0.0 } else { f };

    let mut bits = f.to_bits();
    if bits & (1 << 63) != 0 {
        bits = !bits;
    } else {
        bits |= 1 << 63;
    }

    dst.extend_from_slice(&bits.to_be_bytes());
}

pub fn append_time(dst: &mut Vec<u8>, t: &Time) {
    dst.extend_from_slice(&((t.unix() as u64) ^ (1 << 63)).to_be_bytes());
    dst.extend_from_slice(&(t.nanosecond() as u32).to_be_bytes());
}

pub fn seq_key(seq: u32) -> [u8; 4] {
    seq.to_be_bytes()
}

pub fn key_seq(key: &[u8]) -> u32 {
    let tail = &key[key.len() - 4..];
    u32::from_be_bytes([tail[0], tail[1], tail[2], tail[3]])
}

pub fn key_payload(key: &[u8]) -> &[u8] {
    &key[..key.len() - 4]
}

pub fn entry_key(payload: &[u8], seq: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 4);
    out.extend_from_slice(payload);
    out.extend_from_slice(&seq.to_be_bytes());
    out
}

pub fn element_string(v: &Value) -> Vec<u8> {
    match v {
        Value::String(s) => s.as_bytes().to_vec(),
        Value::Number(f) => strconv::format_float(*f, b'f', -1, 64).into_bytes(),
        Value::Int(n) => strconv::itoa(*n).into_bytes(),
        _ => sprint_value(v),
    }
}

enum PrintFrame<'a> {
    Array(std::slice::Iter<'a, Value>, bool),
    Object(std::collections::btree_map::Iter<'a, String, Value>, bool),
    Maps(std::slice::Iter<'a, Map>, bool),
}

fn print_scalar(out: &mut Vec<u8>, v: &Value) {
    match v {
        Value::Null => out.extend_from_slice(b"<nil>"),
        Value::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
        Value::Number(f) => gofmt::appendf(out, "%v", &[Arg::Float(*f)]),
        Value::Int(n) => out.extend_from_slice(strconv::itoa(*n).as_bytes()),
        Value::String(s) => out.extend_from_slice(s.as_bytes()),
        _ => {}
    }
}

fn open_print<'a>(out: &mut Vec<u8>, stack: &mut Vec<PrintFrame<'a>>, v: &'a Value) {
    match v {
        Value::Array(a) => {
            out.push(b'[');
            stack.push(PrintFrame::Array(a.iter(), true));
        }
        Value::Object(m) => {
            out.extend_from_slice(b"map[");
            stack.push(PrintFrame::Object(m.iter(), true));
        }
        Value::DocList(l) => {
            out.push(b'[');
            stack.push(PrintFrame::Maps(l.iter(), true));
        }
        _ => print_scalar(out, v),
    }
}

fn sprint_value(v: &Value) -> Vec<u8> {
    let mut out = Vec::new();
    let mut stack: Vec<PrintFrame<'_>> = Vec::new();
    open_print(&mut out, &mut stack, v);

    while let Some(top) = stack.last_mut() {
        match top {
            PrintFrame::Array(it, first) => match it.next() {
                Some(item) => {
                    if !*first {
                        out.push(b' ');
                    }
                    *first = false;
                    open_print(&mut out, &mut stack, item);
                }
                None => {
                    out.push(b']');
                    stack.pop();
                }
            },
            PrintFrame::Object(it, first) => match it.next() {
                Some((k, item)) => {
                    if !*first {
                        out.push(b' ');
                    }
                    *first = false;
                    out.extend_from_slice(k.as_bytes());
                    out.push(b':');
                    open_print(&mut out, &mut stack, item);
                }
                None => {
                    out.push(b']');
                    stack.pop();
                }
            },
            PrintFrame::Maps(it, first) => match it.next() {
                Some(m) => {
                    if !*first {
                        out.push(b' ');
                    }
                    *first = false;
                    out.extend_from_slice(b"map[");
                    stack.push(PrintFrame::Object(m.iter(), true));
                }
                None => {
                    out.push(b']');
                    stack.pop();
                }
            },
        }
    }

    out
}

pub fn filter_keys(doc: Option<&Value>, field: &[u8]) -> Vec<Vec<u8>> {
    let Some(doc) = doc else {
        return Vec::new();
    };

    let values = docpath::resolve_values(doc, field);
    if values.is_empty() {
        return Vec::new();
    }

    let mut keys = vec![vec![KEY_PRESENT]];
    for value in values {
        append_value_keys(&mut keys, value);
    }

    keys
}

fn append_value_keys(keys: &mut Vec<Vec<u8>>, value: &Value) {
    match value {
        Value::String(s) => {
            let s = s.as_bytes();
            keys.push(terminated(KEY_LOWER, &strings::to_lower(s)));
            keys.push(terminated(KEY_RAW, s));
            if let Some((t, _)) = docpath::parse_date(s) {
                let mut k = vec![KEY_DATE];
                append_time(&mut k, &t);
                keys.push(k);
            }
        }
        Value::Number(f) => {
            let mut k = vec![KEY_NUMBER];
            append_number(&mut k, *f);
            keys.push(k);
        }
        Value::Int(n) => {
            let mut k = vec![KEY_NUMBER];
            append_number(&mut k, *n as f64);
            keys.push(k);
        }
        Value::Bool(b) => keys.push(vec![KEY_BOOL, u8::from(*b)]),
        Value::Array(items) => {
            for item in items {
                keys.push(terminated(KEY_ELEMENT, &element_string(item)));
            }
        }
        _ => {}
    }
}

pub fn sort_key(doc: Option<&Value>, field: &[u8]) -> Vec<u8> {
    match doc {
        None => vec![SORT_RANK_MISSING],
        Some(doc) => sort_value_key_of(&docpath::sort_value(doc, field)),
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SortInput<'a> {
    Nil,
    Number(f64),
    Time(&'a Time),
    String(&'a [u8]),
    Bool(bool),
    Other,
}

pub fn sort_value_key(v: SortInput<'_>) -> Vec<u8> {
    match v {
        SortInput::Nil => vec![SORT_RANK_MISSING],
        SortInput::Number(f) => {
            let mut k = vec![SORT_RANK_NUMBER];
            append_number(&mut k, f);
            k
        }
        SortInput::Time(t) => {
            let mut k = vec![SORT_RANK_TIME];
            append_time(&mut k, t);
            k
        }
        SortInput::String(s) => terminated(SORT_RANK_STRING, s),
        SortInput::Bool(b) => vec![SORT_RANK_BOOL, u8::from(b)],
        SortInput::Other => vec![SORT_RANK_OTHER],
    }
}

pub fn sort_input<'a>(v: &'a SortValue<'a, Value>) -> SortInput<'a> {
    match v {
        SortValue::Nil => SortInput::Nil,
        SortValue::Time(t) => SortInput::Time(t),
        SortValue::Value(n) => match n {
            Value::Null => SortInput::Nil,
            Value::Number(f) => SortInput::Number(*f),
            Value::Int(i) => SortInput::Number(*i as f64),
            Value::String(s) => SortInput::String(s.as_bytes()),
            Value::Bool(b) => SortInput::Bool(*b),
            _ => SortInput::Other,
        },
    }
}

pub fn sort_value_key_of(v: &SortValue<'_, Value>) -> Vec<u8> {
    sort_value_key(sort_input(v))
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
    pub struct RefPath {
        pub path: Vec<u8> => "Path",
        pub target: Vec<u8> => "Target",
    }
}

pub fn is_reference(m: &Map) -> Option<(&[u8], &[u8])> {
    let target = match m.get("@entity") {
        Some(Value::String(s)) if !s.is_empty() => s.as_bytes(),
        _ => return None,
    };

    let id = match m.get("id") {
        Some(Value::String(s)) => s.as_bytes(),
        _ => b"".as_slice(),
    };

    Some((target, id))
}

fn child_path(path: &[u8], key: &str) -> Vec<u8> {
    if path.is_empty() {
        return key.as_bytes().to_vec();
    }

    let mut p = Vec::with_capacity(path.len() + 1 + key.len());
    p.extend_from_slice(path);
    p.push(b'.');
    p.extend_from_slice(key.as_bytes());
    p
}

fn walk_refs<'a>(doc: Option<&'a Value>, mut found: impl FnMut(&[u8], &'a [u8], &'a [u8])) {
    let Some(doc) = doc else {
        return;
    };

    let mut stack: Vec<(&'a Value, Vec<u8>)> = vec![(doc, Vec::new())];
    while let Some((v, path)) = stack.pop() {
        match v {
            Value::Object(m) => {
                if !path.is_empty()
                    && let Some((target, id)) = is_reference(m)
                {
                    found(&path, target, id);
                    continue;
                }

                for (k, child) in m.iter().rev() {
                    stack.push((child, child_path(&path, k)));
                }
            }
            Value::Array(items) => {
                for item in items.iter().rev() {
                    stack.push((item, path.clone()));
                }
            }
            _ => {}
        }
    }
}

pub fn document_refs(doc: Option<&Value>) -> Vec<RefPath> {
    let mut out = Vec::new();
    walk_refs(doc, |path, target, _| {
        out.push(RefPath {
            path: path.to_vec(),
            target: target.to_vec(),
        });
    });
    out
}

pub fn ref_keys(doc: Option<&Value>) -> Vec<Vec<u8>> {
    let mut keys = Vec::new();
    walk_refs(doc, |path, target, id| keys.push(ref_key(path, target, id)));
    keys
}

pub fn ref_key(path: &[u8], target: &[u8], id: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    append_terminated(&mut b, path);
    append_terminated(&mut b, target);
    append_terminated(&mut b, id);
    b
}

pub fn ref_prefix(path: &[u8], target: &[u8]) -> Vec<u8> {
    let mut b = Vec::new();
    append_terminated(&mut b, path);
    append_terminated(&mut b, target);
    b
}

#[doc(hidden)]
pub fn extract_keys(spec: &IndexSpec, doc: Option<&Value>) -> Vec<Vec<u8>> {
    if doc.is_none() {
        return Vec::new();
    }

    match spec.kind {
        INDEX_FILTER | INDEX_BITMAP => filter_keys(doc, &spec.field),
        INDEX_SORT => vec![sort_key(doc, &spec.field)],
        INDEX_REFS => ref_keys(doc),
        _ => Vec::new(),
    }
}

#[doc(hidden)]
pub fn unique_keys(mut keys: Vec<Vec<u8>>) -> Vec<Vec<u8>> {
    keys.sort_unstable();
    keys.dedup();
    keys
}

pub(crate) fn diff_sorted(old: &[Vec<u8>], new: &[Vec<u8>]) -> (Vec<Vec<u8>>, Vec<Vec<u8>>) {
    let mut removed = Vec::new();
    let mut added = Vec::new();
    let (mut i, mut j) = (0, 0);
    while i < old.len() || j < new.len() {
        if j >= new.len() || (i < old.len() && old[i] < new[j]) {
            removed.push(old[i].clone());
            i += 1;
        } else if i >= old.len() || new[j] < old[i] {
            added.push(new[j].clone());
            j += 1;
        } else {
            i += 1;
            j += 1;
        }
    }

    (removed, added)
}
