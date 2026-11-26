use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::gocompat::errors::BoxError;
use crate::gocompat::sort as gosort;
use crate::gocompat::time::Duration as GoDuration;
use crate::log;
use crate::memory;
use crate::pagetree::{Token, Tree};

use super::bitmap::bitmap_key;
use super::composite::{add_irregular, composite_keys};
use super::engine::{BUILD_QUEUE, ENGINE_LOG, Engine, Error};
use super::entity::{
    BuildPass, DocRef, Entity, EntityState, Index, Root, apply_doc_change, decode_doc,
};
use super::indexkey::{
    INDEX_BITMAP, INDEX_COMPOSITE, INDEX_SEARCH, IndexSpec, extract_keys, seq_key, unique_keys,
};
use super::trigram::{
    POSTING_CHUNK, POSTING_CHUNK_BITS, Posting, posting_add, posting_key, search_grams,
};

pub const SMALL_ENTITY_DOCS: usize = 512;
pub const MAX_BATCH_ENTRIES: usize = 2_000_000;
pub const MAX_BATCH_INDEXES: usize = 32;
pub const BUILD_ENTRY_BYTES: i64 = 72;
pub const SEARCH_DOC_BYTES: i64 = 640;
pub const SMALL_BUILD_BYTES: i64 = 1 << 20;
pub const INDEX_IDLE_AFTER: GoDuration = GoDuration::HOUR;
pub const IDLE_SWEEP_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

pub(crate) fn unix_now() -> i64 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_secs() as i64,
        Err(e) => -(e.duration().as_secs_f64().ceil() as i64),
    }
}

pub(crate) fn unix_nanos_now() -> i128 {
    match SystemTime::now().duration_since(UNIX_EPOCH) {
        Ok(d) => d.as_nanos() as i128,
        Err(e) => -(e.duration().as_nanos() as i128),
    }
}

impl Entity {
    pub(crate) fn touch(&self, name: &[u8]) {
        self.meta(name).uses.fetch_add(1, Ordering::SeqCst);
        let now = unix_now();
        let found = self.usage.read().get(name).cloned();
        if let Some(last) = found {
            if last.load(Ordering::SeqCst) != now {
                last.store(now, Ordering::SeqCst);
            }
            return;
        }

        self.usage
            .write()
            .entry(name.to_vec())
            .or_insert_with(|| Arc::new(AtomicI64::new(now)));
    }

    pub(crate) fn last_use(&self, name: &[u8]) -> Option<i64> {
        self.usage
            .read()
            .get(name)
            .map(|c| c.load(Ordering::SeqCst))
    }
}

struct Claimed {
    name: Vec<u8>,
    spec: IndexSpec,
    order: u64,
}

impl Engine {
    pub(crate) fn schedule(&self, ent: &Arc<Entity>) -> bool {
        if !self.opts.background || self.closed.load(Ordering::SeqCst) {
            return false;
        }

        if ent
            .queued
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_err()
        {
            return true;
        }

        let mut q = self.builds.q.lock();
        if q.len() >= BUILD_QUEUE {
            drop(q);
            ent.queued.store(false, Ordering::SeqCst);
            return false;
        }

        q.push_back(ent.clone());
        self.builds.cv.notify_one();
        true
    }

    pub(crate) fn build_worker(&self) {
        loop {
            let ent = {
                let mut q = self.builds.q.lock();
                loop {
                    if self.bell.is_stopped() {
                        return;
                    }
                    if let Some(ent) = q.pop_front() {
                        break ent;
                    }
                    q = self.builds.cv.wait(q);
                }
            };

            self.guarded("index build", || self.build_entity(&ent, true));
        }
    }

    pub(crate) fn build_entity(&self, ent: &Arc<Entity>, background: bool) {
        if background {
            ent.queued.store(false, Ordering::SeqCst);
        }

        loop {
            let more = self.build_pass(ent);
            if !more {
                return;
            }

            if background && self.schedule(ent) {
                return;
            }
        }
    }

    fn claim(&self, ent: &Arc<Entity>) -> Option<(Arc<Root>, u64, Vec<Claimed>, bool)> {
        let mut st = ent.apply.lock();
        let snap = ent.root.load_full();

        let mut pending: Vec<Claimed> = Vec::new();
        let mut gone: Vec<Vec<u8>> = Vec::new();
        for (name, bs) in st.building.iter() {
            if bs.pass.is_some() {
                continue;
            }

            match snap.indexes.get(name) {
                Some(idx) if !idx.ready && !idx.dormant => pending.push(Claimed {
                    name: name.clone(),
                    spec: idx.spec.clone(),
                    order: bs.order,
                }),
                _ => gone.push(name.clone()),
            }
        }
        for name in gone {
            st.building.remove(&name);
        }

        gosort::slice(&mut pending, |a, b| a.order < b.order);

        let n = snap.slots.len();
        let mut limit = MAX_BATCH_INDEXES.min((MAX_BATCH_ENTRIES / n.max(1)).max(1));
        let budget = memory::BUILDS.limit();
        if budget > 0 {
            let per = (n.max(1) as i64) * BUILD_ENTRY_BYTES;
            limit = limit.min(((budget / per) as usize).max(1));
        }
        let more = pending.len() > limit;
        if more {
            pending.truncate(limit);
        }

        if pending.is_empty() {
            return None;
        }

        st.next_pass += 1;
        let pass = st.next_pass;
        for c in &pending {
            if let Some(bs) = st.building.get_mut(&c.name) {
                bs.pass = Some(pass);
            }
        }
        st.passes.insert(pass, BuildPass::default());

        Some((snap, pass, pending, more))
    }

