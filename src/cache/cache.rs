use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use crate::crypto::sha256;
use crate::globals;
use crate::gocompat::strconv;
use crate::gocompat::time::{Duration, Time};
use crate::kv;
use crate::log::{self, Logger};
use crate::metrics;
use crate::sync::RwLock;

const CACHE_LOG: Logger = log::for_component("cache");

pub const HASH_SIZE: usize = sha256::SIZE;

struct Dependency {
    entity: Vec<u8>,
    version: String,
}

#[derive(Clone)]
struct CacheItem {
    v: Arc<[u8]>,
    exp: i64,
    deps: Option<Arc<[Dependency]>>,
}

pub type VersionOf = fn(&[u8]) -> String;

static VERSION_OF: RwLock<Option<VersionOf>> = RwLock::new(None);

pub fn version_of() -> Option<VersionOf> {
    *VERSION_OF.read()
}

pub fn set_version_of(f: Option<VersionOf>) -> Option<VersionOf> {
    std::mem::replace(&mut *VERSION_OF.write(), f)
}

fn capture_deps<E: AsRef<[u8]>>(entities: &[E]) -> Option<Arc<[Dependency]>> {
    let version_of = version_of()?;
    if entities.is_empty() {
        return None;
    }

    let deps: Vec<Dependency> = entities
        .iter()
        .map(|entity| Dependency {
            entity: entity.as_ref().to_vec(),
            version: version_of(entity.as_ref()),
        })
        .collect();

    Some(deps.into())
}

impl CacheItem {
    fn fresh(&self, now: i64) -> bool {
        if now > self.exp {
            return false;
        }

        let Some(deps) = &self.deps else {
            return true;
        };

        let Some(version_of) = version_of() else {
            return false;
        };

        deps.iter().all(|d| version_of(&d.entity) == d.version)
    }
}

#[derive(Default)]
struct CacheMaps {
    data: BTreeMap<[u8; HASH_SIZE], CacheItem>,
    ids: BTreeMap<Vec<u8>, CacheItem>,
}

#[derive(Default)]
struct CacheEntity {
    maps: RwLock<CacheMaps>,
}

impl CacheEntity {
    fn size(&self) -> i64 {
        let maps = self.maps.read();

        let mut total = 0i64;
        for it in maps.data.values() {
            total += len_i64(&it.v);
        }

        for it in maps.ids.values() {
            total += len_i64(&it.v);
        }

        total
    }
}

pub struct CacheStore {
    entities: RwLock<BTreeMap<Vec<u8>, Arc<CacheEntity>>>,
    ttl: Duration,
    max_bytes: i64,
    max_entry: i64,
    bytes: AtomicI64,
}

pub const DEFAULT_MAX_MB: i64 = 128;
pub const DEFAULT_MAX_ENTRY_KB: i64 = 1024;

pub type Limits = fn() -> (i64, i64);

static LIMITS: RwLock<Option<Limits>> = RwLock::new(None);

pub fn set_limits(f: Option<Limits>) -> Option<Limits> {
    std::mem::replace(&mut *LIMITS.write(), f)
}

pub fn limits() -> (i64, i64) {
    let hook = *LIMITS.read();
    match hook {
        Some(f) => f(),
        None => default_limits(),
    }
}

pub fn default_limits() -> (i64, i64) {
    let (mut max_mb, mut max_entry_kb) = (DEFAULT_MAX_MB, DEFAULT_MAX_ENTRY_KB);
    if let Some(cfg) = globals::config() {
        if cfg.api.cache.max_mb > 0 {
            max_mb = cfg.api.cache.max_mb;
        }

        if cfg.api.cache.max_entry_kb > 0 {
            max_entry_kb = cfg.api.cache.max_entry_kb;
        }
    }

    (max_mb.wrapping_shl(20), max_entry_kb.wrapping_shl(10))
}

static CACHE_STORE: RwLock<Option<Arc<CacheStore>>> = RwLock::new(None);

pub fn cache_store() -> Option<Arc<CacheStore>> {
    CACHE_STORE.read().clone()
}

pub fn init_cache(ttl: Duration) {
    let ttl = if ttl.0 <= 0 { Duration::SECOND } else { ttl };

    let (max_bytes, max_entry) = limits();
    let store = Arc::new(CacheStore {
        entities: RwLock::new(BTreeMap::new()),
        ttl,
        max_bytes,
        max_entry,
        bytes: AtomicI64::new(0),
    });

    let previous = CACHE_STORE.write().replace(store);
    drop(previous);

    CACHE_LOG.debug("api cache initialized", kv!["ttl" => ttl]);
}

