use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};

use arc_swap::{ArcSwap, ArcSwapOption};

use crate::gocompat::binary;
use crate::gocompat::errors::BoxError;
use crate::gocompat::json::{self, Map, Value};
use crate::journal::{self, Location, Record};
use crate::pagetree::{Token, Tree};
use crate::sync::{Mutex, RwLock};

use super::backlog::{IndexBacklog, IndexOp, change_ops};
use super::bitmap::update_bitmap;
use super::changes::{ChangeLog, FieldStamps, PendingStamp, Watched};
use super::composite::apply_composite_change;
use super::engine::Engine;
use super::indexkey::{
    INDEX_BITMAP, INDEX_COMPOSITE, INDEX_SEARCH, IndexSpec, RefPath, diff_sorted, document_refs,
    extract_keys, seq_key, unique_keys,
};
use super::indexmeta::IndexMeta;
use super::trigram::{STALE_SEARCH_SLACK, diff_grams, search_grams, update_postings};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Slot {
    pub seq: u32,
    pub id: Vec<u8>,
    pub lsn: u64,
    pub loc: Location,
}

#[doc(hidden)]
pub fn encode_slot(dst: &mut Vec<u8>, s: &Slot) {
    binary::append_uvarint(dst, s.lsn);
    binary::append_uvarint(dst, u64::from(s.loc.segment));
    binary::append_uvarint(dst, u64::from(s.loc.offset));
    binary::append_uvarint(dst, u64::from(s.loc.length));
    dst.extend_from_slice(&s.id);
}

#[doc(hidden)]
pub fn decode_slot_value(seq: u32, b: &[u8]) -> Slot {
    let mut pos = 0usize;
    let mut next = || {
        let (v, n) = binary::uvarint(&b[pos..]);
        if n <= 0 {
            return 0;
        }
        pos += n as usize;
        v
    };

    let lsn = next();
    let segment = next() as u32;
    let offset = next() as u32;
    let length = next() as u32;

    Slot {
        seq,
        id: b[pos..].to_vec(),
        lsn,
        loc: Location {
            segment,
            offset,
            length,
        },
    }
}

pub(crate) fn seq_of(b: &[u8]) -> u32 {
    u32::from_be_bytes([b[0], b[1], b[2], b[3]])
}

#[derive(Clone)]
pub(crate) struct Index {
    pub(crate) spec: IndexSpec,
    pub(crate) ready: bool,
    pub(crate) dormant: bool,
    pub(crate) tree: Tree,
}

impl Index {
    pub(crate) fn ready(spec: IndexSpec, tree: Tree) -> Arc<Index> {
        Arc::new(Index {
            spec,
            ready: true,
            dormant: false,
            tree,
        })
    }

    pub(crate) fn dormant(spec: IndexSpec) -> Arc<Index> {
        Arc::new(Index {
            spec,
            ready: false,
            dormant: true,
            tree: Tree::default(),
        })
    }

    pub(crate) fn building(spec: IndexSpec) -> Arc<Index> {
        Arc::new(Index {
            spec,
            ready: false,
            dormant: false,
            tree: Tree::default(),
        })
    }
}

pub(crate) type Indexes = BTreeMap<Vec<u8>, Arc<Index>>;

#[derive(Clone)]
pub(crate) struct Root {
    pub(crate) gen_: u64,
    pub(crate) next_seq: u32,
    pub(crate) slots: Tree,
    pub(crate) ids: Tree,
    pub(crate) indexes: Arc<Indexes>,
    pub(crate) version: u64,
    pub(crate) refs: Arc<Vec<RefPath>>,
    pub(crate) lagging: bool,
}

impl Root {
    pub(crate) fn slot_by_seq(&self, seq: u32) -> Option<Slot> {
        let b = self.slots.get(&seq_key(seq))?;
        Some(decode_slot_value(seq, &b))
    }

    pub(crate) fn slot_by_id(&self, id: &[u8]) -> Option<Slot> {
        let b = self.ids.get(id)?;
        self.slot_by_seq(seq_of(&b))
    }
}