    fn build_pass(&self, ent: &Arc<Entity>) -> bool {
        let Some((snap, pass, jobs, more)) = self.claim(ent) else {
            return false;
        };

        let start = Instant::now();
        let search_job = jobs.iter().rposition(|j| j.spec.kind == INDEX_SEARCH);

        let len = snap.slots.len() as i64;
        let mut need = len * jobs.len() as i64 * BUILD_ENTRY_BYTES;
        if search_job.is_some() {
            need += len * SEARCH_DOC_BYTES;
        }

        struct Release(i64);
        impl Drop for Release {
            fn drop(&mut self) {
                memory::BUILDS.release(self.0);
            }
        }
        let mut _reserved = None;
        if need >= SMALL_BUILD_BYTES {
            let stop = || -> Result<(), BoxError> {
                if self.closed.load(Ordering::SeqCst) {
                    return Err(Box::new(Error::Closed));
                }
                Ok(())
            };
            if memory::BUILDS
                .acquire(need, GoDuration(-1), Some(&stop))
                .is_err()
            {
                self.abandon_pass(ent, pass, &jobs);
                return false;
            }
            _reserved = Some(Release(need));
        }

        let mut keys: Vec<Vec<Vec<u8>>> = vec![Vec::new(); jobs.len()];
        let mut irregular: BTreeMap<usize, i64> = BTreeMap::new();
        let mut bitmaps: BTreeMap<usize, BTreeMap<Vec<u8>, Vec<u8>>> = BTreeMap::new();
        for (i, j) in jobs.iter().enumerate() {
            if j.spec.kind == INDEX_BITMAP {
                bitmaps.insert(i, BTreeMap::new());
            }
        }
        let mut containers: Option<BTreeMap<[u8; 7], Vec<u8>>> =
            search_job.map(|_| BTreeMap::new());

        let mut docs = 0usize;
        let view =
            super::engine::Snapshot::new(self.arc(), Some(ent.clone()), snap.clone(), &ent.name);
        view.scan_values(|d, raw| {
            let seq = d.seq();
            let doc: DocRef = match raw {
                Ok(b) => decode_doc(b),
                Err(_) => None,
            };
            let doc = doc.as_deref();

            for (i, j) in jobs.iter().enumerate() {
                if Some(i) == search_job {
                    continue;
                }
                if let Some(m) = bitmaps.get_mut(&i) {
                    let chunk = seq >> POSTING_CHUNK_BITS;
                    let off = (seq & (POSTING_CHUNK as u32 - 1)) as u16;
                    for k in unique_keys(extract_keys(&j.spec, doc)) {
                        let key = bitmap_key(&k, chunk);
                        let cur = m.entry(key).or_default();
                        if let Posting::New(n) = posting_add(cur, off) {
                            *cur = n;
                        }
                    }
                    continue;
                }
                if j.spec.kind == INDEX_COMPOSITE {
                    let (ck, irr) = composite_keys(&j.spec, doc);
                    for mut k in ck {
                        k.extend_from_slice(&seq_key(seq));
                        keys[i].push(k);
                    }
                    if irr {
                        *irregular.entry(i).or_default() += 1;
                    }
                    continue;
                }
                for mut k in unique_keys(extract_keys(&j.spec, doc)) {
                    k.extend_from_slice(&seq_key(seq));
                    keys[i].push(k);
                }
            }

            if let Some(c) = containers.as_mut()
                && doc.is_some()
            {
                let chunk = seq >> POSTING_CHUNK_BITS;
                let off = (seq & (POSTING_CHUNK as u32 - 1)) as u16;
                for g in search_grams(doc) {
                    let cur = c.entry(posting_key(g, chunk)).or_default();
                    if let Posting::New(n) = posting_add(cur, off) {
                        *cur = n;
                    }
                }
            }

            docs += 1;
            true
        });

        let mut trees: Vec<Tree> = Vec::with_capacity(jobs.len());
        for (i, list) in keys.iter_mut().enumerate() {
            if Some(i) == search_job {
                let c = containers.take().unwrap_or_default();
                let (k, v): (Vec<[u8; 7]>, Vec<Vec<u8>>) = c.into_iter().unzip();
                trees.push(Tree::from_sorted(Some(self.loader.clone()), &k, Some(&v)));
                continue;
            }
            if let Some(m) = bitmaps.remove(&i) {
                let (k, v): (Vec<Vec<u8>>, Vec<Vec<u8>>) = m.into_iter().unzip();
                trees.push(Tree::from_sorted(Some(self.loader.clone()), &k, Some(&v)));
                continue;
            }
            let mut list = std::mem::take(list);
            list.sort_unstable();
            let mut t = Tree::from_sorted(Some(self.loader.clone()), &list, None::<&[Vec<u8>]>);
            drop(list);
            if let Some(&n) = irregular.get(&i)
                && n > 0
            {
                t = add_irregular(t.with_token(None), n);
            }
            trees.push(t);
        }

        let requeue = self.commit_pass(ent, &snap, pass, &jobs, trees);

        ENGINE_LOG.debug(
            "indexes built",
            crate::kv![
                "entity" => ent.name.as_slice(),
                "indexes" => jobs.len(),
                "documents" => docs,
                "duration_ms" => log::since(start),
            ],
        );

        more || requeue
    }

