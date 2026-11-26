use std::collections::HashMap;
use std::sync::{Arc, Weak};

use crate::datastore::{Engine, Snapshot, sort_key};
use crate::docpath;
use crate::engine;
use crate::gocompat::json::Value;
use crate::query::{Filters, Ops};
use crate::storage;
use crate::sync::Mutex;

use super::hook::{HOOK_ENTITY, Hook};
use super::prepare::PREPARED;

#[derive(Clone)]
pub(crate) struct RegisteredHook {
    pub(crate) hook: Arc<Hook>,
    pub(crate) sort_key: Vec<u8>,
}

#[derive(Default)]
struct Registry {
    engine: Weak<Engine>,
    generation: u64,
    version: u64,
    loaded: bool,
    hooks: Vec<RegisteredHook>,
    by_entity: HashMap<String, Arc<Vec<Arc<Hook>>>>,
}

static HOOKS: Mutex<Option<Registry>> = Mutex::new(None);

pub(crate) fn same_engine(w: &Weak<Engine>, e: &Arc<Engine>) -> bool {
    std::ptr::eq(w.as_ptr(), Arc::as_ptr(e))
}

fn for_entity(entity: &str) -> Option<Arc<Vec<Arc<Hook>>>> {
    let e = storage::engine()?;

    let snap = e.snapshot(HOOK_ENTITY.as_bytes());

    let mut g = HOOKS.lock();
    let r = g.get_or_insert_with(Registry::default);

    if !r.loaded
        || !same_engine(&r.engine, &e)
        || r.generation != snap.generation()
        || r.version != snap.version()
    {
        load(r, &e, &snap);
    }

    if let Some(list) = r.by_entity.get(entity) {
        return Some(list.clone());
    }

    let list: Vec<Arc<Hook>> = r
        .hooks
        .iter()
        .filter(|h| docpath::match_glob(entity.as_bytes(), h.hook.entity.as_bytes()))
        .map(|h| h.hook.clone())
        .collect();
    let list = Arc::new(list);

    r.by_entity.insert(entity.to_string(), list.clone());

    Some(list)
}

fn load(r: &mut Registry, e: &Arc<Engine>, snap: &Snapshot) {
    let mut list: Vec<RegisteredHook> = Vec::new();
    snap.ascend(0, |d| {
        let Ok(data) = d.decode() else {
            return true;
        };

        let mut h = Hook::default();
        h.from_data_map(&data);
        let doc = Value::Object(data);
        list.push(RegisteredHook {
            hook: Arc::new(h),
            sort_key: sort_key(Some(&doc), b"priority"),
        });

        true
    });

    list.sort_by(|a, b| a.sort_key.cmp(&b.sort_key));

    r.engine = Arc::downgrade(e);
    r.generation = snap.generation();
    r.version = snap.version();
    r.hooks = list;
    r.by_entity = HashMap::new();
    r.loaded = true;

    PREPARED.sync(e, &r.hooks);
}

pub fn prepare_indexes() {
    for_entity(HOOK_ENTITY);
}

pub(crate) fn hooks_for_entity(entity: &str) -> Arc<Vec<Arc<Hook>>> {
    if let Some(list) = for_entity(entity) {
        return list;
    }

    Arc::new(list_hooks_from_engine(entity))
}

fn list_hooks_from_engine(entity: &str) -> Vec<Arc<Hook>> {
    let filters: Filters = Filters::from([(
        b"entity".to_vec(),
        Ops::from([(b"eq".to_vec(), entity.as_bytes().to_vec())]),
    )]);
    let data = engine::list_entities(
        HOOK_ENTITY.as_bytes(),
        0,
        0,
        b"priority",
        true,
        Some(&filters),
        b"",
        b"",
    );

    data.iter()
        .map(|item| {
            let mut h = Hook::default();
            h.from_data_map(item);
            Arc::new(h)
        })
        .collect()
}

pub(crate) fn hooks_for_event(entity: &str, event: &str) -> Vec<Arc<Hook>> {
    hooks_for_entity(entity)
        .iter()
        .filter(|h| h.event == event)
        .cloned()
        .collect()
}