pub(crate) struct BuildState {
    pub(crate) order: u64,
    pub(crate) pass: Option<u64>,
}

#[derive(Default)]
pub(crate) struct BuildPass {
    pub(crate) before: BTreeMap<u32, Option<Arc<Value>>>,
}

#[derive(Default)]
pub(crate) struct EntityState {
    pub(crate) building: BTreeMap<Vec<u8>, BuildState>,
    pub(crate) passes: BTreeMap<u64, BuildPass>,
    pub(crate) next_pass: u64,
}

pub(crate) struct Entity {
    pub(crate) name: Vec<u8>,
    pub(crate) apply: Mutex<EntityState>,
    pub(crate) root: ArcSwap<Root>,
    pub(crate) queued: AtomicBool,
    pub(crate) usage: RwLock<HashMap<Vec<u8>, Arc<AtomicI64>>>,
    pub(crate) metas: RwLock<HashMap<Vec<u8>, Arc<IndexMeta>>>,
    pub(crate) pins: RwLock<HashMap<Vec<u8>, Arc<AtomicI64>>>,
    pub(crate) changes: ChangeLog,
    pub(crate) watched: ArcSwapOption<Watched>,
    pub(crate) stale: AtomicI64,
    pub(crate) advised: AtomicI64,
    pub(crate) backlog: IndexBacklog,
}

impl Engine {
    pub(crate) fn new_entity(&self, name: &[u8]) -> Arc<Entity> {
        Arc::new(Entity {
            name: name.to_vec(),
            apply: Mutex::new(EntityState::default()),
            root: ArcSwap::new(Arc::new(self.empty_root(0, 0))),
            queued: AtomicBool::new(false),
            usage: RwLock::new(HashMap::new()),
            metas: RwLock::new(HashMap::new()),
            pins: RwLock::new(HashMap::new()),
            changes: ChangeLog::default(),
            watched: ArcSwapOption::empty(),
            stale: AtomicI64::new(0),
            advised: AtomicI64::new(0),
            backlog: IndexBacklog::default(),
        })
    }

    pub(crate) fn empty_root(&self, gen_: u64, version: u64) -> Root {
        Root {
            gen_,
            next_seq: 0,
            slots: self.new_tree(),
            ids: self.new_tree(),
            indexes: Arc::new(BTreeMap::new()),
            version,
            refs: Arc::new(Vec::new()),
            lagging: false,
        }
    }
}

pub(crate) type DocRef = Option<Arc<Value>>;

pub(crate) struct Draft<'a> {
    pub(crate) e: &'a Engine,
    pub(crate) ent: &'a Arc<Entity>,
    pub(crate) st: &'a mut EntityState,
    pub(crate) r: Root,
    pub(crate) indexes: Arc<Indexes>,
    pub(crate) cloned: bool,
    pub(crate) refs_seen: BTreeSet<RefPath>,
    pub(crate) released: Vec<Tree>,
    pub(crate) tok: Token,
    pub(crate) stamps: Vec<PendingStamp>,
    pub(crate) floors: Vec<Arc<FieldStamps>>,
    pub(crate) deferred: bool,
    pub(crate) ops: BTreeMap<Vec<u8>, Vec<IndexOp>>,
}

