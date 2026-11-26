use std::collections::{BTreeMap, BTreeSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::gocompat::json::Value;
use crate::pagetree::{Token, Tree, UpsertOut};
use crate::sync::Mutex;

use super::bitmap::bitmap_key;
use super::composite::{IRREGULAR_KEY, add_irregular, composite_keys};
use super::engine::{ENGINE_LOG, Engine, Snapshot, panic_text};
use super::entity::{Entity, Index};
use super::indexkey::{
    INDEX_BITMAP, INDEX_COMPOSITE, INDEX_SEARCH, IndexSpec, diff_sorted, extract_keys, seq_key,
    unique_keys,
};
use super::trigram::{
    POSTING_CHUNK, POSTING_CHUNK_BITS, Posting, diff_grams, posting_add, posting_key,
    posting_remove, search_grams, set_bit_in_place,
};
use super::tx::Tx;

pub const BULK_BATCH_RECORDS: usize = 64;
pub const SETTLE_INLINE_OPS: i64 = 4096;
pub const SETTLE_MAX_PASSES: usize = 3;
pub const BACKLOG_IDLE_AFTER: Duration = Duration::from_millis(300);
pub const BACKLOG_TICK: Duration = Duration::from_millis(100);
pub const BACKLOG_OP_OVERHEAD: i64 = 48;
pub const BACKLOG_MAX_WAIT: Duration = Duration::from_secs(30);
pub const BACKLOG_WAIT_POLL: Duration = Duration::from_millis(10);

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OpKind {
    Put = 0,
    Delete = 1,
    BitmapAdd = 2,
    BitmapRemove = 3,
    PostingAdd = 4,
    IrregularUp = 5,
    IrregularDown = 6,
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexOp {
    pub key: Vec<u8>,
    pub seq: u32,
    pub kind: OpKind,
}

#[derive(Default)]
pub(crate) struct BacklogState {
    pub(crate) ops: BTreeMap<Vec<u8>, Vec<IndexOp>>,
    pub(crate) bytes: i64,
    pub(crate) running: Option<BTreeSet<Vec<u8>>>,
    pub(crate) dropped: Option<BTreeSet<Vec<u8>>>,
}

#[derive(Default)]
pub(crate) struct IndexBacklog {
    pub(crate) mu: Mutex<BacklogState>,
    pub(crate) pending: AtomicI64,
    pub(crate) last_op: AtomicI64,
    pub(crate) lagging: AtomicBool,
    pub(crate) pass_mu: Mutex<()>,
}

fn with_seq(k: &[u8], seq: u32) -> Vec<u8> {
    let mut key = Vec::with_capacity(k.len() + 4);
    key.extend_from_slice(k);
    key.extend_from_slice(&seq_key(seq));
    key
}

#[doc(hidden)]
pub fn change_ops(
    spec: &IndexSpec,
    seq: u32,
    old: Option<&Value>,
    new: Option<&Value>,
) -> Vec<IndexOp> {
    let chunk = seq >> POSTING_CHUNK_BITS;

    if spec.kind == INDEX_SEARCH {
        let (_, added) = diff_grams(&search_grams(old), &search_grams(new));
        return added
            .into_iter()
            .map(|g| IndexOp {
                key: posting_key(g, chunk).to_vec(),
                seq,
                kind: OpKind::PostingAdd,
            })
            .collect();
    }

    if spec.kind == INDEX_COMPOSITE {
        let (old_keys, old_irregular) = composite_keys(spec, old);
        let (new_keys, new_irregular) = composite_keys(spec, new);
        let (removed, added) = diff_sorted(&old_keys, &new_keys);
        let mut ops = Vec::with_capacity(removed.len() + added.len() + 1);
        for k in &removed {
            ops.push(IndexOp {
                key: with_seq(k, seq),
                seq,
                kind: OpKind::Delete,
            });
        }
        for k in &added {
            ops.push(IndexOp {
                key: with_seq(k, seq),
                seq,
                kind: OpKind::Put,
            });
        }
        if old_irregular && !new_irregular {
            ops.push(IndexOp {
                key: IRREGULAR_KEY.to_vec(),
                seq,
                kind: OpKind::IrregularDown,
            });
        } else if !old_irregular && new_irregular {
            ops.push(IndexOp {
                key: IRREGULAR_KEY.to_vec(),
                seq,
                kind: OpKind::IrregularUp,
            });
        }
        return ops;
    }

    let old_keys = unique_keys(extract_keys(spec, old));
    let new_keys = unique_keys(extract_keys(spec, new));
    if old_keys == new_keys {
        return Vec::new();
    }

    let (removed, added) = diff_sorted(&old_keys, &new_keys);
    let mut ops = Vec::with_capacity(removed.len() + added.len());
    if spec.kind == INDEX_BITMAP {
        for v in &removed {
            ops.push(IndexOp {
                key: bitmap_key(v, chunk),
                seq,
                kind: OpKind::BitmapRemove,
            });
        }
        for v in &added {
            ops.push(IndexOp {
                key: bitmap_key(v, chunk),
                seq,
                kind: OpKind::BitmapAdd,
            });
        }
        return ops;
    }

    for k in &removed {
        ops.push(IndexOp {
            key: with_seq(k, seq),
            seq,
            kind: OpKind::Delete,
        });
    }
    for k in &added {
        ops.push(IndexOp {
            key: with_seq(k, seq),
            seq,
            kind: OpKind::Put,
        });
    }

    ops
}

#[doc(hidden)]
pub fn apply_ops(mut tree: Tree, ops: &mut [IndexOp]) -> Tree {
    ops.sort_by(|a, b| a.key.cmp(&b.key));

    for op in ops.iter() {
        let off = (op.seq & (POSTING_CHUNK as u32 - 1)) as u16;
        match op.kind {
            OpKind::Put => tree = tree.put(&op.key, &[]).0,
            OpKind::Delete => tree = tree.delete(&op.key).0,
            OpKind::BitmapAdd => {
                tree = tree.upsert(&op.key, |c| match c {
                    None => match posting_add(&[], off) {
                        Posting::New(n) => UpsertOut::Replace(n),
                        _ => UpsertOut::Replace(Vec::new()),
                    },
                    Some(c) => match posting_add(c, off) {
                        Posting::New(n) => UpsertOut::Replace(n),
                        _ => UpsertOut::InPlace,
                    },
                });
            }
            OpKind::BitmapRemove => {
                let next = {
                    let Some(cur) = tree.get(&op.key) else {
                        continue;
                    };
                    match posting_remove(&cur, off) {
                        Posting::Empty => None,
                        Posting::Same => Some(cur.to_vec()),
                        Posting::New(n) => Some(n),
                    }
                };
                let Some(next) = next else {
                    tree = tree.delete(&op.key).0;
                    continue;
                };
                tree = tree.update(&op.key, move |_| UpsertOut::Replace(next)).0;
            }
            OpKind::PostingAdd => {
                tree = tree.upsert(&op.key, |c| match c {
                    None => match posting_add(&[], off) {
                        Posting::New(n) => UpsertOut::Replace(n),
                        _ => UpsertOut::Replace(Vec::new()),
                    },
                    Some(c) => {
                        if c[0] == 1 {
                            set_bit_in_place(c, off);
                            return UpsertOut::InPlace;
                        }
                        match posting_add(c, off) {
                            Posting::New(n) => UpsertOut::Replace(n),
                            _ => UpsertOut::InPlace,
                        }
                    }
                });
            }
            OpKind::IrregularUp => tree = add_irregular(tree, 1),
            OpKind::IrregularDown => tree = add_irregular(tree, -1),
        }
    }

    tree
}

fn ops_bytes(ops: &[IndexOp]) -> i64 {
    ops.iter()
        .map(|op| op.key.len() as i64 + BACKLOG_OP_OVERHEAD)
        .sum()
}

fn now_nanos() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i64,
        Err(e) => -(e.duration().as_nanos() as i64),
    }
}

