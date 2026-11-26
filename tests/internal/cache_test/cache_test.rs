use std::sync::Arc;
use std::time::Duration as StdDuration;

use elysiandb::cache::{self, CacheStore};
use elysiandb::gocompat::time::Duration;

use super::{Filters, filters, serial};

fn store() -> Arc<CacheStore> {
    match cache::cache_store() {
        Some(store) => store,
        None => panic!("the cache is not initialized"),
    }
}

fn users_hash(
    limit: i64,
    offset: i64,
    sort_field: &str,
    asc: bool,
    f: &Filters,
    user: &str,
) -> [u8; 32] {
    cache::hash_query(
        b"users",
        limit,
        offset,
        sort_field.as_bytes(),
        asc,
        f,
        b"",
        b"",
        b"",
        false,
        user.as_bytes(),
    )
}

fn text(v: &Option<Arc<[u8]>>) -> String {
    match v {
        Some(b) => String::from_utf8_lossy(b).into_owned(),
        None => "<nil>".to_string(),
    }
}

#[test]
fn cache_set_get() {
    let _serial = serial();
    cache::init_cache(Duration::SECOND * 10);
    let entity = b"users";
    let hash = users_hash(10, 0, "name", true, &Filters::new(), "alice");
    let value: &[u8] = br#"{"id":"1","name":"Alice"}"#;
    store().set(entity, &hash, value);
    let got = store().get(entity, &hash);
    if got.as_deref() != Some(value) {
        panic!(
            "got {}, want {}",
            text(&got),
            String::from_utf8_lossy(value)
        );
    }
}

#[test]
fn cache_get_not_exist() {
    let _serial = serial();
    cache::init_cache(Duration::SECOND * 10);
    let entity = b"users";
    let hash = users_hash(10, 0, "name", true, &Filters::new(), "alice");
    let got = store().get(entity, &hash);
    if got.is_some() {
        panic!("expected nil, got {}", text(&got));
    }
}

#[test]
fn cache_invalid_hash_length() {
    let _serial = serial();
    cache::init_cache(Duration::SECOND * 10);
    let entity = b"users";
    let got = store().get(entity, b"short");
    if got.is_some() {
        panic!("expected nil for invalid hash length");
    }
    store().set(entity, b"short", &b"x"[..]);
}

#[test]
fn cache_purge() {
    let _serial = serial();
    cache::init_cache(Duration::SECOND * 10);
    let entity = b"users";
    let hash = users_hash(10, 0, "name", true, &Filters::new(), "alice");
    let value: &[u8] = br#"{"id":"1"}"#;
    store().set(entity, &hash, value);
    store().purge(entity);
    let got = store().get(entity, &hash);
    if got.is_some() {
        panic!("expected nil after purge, got {}", text(&got));
    }
}

#[test]
fn hash_query_different_inputs() {
    let none = Filters::new();
    let h1 = users_hash(10, 0, "name", true, &none, "alice");
    let h2 = users_hash(20, 0, "name", true, &none, "alice");
    if h1 == h2 {
        panic!("expected different hashes for different limits");
    }
    let h3 = users_hash(10, 5, "name", true, &none, "alice");
    if h1 == h3 {
        panic!("expected different hashes for different offsets");
    }
    let h4 = users_hash(10, 0, "name", false, &none, "alice");
    if h1 == h4 {
        panic!("expected different hashes for ascending vs descending");
    }
    let h5 = users_hash(10, 0, "age", true, &none, "alice");
    if h1 == h5 {
        panic!("expected different hashes for different sort fields");
    }
}

#[test]
fn hash_query_with_different_users() {
    let none = Filters::new();
    let h1 = users_hash(10, 0, "name", true, &none, "alice");
    let h2 = users_hash(10, 0, "name", true, &none, "bob");
    if h1 == h2 {
        panic!("expected different hashes for different usernames");
    }
}

#[test]
fn hash_query_with_filters_string_ops() {
    let f1 = filters(&[("name", &[("eq", "Alice")])]);
    let f2 = filters(&[("name", &[("neq", "Alice")])]);
    let f3 = filters(&[("name", &[("eq", "Bob")])]);
    let h1 = users_hash(10, 0, "name", true, &f1, "alice");
    let h2 = users_hash(10, 0, "name", true, &f2, "alice");
    let h3 = users_hash(10, 0, "name", true, &f3, "alice");
    if h1 == h2 {
        panic!("expected different hashes for eq vs neq");
    }
    if h1 == h3 {
        panic!("expected different hashes for different eq values");
    }
}

#[test]
fn cache_expiration() {
    let _serial = serial();
    cache::init_cache(Duration::MILLISECOND * 100);
    let entity = b"users";
    let hash = users_hash(10, 0, "name", true, &Filters::new(), "alice");
    let value: &[u8] = br#"{"id":"1","name":"Alice"}"#;
    store().set(entity, &hash, value);
    std::thread::sleep(StdDuration::from_millis(200));
    store().clean_expired();
    let got = store().get(entity, &hash);
    if got.is_some() {
        panic!("expected nil after expiration, got {}", text(&got));
    }
}

#[test]
fn cache_set_get_by_id() {
    let _serial = serial();
    cache::init_cache(Duration::SECOND * 10);
    let entity = b"users";
    let id = b"123";
    let value: &[u8] = br#"{"id":"123","name":"Bob"}"#;
    store().set_by_id(entity, id, value);
    let got = store().get_by_id(entity, id);
    if got.as_deref() != Some(value) {
        panic!(
            "got {}, want {}",
            text(&got),
            String::from_utf8_lossy(value)
        );
    }
}

#[test]
fn cache_expiration_by_id() {
    let _serial = serial();
    cache::init_cache(Duration::MILLISECOND * 100);
    let entity = b"users";
    let id = b"456";
    let value: &[u8] = br#"{"id":"456","name":"Charlie"}"#;
    store().set_by_id(entity, id, value);
    std::thread::sleep(StdDuration::from_millis(200));
    store().clean_expired();
    let got = store().get_by_id(entity, id);
    if got.is_some() {
        panic!("expected nil after expiration, got {}", text(&got));
    }
}