fn len_i64(v: &[u8]) -> i64 {
    i64::try_from(v.len()).unwrap_or(i64::MAX)
}

fn size_i64(size: usize) -> i64 {
    i64::try_from(size).unwrap_or(i64::MAX)
}

fn now_unix_nano() -> i64 {
    Time::now().unix_nano()
}

fn hash_key(hash: &[u8]) -> Option<[u8; HASH_SIZE]> {
    <[u8; HASH_SIZE]>::try_from(hash).ok()
}

impl CacheStore {
    pub fn entries(&self) -> usize {
        let entities: Vec<Arc<CacheEntity>> = self.entities.read().values().cloned().collect();

        let mut total = 0;
        for e in entities {
            let maps = e.maps.read();
            total += maps.data.len() + maps.ids.len();
        }

        total
    }

    fn entity(&self, entity: &[u8]) -> Option<Arc<CacheEntity>> {
        self.entities.read().get(entity).cloned()
    }

    fn entity_or_create(&self, entity: &[u8]) -> Arc<CacheEntity> {
        let mut entities = self.entities.write();
        if let Some(e) = entities.get(entity) {
            return e.clone();
        }

        let e = Arc::new(CacheEntity::default());
        entities.insert(entity.to_vec(), e.clone());
        e
    }

    pub fn get(&self, entity: &[u8], hash: &[u8]) -> Option<Arc<[u8]>> {
        let key = hash_key(hash)?;

        let Some(e) = self.entity(entity) else {
            metrics::cache_miss();
            return None;
        };

        let it = e.maps.read().data.get(&key).cloned();

        match it {
            Some(it) if it.fresh(now_unix_nano()) => {
                metrics::cache_hit();
                Some(it.v)
            }
            _ => {
                metrics::cache_miss();
                None
            }
        }
    }

    pub fn accepts(&self, size: usize) -> bool {
        size_i64(size) <= self.max_entry
    }

    fn admit(&self, size: usize) -> bool {
        let size = size_i64(size);
        if size > self.max_entry {
            return false;
        }

        if self
            .bytes
            .fetch_add(size, Ordering::SeqCst)
            .wrapping_add(size)
            > self.max_bytes
        {
            self.evict();
        }

        true
    }

    fn evict(&self) {
        let target = self.max_bytes.wrapping_mul(9) / 10;

        let entities: Vec<Arc<CacheEntity>> = self.entities.read().values().cloned().collect();

        for e in entities {
            if self.bytes.load(Ordering::SeqCst) <= target {
                return;
            }

            let mut maps = e.maps.write();
            while let Some((_, it)) = maps.data.pop_first() {
                self.bytes.fetch_sub(len_i64(&it.v), Ordering::SeqCst);
                if self.bytes.load(Ordering::SeqCst) <= target {
                    break;
                }
            }

            loop {
                if self.bytes.load(Ordering::SeqCst) <= target {
                    break;
                }
                let Some((_, it)) = maps.ids.pop_first() else {
                    break;
                };
                self.bytes.fetch_sub(len_i64(&it.v), Ordering::SeqCst);
            }
        }
    }

    pub fn bytes(&self) -> i64 {
        self.bytes.load(Ordering::SeqCst)
    }

    pub fn set<V>(&self, entity: &[u8], hash: &[u8], value: V)
    where
        V: AsRef<[u8]> + Into<Arc<[u8]>>,
    {
        self.set_with_deps::<V, &[u8]>(entity, hash, value, &[]);
    }

    pub fn set_with_deps<V, E>(&self, entity: &[u8], hash: &[u8], value: V, entities: &[E])
    where
        V: AsRef<[u8]> + Into<Arc<[u8]>>,
        E: AsRef<[u8]>,
    {
        let Some(key) = hash_key(hash) else {
            return;
        };

        let e = self.entity_or_create(entity);

        if !self.admit(value.as_ref().len()) {
            return;
        }

        let exp = Time::now().add(self.ttl).unix_nano();
        let deps = capture_deps(entities);
        let item = CacheItem {
            v: value.into(),
            exp,
            deps,
        };

        let mut maps = e.maps.write();
        if let Some(old) = maps.data.get(&key) {
            self.bytes.fetch_sub(len_i64(&old.v), Ordering::SeqCst);
        }
        let previous = maps.data.insert(key, item);
        drop(maps);
        drop(previous);
    }

    pub fn get_by_id(&self, entity: &[u8], id: &[u8]) -> Option<Arc<[u8]>> {
        let e = self.entity(entity)?;

        let it = e.maps.read().ids.get(id).cloned()?;

        if !it.fresh(now_unix_nano()) {
            return None;
        }

        Some(it.v)
    }