impl<'a> Draft<'a> {
    pub(crate) fn new(e: &'a Engine, ent: &'a Arc<Entity>, st: &'a mut EntityState) -> Draft<'a> {
        let mut r = (**ent.root.load()).clone();
        let tok = Token::new();
        r.slots = r.slots.with_token(Some(&tok));
        r.ids = r.ids.with_token(Some(&tok));
        let indexes = r.indexes.clone();
        let deferred = e.opts.background && ent.backlog.active();

        Draft {
            e,
            ent,
            st,
            r,
            indexes,
            cloned: false,
            refs_seen: BTreeSet::new(),
            released: Vec::new(),
            tok,
            stamps: Vec::new(),
            floors: Vec::new(),
            deferred,
            ops: BTreeMap::new(),
        }
    }

    fn mutable_indexes(&mut self) -> &mut Indexes {
        if !self.cloned {
            self.indexes = Arc::new((*self.indexes).clone());
            self.cloned = true;
        }

        Arc::make_mut(&mut self.indexes)
    }

    pub(crate) fn publish(mut self) -> Vec<Tree> {
        self.tok.freeze();
        self.r.slots = self.r.slots.with_token(None);
        self.r.ids = self.r.ids.with_token(None);
        if self.cloned {
            let m = Arc::make_mut(&mut self.indexes);
            for idx in m.values_mut() {
                if idx.tree.root().is_some() {
                    *idx = Arc::new(Index {
                        spec: idx.spec.clone(),
                        ready: idx.ready,
                        dormant: idx.dormant,
                        tree: idx.tree.with_token(None),
                    });
                }
            }
        }

        self.r.indexes = self.indexes.clone();
        if !self.refs_seen.is_empty() {
            let mut refs: Vec<RefPath> = (*self.r.refs).clone();
            refs.extend(std::mem::take(&mut self.refs_seen));
            refs.sort();
            self.r.refs = Arc::new(refs);
        }

        if !self.ops.is_empty() {
            let ops = std::mem::take(&mut self.ops);
            self.e.merge_backlog(self.ent, ops);
            self.r.lagging = true;
        }
        self.ent
            .backlog
            .lagging
            .store(self.r.lagging, Ordering::SeqCst);

        self.ent.root.store(Arc::new(self.r.clone()));
        self.apply_stamps();

        std::mem::take(&mut self.released)
    }

    fn has_ref(&self, rf: &RefPath) -> bool {
        self.refs_seen.contains(rf) || self.r.refs.iter().any(|r| r == rf)
    }

    fn needs_decode(&self) -> bool {
        !self.indexes.is_empty()
            || !self.st.building.is_empty()
            || !self.st.passes.is_empty()
            || self.ent.watched.load().is_some()
    }

    pub(crate) fn put(&mut self, rec: &Record, doc: DocRef, replay: bool) {
        if rec.r#gen != self.r.gen_ {
            return;
        }

        let old = self.r.slot_by_id(&rec.key);
        if let Some(o) = &old
            && replay
            && rec.lsn <= o.lsn
        {
            return;
        }

        if old.is_none() && rec.kind == journal::KIND_DOC_MOVE {
            return;
        }

        let mut s = Slot {
            seq: 0,
            id: rec.key.clone(),
            lsn: rec.lsn,
            loc: rec.loc,
        };
        self.e.cache.put(rec.lsn, Arc::from(rec.value.as_slice()));
        self.e.live.add(rec.loc.segment, i64::from(rec.loc.length));

        match &old {
            Some(o) => {
                s.seq = o.seq;
                self.e.cache.drop_lsn(o.lsn);
                self.e.live.add(o.loc.segment, -i64::from(o.loc.length));
            }
            None => {
                s.seq = self.r.next_seq;
                self.r.next_seq = self.r.next_seq.wrapping_add(1);
                self.r.ids = self.r.ids.put(&s.id, &seq_key(s.seq)).0;
            }
        }

        let mut enc = Vec::with_capacity(s.id.len() + 16);
        encode_slot(&mut enc, &s);
        self.r.slots = self.r.slots.put(&seq_key(s.seq), &enc).0;

        if rec.kind == journal::KIND_DOC_MOVE {
            return;
        }

        self.ent.changes.add(self.r.version, rec.lsn, s.seq);
        self.r.version = rec.lsn;

        let doc: DocRef = match doc {
            Some(d) => Some(d),
            None => decode_doc(&rec.value),
        };

        for rf in document_refs(doc.as_deref()) {
            if !self.has_ref(&rf) {
                self.refs_seen.insert(rf);
            }
        }

        if !self.needs_decode() {
            return;
        }

        let old_doc = match &old {
            Some(o) => self.e.decode_old(o),
            None => None,
        };

        let old_known = old.is_none() || old_doc.is_some();
        self.stamp_change(old_doc.as_deref(), doc.as_deref(), old_known);
        self.update_indexes(s.seq, old_doc, doc);
    }

