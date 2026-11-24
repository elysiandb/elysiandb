use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::durable::PAGE_HEADER_SIZE;
use crate::gocompat::time::Time;
use crate::sync::Mutex;

use super::engine::{Engine, IndexView};
use super::entity::{Entity, Index};
use super::indexkey::{
    INDEX_BITMAP, INDEX_COMPOSITE, INDEX_FILTER, INDEX_REFS, INDEX_SEARCH, INDEX_SORT, IndexKind,
    IndexSpec,
};

pub const ORIGIN_QUERY: &str = "query";
pub const ORIGIN_HOOK: &str = "hook";
pub const ORIGIN_ADVISOR: &str = "advisor";

pub(crate) struct IndexMeta {
    origin: Mutex<Vec<u8>>,
    pub(crate) uses: AtomicI64,
    pub(crate) bytes: AtomicI64,
}

impl IndexMeta {
    pub(crate) fn origin(&self) -> Vec<u8> {
        self.origin.lock().clone()
    }

    pub(crate) fn set_origin(&self, o: &[u8]) {
        *self.origin.lock() = o.to_vec();
    }
}

impl Entity {
    pub(crate) fn meta(&self, name: &[u8]) -> Arc<IndexMeta> {
        if let Some(m) = self.metas.read().get(name) {
            return m.clone();
        }

        self.metas
            .write()
            .entry(name.to_vec())
            .or_insert_with(|| {
                Arc::new(IndexMeta {
                    origin: Mutex::new(ORIGIN_QUERY.as_bytes().to_vec()),
                    uses: AtomicI64::new(0),
                    bytes: AtomicI64::new(0),
                })
            })
            .clone()
    }

    pub(crate) fn pinned(&self, spec: &IndexSpec) -> bool {
        self.pins
            .read()
            .get(&pin_key(spec))
            .is_some_and(|v| v.load(Ordering::SeqCst) > 0)
    }
}

fn pin_key(spec: &IndexSpec) -> Vec<u8> {
    match spec.kind {
        INDEX_FILTER | INDEX_BITMAP => {
            let mut k = b"filter:".to_vec();
            k.extend_from_slice(&spec.field);
            k
        }
        _ => spec.name(),
    }
}

pub fn index_kind_name(k: IndexKind) -> Vec<u8> {
    match k {
        INDEX_FILTER => b"ordered".to_vec(),
        INDEX_SORT => b"sort".to_vec(),
        INDEX_REFS => b"references".to_vec(),
        INDEX_SEARCH => b"search".to_vec(),
        INDEX_BITMAP => b"bitmap".to_vec(),
        INDEX_COMPOSITE => b"composite".to_vec(),
        other => vec![other.0],
    }
}

#[derive(Clone, Debug)]
pub struct IndexState {
    pub entity: Vec<u8>,
    pub index: Vec<u8>,
    pub kind: Vec<u8>,
    pub origin: Vec<u8>,
    pub state: &'static str,
    pub pinned: bool,
    pub entries: i64,
    pub bytes: i64,
    pub uses: i64,
    pub last_used: Time,
}

impl Engine {
    pub fn pin_index(&self, name: &[u8], spec: &IndexSpec) {
        let ent = self.entity_or_create(name);
        let key = pin_key(spec);
        let found = ent.pins.read().get(&key).cloned();
        let v = match found {
            Some(v) => v,
            None => ent.pins.write().entry(key).or_default().clone(),
        };
        v.fetch_add(1, Ordering::SeqCst);
    }

    pub fn unpin_index(&self, name: &[u8], spec: &IndexSpec) {
        let Some(ent) = self.entity(name, false) else {
            return;
        };

        let key = pin_key(spec);
        let found = ent.pins.read().get(&key).cloned();
        if let Some(v) = found
            && v.fetch_sub(1, Ordering::SeqCst) - 1 <= 0
        {
            ent.pins.write().remove(&key);
        }
    }

    pub fn ensure_index_with(&self, name: &[u8], spec: &IndexSpec, origin: &[u8]) -> bool {
        let ent = self.entity_or_create(name);
        let key = spec.name();
        if !ent.root.load().indexes.contains_key(&key) {
            ent.meta(&key).set_origin(origin);
        }

        self.ensure_index(name, spec)
    }

    pub fn index_states(&self) -> Vec<IndexState> {
        let mut out = Vec::new();
        for ent in self.all_entities() {
            for (name, idx) in ent.root.load().indexes.iter() {
                let m = ent.meta(name);
                let mut st = IndexState {
                    entity: ent.name.clone(),
                    index: name.clone(),
                    kind: index_kind_name(idx.spec.kind),
                    origin: m.origin(),
                    state: "",
                    pinned: ent.pinned(&idx.spec),
                    entries: 0,
                    bytes: m.bytes.load(Ordering::SeqCst),
                    uses: m.uses.load(Ordering::SeqCst),
                    last_used: match ent.last_use(name) {
                        Some(sec) => Time::from_unix(sec, 0),
                        None => Time::default(),
                    },
                };
                if idx.spec.kind == INDEX_SEARCH {
                    st.index = b"search".to_vec();
                }
                if idx.dormant {
                    st.state = "released";
                    st.bytes = 0;
                    st.entries = 0;
                } else if idx.ready {
                    st.state = "ready";
                    st.entries = IndexView::of(idx.clone()).len();
                } else {
                    st.state = "building";
                }
                out.push(st);
            }
        }

        out.sort_by(|a, b| a.entity.cmp(&b.entity).then_with(|| a.index.cmp(&b.index)));

        out
    }

    pub(crate) fn measure_indexes(&self) {
        for ent in self.all_entities() {
            let r = ent.root.load_full();
            for (name, idx) in r.indexes.iter() {
                if !idx.ready {
                    continue;
                }

                let mut size = 0i64;
                idx.tree.walk_stored(|_, n| {
                    size += i64::from(n) + PAGE_HEADER_SIZE;
                });
                let _ = idx.tree.for_each_dirty(|_, p| {
                    size += p.bytes();
                    Ok(())
                });

                ent.meta(name).bytes.store(size, Ordering::SeqCst);
            }
        }
    }

    pub fn release_indexes(&self) {
        for ent in self.all_entities() {
            let mut st = ent.apply.lock();
            let cur = ent.root.load_full();
            let mut r = (*cur).clone();
            r.indexes = Arc::new(
                cur.indexes
                    .iter()
                    .map(|(name, idx)| (name.clone(), Index::dormant(idx.spec.clone())))
                    .collect(),
            );
            st.building.clear();
            self.forget_all_backlog(&ent);
            r.lagging = false;
            ent.backlog.lagging.store(false, Ordering::SeqCst);
            ent.root.store(Arc::new(r));
        }
    }

    pub fn wake_indexes(&self) {
        if !self.opts.background {
            return;
        }

        for ent in self.all_entities() {
            let specs: Vec<IndexSpec> = ent
                .root
                .load()
                .indexes
                .values()
                .filter(|idx| idx.dormant)
                .map(|idx| idx.spec.clone())
                .collect();
            for spec in specs {
                self.ensure_index(&ent.name, &spec);
            }
        }
    }
}