    fn abandon_pass(&self, ent: &Arc<Entity>, pass: u64, jobs: &[Claimed]) {
        let mut st = ent.apply.lock();

        st.passes.remove(&pass);
        for j in jobs {
            if let Some(bs) = st.building.get_mut(&j.name)
                && bs.order == j.order
            {
                bs.pass = None;
            }
        }
    }

    fn commit_pass(
        &self,
        ent: &Arc<Entity>,
        snap: &Arc<Root>,
        pass: u64,
        jobs: &[Claimed],
        trees: Vec<Tree>,
    ) -> bool {
        let mut guard = ent.apply.lock();
        let st: &mut EntityState = &mut guard;

        let before = st.passes.remove(&pass).unwrap_or_default().before;

        let cur = ent.root.load_full();
        let mut after: BTreeMap<u32, DocRef> = BTreeMap::new();
        let mut current = |seq: u32| -> DocRef {
            if let Some(d) = after.get(&seq) {
                return d.clone();
            }
            let doc = match cur.slot_by_seq(seq) {
                Some(s) => self.decode_slot(&s),
                None => None,
            };
            after.insert(seq, doc.clone());
            doc
        };

        let tok = Token::new();
        let mut nr: Option<(Root, super::entity::Indexes)> = None;
        let mut requeue = false;
        for (j, tree) in jobs.iter().zip(trees) {
            let Some(bs) = st.building.get_mut(&j.name) else {
                continue;
            };
            if bs.order != j.order {
                continue;
            }
            let Some(idx) = cur.indexes.get(&j.name) else {
                continue;
            };
            if idx.ready || idx.dormant {
                continue;
            }

            if cur.gen_ != snap.gen_ {
                bs.pass = None;
                requeue = true;
                continue;
            }

            let mut tree = tree.with_token(Some(&tok));
            for (&seq, old) in before.iter() {
                let now = current(seq);
                tree = apply_doc_change(&j.spec, tree, seq, old.as_deref(), now.as_deref()).0;
            }

            st.building.remove(&j.name);
            let entry = nr.get_or_insert_with(|| ((*cur).clone(), (*cur.indexes).clone()));
            entry.1.insert(
                j.name.clone(),
                Index::ready(j.spec.clone(), tree.with_token(None)),
            );
        }
        tok.freeze();

        if let Some((mut r, indexes)) = nr {
            r.indexes = Arc::new(indexes);
            ent.root.store(Arc::new(r));
        }

        requeue
    }

    pub fn drop_idle_indexes(&self, idle: GoDuration) -> usize {
        let ents = self.all_entities();

        let cutoff = unix_nanos_now() - i128::from(idle.0);
        let mut dropped = 0usize;
        for ent in ents {
            let _g = ent.apply.lock();
            let r = ent.root.load_full();
            let mut nr: Option<super::entity::Indexes> = None;
            for (name, idx) in r.indexes.iter() {
                if !idx.ready || idx.dormant || ent.pinned(&idx.spec) {
                    continue;
                }

                let Some(last) = ent.last_use(name) else {
                    ent.touch(name);
                    continue;
                };

                if i128::from(last) * 1_000_000_000 >= cutoff {
                    continue;
                }

                nr.get_or_insert_with(|| (*r.indexes).clone())
                    .insert(name.clone(), Index::dormant(idx.spec.clone()));
                self.forget_backlog(&ent, name);
                dropped += 1;
            }

            if let Some(indexes) = nr {
                let mut root = (*r).clone();
                root.indexes = Arc::new(indexes);
                ent.root.store(Arc::new(root));
            }
        }

        if dropped > 0 {
            ENGINE_LOG.debug(
                "idle indexes released",
                crate::kv!["indexes" => dropped, "idle_minutes" => idle.minutes() as i64],
            );
        }

        dropped
    }
}
