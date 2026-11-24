use std::sync::Arc;

use elysiandb::api;
use elysiandb::configuration::{Config, StatsConfig, StoreConfig};
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::time::Time;
use elysiandb::stat;
use elysiandb::storage;

use super::tempdir::TempDir;
use super::{CloseStorage, serial, set_store};

fn put_document(entity: &[u8], id: &str) {
    let mut m = Map::new();
    m.insert("id".to_string(), Value::String(id.to_string()));
    api::write_entity(entity, &mut m);
}

#[test]
fn test_reset_store_deletes_kv_keys_and_keeps_the_documents() {
    let _serial = serial();
    let dir = TempDir::new();
    globals::set_config(Some(Arc::new(Config {
        store: StoreConfig {
            folder: dir.path().to_string_lossy().into_owned(),
            shards: 4,
            ..StoreConfig::default()
        },
        stats: StatsConfig { enabled: true },
        ..Config::default()
    })));
    api::index::register();
    stat::init();
    storage::load_db();
    let _close = CloseStorage;

    put_document(b"books", "b1");
    put_document(b"books", "b2");

    let index_key = globals::api_entity_index_id_key("books");
    let types_key = globals::api_all_entity_types_list_key();
    let ttl_api_key = format!("{}temporary", globals::API_KEY_PREFIX);

    for (key, value) in [
        (index_key.as_str(), "corrupted"),
        (types_key.as_str(), "nothing"),
        ("note", "hello"),
    ] {
        if let Err(err) = storage::put_key_value(key.as_bytes(), value.as_bytes()) {
            panic!("{err}");
        }
    }
    if let Err(err) = storage::put_key_value_with_ttl(b"session", b"x", 3600) {
        panic!("{err}");
    }
    if let Err(err) = storage::put_key_value_with_ttl(ttl_api_key.as_bytes(), b"y", 3600) {
        panic!("{err}");
    }

    match storage::get_by_key(index_key.as_bytes()) {
        Ok(raw) if raw == b"b1\nb2" => {}
        other => {
            panic!("the id index is built from the engine and cannot be overwritten, got {other:?}")
        }
    }

    storage::reset_store();

    for key in ["note", "session", ttl_api_key.as_str()] {
        if storage::get_by_key(key.as_bytes()).is_ok() {
            panic!("{key} should not exist after a reset");
        }
    }

    match storage::get_by_key(index_key.as_bytes()) {
        Ok(raw) if raw == b"b1\nb2" => {}
        other => panic!("the id index must survive a reset, got {other:?}"),
    }
    match storage::get_by_key(types_key.as_bytes()) {
        Ok(raw) if raw == b"books" => {}
        other => panic!("the entity types must survive a reset, got {other:?}"),
    }

    if storage::get_key_expiration(b"session").is_some() {
        panic!("the expiration of a deleted key must be gone");
    }

    if let Err(err) = storage::put_key_value_with_ttl(b"fresh", b"z", 60) {
        panic!("{err}");
    }
    match storage::get_key_expiration(b"fresh") {
        Some(ts) if ts >= Time::now().unix() => {}
        other => panic!("a new key gets its expiration, got {other:?}"),
    }

    let (keys, with_ttl) = storage::count_keys();
    if keys != 1 || with_ttl != 1 {
        panic!("expected 1 key with a TTL, got {keys} {with_ttl}");
    }

    let got = api::count_entities(b"books");
    if got != 2 {
        panic!("documents are not touched by a reset, got {got}");
    }
}

#[test]
fn test_list_json_keys_and_count_json_documents() {
    let _serial = serial();
    let dir = TempDir::new();
    set_store(StoreConfig {
        folder: dir.path().to_string_lossy().into_owned(),
        shards: 4,
        ..StoreConfig::default()
    });
    storage::load_db();
    let _close = CloseStorage;

    for key in ["api:entity:books:id:1", "api:entity:books:id:2"] {
        let doc = Map::from([("id".to_string(), Value::String(key.to_string()))]);
        if let Err(err) = storage::put_json_value(key.as_bytes(), &doc) {
            panic!("{err}");
        }
    }

    let keys = storage::list_json_keys();
    if keys.len() != 2 || storage::count_json_documents() != 2 {
        panic!(
            "unexpected keys {keys:?} and count {}",
            storage::count_json_documents()
        );
    }
}