    pub(crate) fn del(&mut self, rec: &Record, replay: bool) {
        if rec.r#gen != self.r.gen_ {
            return;
        }

        let Some(old) = self.r.slot_by_id(&rec.key) else {
            return;
        };
        if replay && old.lsn >= rec.lsn {
            return;
        }

        let old_doc = if self.needs_decode() {
            self.e.decode_old(&old)
        } else {
            None
        };

        self.r.slots = self.r.slots.delete(&seq_key(old.seq)).0;
        self.r.ids = self.r.ids.delete(&old.id).0;
        self.ent.changes.add(self.r.version, rec.lsn, old.seq);
        self.r.version = rec.lsn;
        self.e.cache.drop_lsn(old.lsn);
        self.e.live.add(old.loc.segment, -i64::from(old.loc.length));

        let known = old_doc.is_some();
        self.stamp_change(old_doc.as_deref(), None, known);
        if old_doc.is_some() {
            self.update_indexes(old.seq, old_doc, None);
        }
    }

    pub(crate) fn reset(&mut self, rec: &Record) {
        if rec.r#gen <= self.r.gen_ {
            return;
        }

        let mut indexes: Indexes = BTreeMap::new();
        for (name, idx) in self.indexes.iter() {
            indexes.insert(
                name.clone(),
                Index::ready(idx.spec.clone(), self.e.new_tree()),
            );
        }
        self.st.building.clear();

        if !self.r.slots.is_empty() {
            self.released.push(self.r.slots.clone());
            self.e
                .reclaim
                .store(super::compact::RECLAIM_ROUNDS, Ordering::SeqCst);
        }

        self.ops.clear();
        self.e.forget_all_backlog(self.ent);
        self.ent.changes.reset();
        let mut fresh = self.e.empty_root(rec.r#gen, rec.lsn);
        fresh.slots = fresh.slots.with_token(Some(&self.tok));
        fresh.ids = fresh.ids.with_token(Some(&self.tok));
        self.r = fresh;
        self.indexes = Arc::new(indexes);
        self.cloned = true;
        self.refs_seen.clear();
    }

    fn queue(&mut self, name: &[u8], ops: Vec<IndexOp>) {
        if ops.is_empty() {
            return;
        }

        self.ops.entry(name.to_vec()).or_default().extend(ops);
    }

    fn update_indexes(&mut self, seq: u32, old: DocRef, new: DocRef) {
        for pass in self.st.passes.values_mut() {
            pass.before.entry(seq).or_insert_with(|| old.clone());
        }

        if self.indexes.is_empty() {
            return;
        }

        let mut changed: BTreeMap<Vec<u8>, Arc<Index>> = BTreeMap::new();
        let indexes = self.indexes.clone();
        for (name, idx) in indexes.iter() {
            if !idx.ready {
                continue;
            }

            if idx.spec.kind == INDEX_SEARCH && old.is_some() {
                let stale = self.ent.stale.fetch_add(1, Ordering::SeqCst) + 1;
                if stale > 2 * self.r.slots.len() as i64 + STALE_SEARCH_SLACK {
                    self.ent.stale.store(0, Ordering::SeqCst);
                    changed.insert(name.clone(), Index::dormant(idx.spec.clone()));
                    self.ops.remove(name);
                    self.e.forget_backlog(self.ent, name);
                    continue;
                }
            }

            if self.deferred {
                let ops = change_ops(&idx.spec, seq, old.as_deref(), new.as_deref());
                self.queue(name, ops);
                continue;
            }

            let (tree, ok) = apply_doc_change(
                &idx.spec,
                idx.tree.with_token(Some(&self.tok)),
                seq,
                old.as_deref(),
                new.as_deref(),
            );
            if !ok {
                continue;
            }

            changed.insert(name.clone(), Index::ready(idx.spec.clone(), tree));
        }

        if changed.is_empty() {
            return;
        }

        let m = self.mutable_indexes();
        for (name, idx) in changed {
            m.insert(name, idx);
        }
    }
}