    pub fn set_by_id<V>(&self, entity: &[u8], id: &[u8], value: V)
    where
        V: AsRef<[u8]> + Into<Arc<[u8]>>,
    {
        self.set_by_id_with_deps::<V, &[u8]>(entity, id, value, &[]);
    }

    pub fn set_by_id_with_deps<V, E>(&self, entity: &[u8], id: &[u8], value: V, entities: &[E])
    where
        V: AsRef<[u8]> + Into<Arc<[u8]>>,
        E: AsRef<[u8]>,
    {
        let e = self.entity_or_create(entity);

        if !self.admit(value.as_ref().len()) {
            return;
        }

        let exp = Time::now().add(self.ttl).unix_nano();
        let deps = capture_deps(entities);
        let item = CacheItem {
            v: value.into(),
            exp,
            deps,
        };

        let mut maps = e.maps.write();
        if let Some(old) = maps.ids.get(id) {
            self.bytes.fetch_sub(len_i64(&old.v), Ordering::SeqCst);
        }
        let previous = maps.ids.insert(id.to_vec(), item);
        drop(maps);
        drop(previous);
    }

    pub fn purge(&self, entity: &[u8]) {
        let e = self.entities.write().remove(entity);

        if let Some(e) = e {
            self.bytes.fetch_sub(e.size(), Ordering::SeqCst);
        }
    }

    pub fn purge_all(&self) {
        let mut entities = self.entities.write();
        let previous = std::mem::take(&mut *entities);
        self.bytes.store(0, Ordering::SeqCst);
        drop(entities);
        drop(previous);
    }

    pub fn clean_expired(&self) {
        let entities: Vec<Arc<CacheEntity>> = self.entities.read().values().cloned().collect();

        let now = now_unix_nano();

        for e in entities {
            let mut maps = e.maps.write();
            maps.data.retain(|_, it| {
                if now > it.exp {
                    self.bytes.fetch_sub(len_i64(&it.v), Ordering::SeqCst);
                    return false;
                }
                true
            });

            maps.ids.retain(|_, it| {
                if now > it.exp {
                    self.bytes.fetch_sub(len_i64(&it.v), Ordering::SeqCst);
                    return false;
                }
                true
            });
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub fn hash_query<F, O, V>(
    entity: &[u8],
    limit: i64,
    offset: i64,
    sort_field: &[u8],
    sort_ascending: bool,
    filters: &BTreeMap<F, BTreeMap<O, V>>,
    fields_param: &[u8],
    search: &[u8],
    includes_param: &[u8],
    count_only_param: bool,
    username: &[u8],
) -> [u8; HASH_SIZE]
where
    F: AsRef<[u8]>,
    O: AsRef<[u8]>,
    V: AsRef<[u8]>,
{
    let mut b: Vec<u8> = Vec::new();
    b.extend_from_slice(entity);
    b.push(b'|');
    b.extend_from_slice(sort_field);
    b.push(b'|');
    b.extend_from_slice(fields_param);
    b.push(b'|');
    b.extend_from_slice(search);
    b.push(b'|');
    b.extend_from_slice(includes_param);
    b.push(b'|');
    b.extend_from_slice(username);
    b.push(b'|');

    if count_only_param {
        b.push(b'1');
    } else {
        b.push(b'0');
    }

    b.push(b'|');

    if sort_ascending {
        b.push(b'A');
    } else {
        b.push(b'D');
    }

    b.push(b'|');
    strconv::append_int(&mut b, limit, 10);
    b.push(b'|');
    strconv::append_int(&mut b, offset, 10);

    let mut keys: Vec<(&[u8], &BTreeMap<O, V>)> =
        filters.iter().map(|(k, ops)| (k.as_ref(), ops)).collect();
    keys.sort_by(|a, b| a.0.cmp(b.0));

    for (k, ops) in keys {
        let mut ops_keys: Vec<(&[u8], &[u8])> = ops
            .iter()
            .map(|(op, value)| (op.as_ref(), value.as_ref()))
            .collect();
        ops_keys.sort_by(|a, b| a.0.cmp(b.0));

        for (op, value) in ops_keys {
            b.push(b'|');
            b.extend_from_slice(k);
            b.push(b'[');
            b.extend_from_slice(op);
            b.extend_from_slice(b"]=");
            b.extend_from_slice(value);
        }
    }

    sha256::sum256(&b)
}

pub fn clean_expired_entries() {
    let store = cache_store();

    if let Some(store) = store {
        store.clean_expired();
    }
}
