use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use elysiandb::cache::{self, CacheStore};
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::time::Duration;

use crate::serial;

static VERSION: AtomicU64 = AtomicU64::new(0);

fn small_limits() -> (i64, i64) {
    (1000, 400)
}

fn version(entity: &[u8]) -> String {
    format!(
        "{}:{}",
        String::from_utf8_lossy(entity),
        VERSION.load(Ordering::SeqCst)
    )
}

struct Restore {
    limits: Option<cache::Limits>,
    version_of: Option<cache::VersionOf>,
}

impl Drop for Restore {
    fn drop(&mut self) {
        cache::set_limits(self.limits.take());
        cache::set_version_of(self.version_of.take());
    }
}

fn small_store() -> (Restore, Arc<CacheStore>) {
    let restore = Restore {
        limits: cache::set_limits(Some(small_limits)),
        version_of: cache::set_version_of(None),
    };
    cache::init_cache(Duration::SECOND * 10);
    match cache::cache_store() {
        Some(store) => (restore, store),
        None => panic!("the cache is not initialized"),
    }
}

fn key(b: u8) -> [u8; 32] {
    [b; 32]
}

fn value(n: usize) -> Vec<u8> {
    vec![b'v'; n]
}

#[test]
fn test_eviction_follows_the_sorted_order() {
    let _serial = serial();
    let (_restore, store) = small_store();

    for b in [9u8, 8, 7] {
        store.set(b"b", &key(b), value(100));
    }
    store.set_by_id(b"b", b"z", value(100));
    for b in [6u8, 5, 4, 3] {
        store.set(b"a", &key(b), value(100));
    }
    store.set_by_id(b"a", b"y", value(100));
    store.set_by_id(b"a", b"x", value(100));
    if store.bytes() != 1000 || store.entries() != 10 {
        panic!("unexpected size {} {}", store.bytes(), store.entries());
    }

    store.set(b"c", &key(1), value(100));
    if store.bytes() != 900 || store.entries() != 9 {
        panic!(
            "unexpected size after eviction {} {}",
            store.bytes(),
            store.entries()
        );
    }

    for b in [3u8, 4] {
        if store.get(b"a", &key(b)).is_some() {
            panic!("expected {b} to be evicted first");
        }
    }
    for b in [5u8, 6] {
        if store.get(b"a", &key(b)).is_none() {
            panic!("expected {b} to stay");
        }
    }
    for id in [b"x", b"y"] {
        if store.get_by_id(b"a", id).is_none() {
            panic!("expected the ids to stay");
        }
    }
    if store.get(b"c", &key(1)).is_none() {
        panic!("expected the new entry to be stored");
    }
}

#[test]
fn test_eviction_removes_ids_after_data() {
    let _serial = serial();
    let (_restore, store) = small_store();

    store.set(b"a", &key(1), value(50));
    store.set_by_id(b"a", b"q", value(200));
    store.set_by_id(b"a", b"p", value(200));
    store.set(b"b", &key(2), value(300));
    store.set(b"b", &key(3), value(250));
    if store.bytes() != 1000 {
        panic!("unexpected size {}", store.bytes());
    }

    store.set_by_id(b"c", b"n", value(10));
    if store.bytes() != 760 {
        panic!("unexpected size after eviction {}", store.bytes());
    }
    if store.get(b"a", &key(1)).is_some() || store.get_by_id(b"a", b"p").is_some() {
        panic!("expected the data then the first id to be evicted");
    }
    if store.get_by_id(b"a", b"q").is_none() || store.get(b"b", &key(2)).is_none() {
        panic!("expected the eviction to stop at 90 percent");
    }
}

#[test]
fn test_size_accounting() {
    let _serial = serial();
    let (_restore, store) = small_store();

    if !store.accepts(400) || store.accepts(401) {
        panic!("unexpected entry limit");
    }

    store.set(b"a", &key(1), value(401));
    store.set_by_id(b"a", b"i", value(401));
    if store.bytes() != 0 || store.entries() != 0 {
        panic!("an entry above the limit must not be stored");
    }

    store.set(b"a", &key(1), value(100));
    store.set(b"a", &key(1), value(30));
    store.set_by_id(b"a", b"i", value(20));
    store.set_by_id(b"a", b"i", value(10));
    if store.bytes() != 40 || store.entries() != 2 {
        panic!("unexpected size {} {}", store.bytes(), store.entries());
    }

    store.set(b"b", &key(2), value(5));
    store.purge(b"a");
    if store.bytes() != 5 || store.entries() != 1 {
        panic!(
            "unexpected size after purge {} {}",
            store.bytes(),
            store.entries()
        );
    }

    store.purge_all();
    if store.bytes() != 0 || store.entries() != 0 {
        panic!("unexpected size after purge all");
    }
}

#[test]
fn test_dependencies_make_entries_stale() {
    let _serial = serial();
    let (_restore, store) = small_store();
    cache::set_version_of(Some(version));

    store.set_with_deps(b"a", &key(1), value(10), &[b"a".as_slice(), b"b"]);
    store.set_by_id_with_deps(b"a", b"i", value(10), &[b"a"]);
    if store.get(b"a", &key(1)).is_none() || store.get_by_id(b"a", b"i").is_none() {
        panic!("expected fresh entries");
    }

    VERSION.fetch_add(1, Ordering::SeqCst);
    if store.get(b"a", &key(1)).is_some() || store.get_by_id(b"a", b"i").is_some() {
        panic!("expected stale entries");
    }
    if store.entries() != 2 || store.bytes() != 20 {
        panic!("stale entries stay until they expire");
    }
}

#[test]
fn test_ttl_defaults_to_one_second() {
    let _serial = serial();
    let restore = Restore {
        limits: cache::set_limits(Some(small_limits)),
        version_of: cache::set_version_of(None),
    };
    cache::init_cache(Duration(0));
    let Some(store) = cache::cache_store() else {
        panic!("the cache is not initialized");
    };
    store.set(b"a", &key(1), value(1));
    store.clean_expired();
    if store.get(b"a", &key(1)).is_none() {
        panic!("a zero TTL must become one second");
    }
    drop(restore);
}

#[test]
fn test_default_limits() {
    let _serial = serial();
    let previous = globals::config();

    globals::set_config(None);
    if cache::default_limits() != (128 << 20, 1024 << 10) {
        panic!("unexpected defaults {:?}", cache::default_limits());
    }

    let mut cfg = Config::default();
    cfg.api.cache.max_mb = 2;
    cfg.api.cache.max_entry_kb = -1;
    globals::set_config(Some(Arc::new(cfg)));
    if cache::default_limits() != (2 << 20, 1024 << 10) {
        panic!("unexpected limits {:?}", cache::default_limits());
    }

    globals::set_config(previous);
}
