use elysiandb::configuration::StoreConfig;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::storage;

use super::tempdir::TempDir;
use super::{CloseStorage, serial, set_store};

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