impl IndexBacklog {
    pub(crate) fn active(&self) -> bool {
        let b = self.mu.lock();
        !b.ops.is_empty() || b.running.is_some()
    }

    pub(crate) fn is_running(&self, name: &[u8]) -> bool {
        let b = self.mu.lock();
        b.running.as_ref().is_some_and(|r| r.contains(name))
    }

    pub(crate) fn lag_names(&self) -> Option<BTreeSet<Vec<u8>>> {
        let b = self.mu.lock();
        let running = b.running.as_ref().map_or(0, |r| r.len());
        if b.ops.is_empty() && running == 0 {
            return None;
        }

        let mut out: BTreeSet<Vec<u8>> = b.ops.keys().cloned().collect();
        if let Some(r) = &b.running {
            out.extend(r.iter().cloned());
        }

        Some(out)
    }

    fn take(&self) -> (BTreeMap<Vec<u8>, Vec<IndexOp>>, i64) {
        let mut b = self.mu.lock();
        let work = std::mem::take(&mut b.ops);
        let size = b.bytes;
        b.bytes = 0;
        self.pending.store(0, Ordering::SeqCst);
        b.running = Some(work.keys().cloned().collect());
        b.dropped = None;

        (work, size)
    }
}

impl Engine {
    pub(crate) fn merge_backlog(&self, ent: &Entity, queued: BTreeMap<Vec<u8>, Vec<IndexOp>>) {
        let b = &ent.backlog;
        let mut st = b.mu.lock();

        let mut count = 0i64;
        let mut size = 0i64;
        for (name, ops) in queued {
            count += ops.len() as i64;
            size += ops_bytes(&ops);
            st.ops.entry(name).or_default().extend(ops);
        }

        st.bytes += size;
        b.pending.fetch_add(count, Ordering::SeqCst);
        b.last_op.store(now_nanos(), Ordering::SeqCst);
        b.lagging.store(true, Ordering::SeqCst);
        self.backlog_bytes.fetch_add(size, Ordering::SeqCst);
    }