pub(crate) fn apply_doc_change(
    spec: &IndexSpec,
    mut tree: Tree,
    seq: u32,
    old: Option<&Value>,
    new: Option<&Value>,
) -> (Tree, bool) {
    if spec.kind == INDEX_SEARCH {
        let (_, added) = diff_grams(&search_grams(old), &search_grams(new));
        if added.is_empty() {
            return (tree, false);
        }

        return (update_postings(tree, seq, &[], &added), true);
    }

    if spec.kind == INDEX_COMPOSITE {
        return apply_composite_change(spec, tree, seq, old, new);
    }

    let old_keys = unique_keys(extract_keys(spec, old));
    let new_keys = unique_keys(extract_keys(spec, new));
    if old_keys == new_keys {
        return (tree, false);
    }

    let (removed, added) = diff_sorted(&old_keys, &new_keys);
    if spec.kind == INDEX_BITMAP {
        return (update_bitmap(tree, seq, &removed, &added), true);
    }

    let sk = seq_key(seq);
    for k in &removed {
        let mut key = k.clone();
        key.extend_from_slice(&sk);
        tree = tree.delete(&key).0;
    }

    for k in &added {
        let mut key = k.clone();
        key.extend_from_slice(&sk);
        tree = tree.put(&key, &[]).0;
    }

    (tree, true)
}

pub fn decode_map(b: &[u8]) -> Result<Option<Map>, json::Error> {
    let mut doc: Option<Map> = None;
    json::unmarshal(b, &mut doc)?;
    Ok(doc)
}

#[doc(hidden)]
pub fn decode_doc(b: &[u8]) -> DocRef {
    match decode_map(b) {
        Ok(Some(m)) => Some(Arc::new(Value::Object(m))),
        _ => None,
    }
}

impl Engine {
    pub(crate) fn slot_bytes(&self, s: &Slot) -> Result<Arc<[u8]>, BoxError> {
        if let Some(b) = self.cache.get(s.lsn) {
            return Ok(b);
        }

        let b: Arc<[u8]> = Arc::from(self.journal().read_value(s.loc)?);
        self.cache.put(s.lsn, b.clone());

        Ok(b)
    }

    pub(crate) fn decode_old(&self, s: &Slot) -> DocRef {
        if let Some(b) = self.cache.get(s.lsn) {
            return decode_doc(&b);
        }

        match self.journal().read_value(s.loc) {
            Ok(b) => decode_doc(&b),
            Err(_) => None,
        }
    }

    pub(crate) fn decode_slot(&self, s: &Slot) -> DocRef {
        match self.slot_bytes(s) {
            Ok(b) => decode_doc(&b),
            Err(_) => None,
        }
    }
}

#[derive(Default)]
pub(crate) struct LiveBytes {
    m: RwLock<HashMap<u32, Arc<AtomicI64>>>,
}

impl LiveBytes {
    pub(crate) fn add(&self, seg: u32, n: i64) {
        if n == 0 {
            return;
        }

        let found = self.m.read().get(&seg).cloned();
        let v = match found {
            Some(v) => v,
            None => self.m.write().entry(seg).or_default().clone(),
        };
        v.fetch_add(n, Ordering::SeqCst);
    }

    pub(crate) fn snapshot(&self) -> BTreeMap<u32, i64> {
        let mut out = BTreeMap::new();
        for (&k, v) in self.m.read().iter() {
            let n = v.load(Ordering::SeqCst);
            if n > 0 {
                out.insert(k, n);
            }
        }
        out
    }

    pub(crate) fn reset(&self, values: &BTreeMap<u32, i64>) {
        self.m.write().clear();
        for (&seg, &n) in values {
            self.add(seg, n);
        }
    }
}