    pub(crate) fn forget_backlog(&self, ent: &Entity, name: &[u8]) {
        let b = &ent.backlog;
        let mut st = b.mu.lock();

        if let Some(ops) = st.ops.remove(name) {
            let size = ops_bytes(&ops);
            st.bytes -= size;
            b.pending.fetch_sub(ops.len() as i64, Ordering::SeqCst);
            self.backlog_bytes.fetch_sub(size, Ordering::SeqCst);
        }

        if st.running.as_ref().is_some_and(|r| r.contains(name)) {
            st.dropped
                .get_or_insert_with(BTreeSet::new)
                .insert(name.to_vec());
        }
    }

    pub(crate) fn forget_all_backlog(&self, ent: &Entity) {
        let b = &ent.backlog;
        let mut st = b.mu.lock();

        self.backlog_bytes.fetch_sub(st.bytes, Ordering::SeqCst);
        st.ops.clear();
        st.bytes = 0;
        b.pending.store(0, Ordering::SeqCst);
        if let Some(running) = st.running.clone() {
            st.dropped.get_or_insert_with(BTreeSet::new).extend(running);
        }
    }

    pub(crate) fn run_index_pass(&self, ent: &Arc<Entity>) -> bool {
        let b = &ent.backlog;
        let _pass = b.pass_mu.lock();

        if b.pending.load(Ordering::SeqCst) == 0 {
            return false;
        }

        let (work, size, base) = {
            let _g = ent.apply.lock();
            let (work, size) = b.take();
            (work, size, ent.root.load_full())
        };

        struct Settle<'a>(&'a Engine, i64);
        impl Drop for Settle<'_> {
            fn drop(&mut self) {
                self.0.backlog_bytes.fetch_sub(self.1, Ordering::SeqCst);
            }
        }
        let _settle = Settle(self, size);

        let mut results: BTreeMap<Vec<u8>, Tree> = BTreeMap::new();
        let mut work = work;
        let outcome = catch_unwind(AssertUnwindSafe(|| {
            let tok = Token::new();
            struct Freeze<'a>(&'a Token);
            impl Drop for Freeze<'_> {
                fn drop(&mut self) {
                    self.0.freeze();
                }
            }
            let _freeze = Freeze(&tok);

            for (name, ops) in work.iter_mut() {
                let Some(idx) = base.indexes.get(name) else {
                    continue;
                };
                if !idx.ready {
                    continue;
                }

                results.insert(
                    name.clone(),
                    apply_ops(idx.tree.with_token(Some(&tok)), ops).with_token(None),
                );
            }
        }));
        let failed = match outcome {
            Ok(()) => false,
            Err(payload) => {
                let text = panic_text(payload.as_ref());
                ENGINE_LOG.error(
                    "an index could not be brought up to date, it will be rebuilt",
                    crate::kv!["entity" => ent.name.as_slice(), "err" => text],
                );
                true
            }
        };

        let _g = ent.apply.lock();

        let cur = ent.root.load_full();
        let mut nr = (*cur).clone();
        let mut indexes = (*cur.indexes).clone();

        let (dropped, still) = {
            let mut st = b.mu.lock();
            let dropped = st.dropped.take();
            st.running = None;
            (dropped, !st.ops.is_empty())
        };

        if cur.gen_ == base.gen_ {
            for name in work.keys() {
                let Some(idx) = cur.indexes.get(name) else {
                    continue;
                };
                if !idx.ready || dropped.as_ref().is_some_and(|d| d.contains(name)) {
                    continue;
                }

                if failed {
                    indexes.insert(name.clone(), Index::dormant(idx.spec.clone()));
                    continue;
                }

                if let Some(tree) = results.remove(name) {
                    indexes.insert(name.clone(), Index::ready(idx.spec.clone(), tree));
                }
            }
        }

        nr.indexes = Arc::new(indexes);
        nr.lagging = still;
        b.lagging.store(still, Ordering::SeqCst);
        ent.root.store(Arc::new(nr));

        true
    }

    pub fn indexed_snapshot(&self, name: &[u8]) -> Snapshot {
        match self.entity(name, false) {
            Some(ent) if ent.backlog.lagging.load(Ordering::SeqCst) => self.settle(&ent, name),
            _ => self.snapshot(name),
        }
    }

    fn settle(&self, ent: &Arc<Entity>, name: &[u8]) -> Snapshot {
        let b = &ent.backlog;
        let mut i = 0;
        while i < SETTLE_MAX_PASSES && b.pending.load(Ordering::SeqCst) >= SETTLE_INLINE_OPS {
            self.run_index_pass(ent);
            i += 1;
        }

        let _pass = b.pass_mu.lock();
        let _g = ent.apply.lock();

        let cur = ent.root.load_full();
        if !cur.lagging {
            return Snapshot::new(self.arc(), Some(ent.clone()), cur, name);
        }

        let (mut work, size) = b.take();
        self.backlog_bytes.fetch_sub(size, Ordering::SeqCst);

        let mut nr = (*cur).clone();
        let mut indexes = (*cur.indexes).clone();

        let tok = Token::new();
        for (iname, ops) in work.iter_mut() {
            let Some(idx) = cur.indexes.get(iname) else {
                continue;
            };
            if !idx.ready {
                continue;
            }

            indexes.insert(iname.clone(), Index::dormant(idx.spec.clone()));
            let tree = idx.tree.with_token(Some(&tok));
            match catch_unwind(AssertUnwindSafe(|| apply_ops(tree, ops).with_token(None))) {
                Ok(t) => {
                    indexes.insert(iname.clone(), Index::ready(idx.spec.clone(), t));
                }
                Err(payload) => {
                    let text = panic_text(payload.as_ref());
                    ENGINE_LOG.error(
                        "an index could not be brought up to date, it will be rebuilt",
                        crate::kv!["entity" => ent.name.as_slice(), "err" => text],
                    );
                }
            }
        }
        tok.freeze();

        {
            let mut st = b.mu.lock();
            st.running = None;
            st.dropped = None;
        }

        nr.indexes = Arc::new(indexes);
        nr.lagging = false;
        b.lagging.store(false, Ordering::SeqCst);
        let nr = Arc::new(nr);
        ent.root.store(nr.clone());

        Snapshot::new(self.arc(), Some(ent.clone()), nr, name)
    }

    pub(crate) fn backlog_limit(&self) -> i64 {
        self.dirty_flush_bytes() / 2
    }

    pub(crate) fn wait_for_backlog(&self) {
        if !self.opts.background {
            return;
        }

        let limit = 2 * self.backlog_limit();
        if self.backlog_bytes.load(Ordering::SeqCst) < limit {
            return;
        }

        let deadline = Instant::now() + BACKLOG_MAX_WAIT;
        while self.backlog_bytes.load(Ordering::SeqCst) >= limit
            && !self.closed.load(Ordering::SeqCst)
            && Instant::now() < deadline
        {
            std::thread::sleep(BACKLOG_WAIT_POLL);
        }
    }

    pub(crate) fn backlog_loop(&self) {
        self.every(BACKLOG_TICK, || {
            self.guarded("index upkeep", || self.drain_backlogs());
        });
    }

    pub(crate) fn drain_backlogs(&self) {
        let idle_since = now_nanos() - BACKLOG_IDLE_AFTER.as_nanos() as i64;
        let mut largest: Option<Arc<Entity>> = None;
        let mut largest_bytes = 0i64;
        for ent in self.all_entities() {
            let b = &ent.backlog;
            if b.pending.load(Ordering::SeqCst) == 0 {
                continue;
            }

            if b.last_op.load(Ordering::SeqCst) <= idle_since {
                self.run_index_pass(&ent);
                continue;
            }

            let size = b.mu.lock().bytes;
            if size > largest_bytes {
                largest_bytes = size;
                largest = Some(ent.clone());
            }
        }

        if let Some(ent) = largest
            && self.backlog_bytes.load(Ordering::SeqCst) >= self.backlog_limit()
        {
            self.run_index_pass(&ent);
        }
    }

    pub(crate) fn settle_all(&self) {
        for ent in self.all_entities() {
            self.guarded("index upkeep", || {
                while ent.backlog.pending.load(Ordering::SeqCst) > 0 {
                    if !self.run_index_pass(&ent) {
                        return;
                    }
                }
            });
        }
    }

    pub fn index_backlog_bytes(&self) -> i64 {
        self.backlog_bytes.load(Ordering::SeqCst)
    }

    pub(crate) fn read_ahead(&self, tx: &Tx<'_>) {
        let mut ids: BTreeMap<&[u8], Vec<&[u8]>> = BTreeMap::new();
        for (entity, id) in tx.doc_ops() {
            ids.entry(entity).or_default().push(id);
        }

        for (name, mut list) in ids {
            let Some(ent) = self.entity(name, false) else {
                continue;
            };

            list.sort_unstable();
            let r = ent.root.load_full();
            for id in list {
                let _ = r.slot_by_id(id);
            }
        }
    }
}
