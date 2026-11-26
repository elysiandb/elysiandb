#[path = "../tempdir.rs"]
mod tempdir;

mod backlog_test;
mod evaluate_test;
mod field_test;
mod filter_operators_test;
mod filter_test;
mod include_test;
mod index_advisor_test;
mod index_forms_test;
mod index_test;
mod migration_test;
mod overload_test;
mod query_test;
mod relationship_test;
mod request_memory_test;
mod search_bytes_test;
mod simulation_test;
mod sort_multi_test;
mod sort_test;
mod storage_test;

use std::sync::{Arc, Once};

use elysiandb::api;
use elysiandb::configuration::{Config, StatsConfig, StoreConfig};
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::query::{Filters, Ops};
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};

use tempdir::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

pub fn setup() -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock();
    INIT.call_once(|| {
        unsafe { std::env::set_var(globals::PROCS_ENV, "2") };
        api::set_read_entity_by_id_func(Some(Arc::new(include_test::mock_read_entity_by_id)));
        api::index::register();
    });
    guard
}

pub struct TestStore {
    _dir: TempDir,
}

impl Drop for TestStore {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

pub fn init_test_store() -> TestStore {
    let dir = TempDir::new();
    globals::set_config(Some(Arc::new(Config {
        store: StoreConfig {
            folder: dir.path().to_string_lossy().into_owned(),
            shards: 8,
            ..StoreConfig::default()
        },
        stats: StatsConfig { enabled: false },
        ..Config::default()
    })));
    storage::load_db();
    storage::load_json_db();

    TestStore { _dir: dir }
}

pub fn set_schema(enabled: bool, strict: bool) {
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.api.schema.enabled = enabled;
    cfg.api.schema.strict = strict;
    globals::set_config(Some(Arc::new(cfg)));
}

pub fn write(entity: &str, text: &str) {
    api::write_entity(entity.as_bytes(), &mut doc(text));
}

pub fn value(text: &str) -> Value {
    json::unmarshal_value_with_integers(text.as_bytes())
        .unwrap_or_else(|e| panic!("invalid test JSON {text}: {e}"))
}

pub fn doc(text: &str) -> Map {
    match &mut value(text) {
        Value::Object(m) => std::mem::take(m),
        _ => panic!("not an object: {text}"),
    }
}

pub fn docs(text: &str) -> Vec<Map> {
    match &mut value(text) {
        Value::Array(items) => std::mem::take(items)
            .into_iter()
            .map(|mut v| match &mut v {
                Value::Object(m) => std::mem::take(m),
                _ => panic!("not an object in {text}"),
            })
            .collect(),
        _ => panic!("not an array: {text}"),
    }
}

pub fn doc_list(text: &str) -> Value {
    Value::DocList(docs(text))
}

pub fn ops(spec: &[(&str, &str)]) -> Ops {
    spec.iter()
        .map(|(op, v)| (op.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect()
}

pub type Spec<'a> = &'a [(&'a str, &'a [(&'a str, &'a str)])];

pub fn filters(spec: Spec<'_>) -> Filters {
    spec.iter()
        .map(|(field, o)| (field.as_bytes().to_vec(), ops(o)))
        .collect()
}

pub fn ids(docs: &[Map]) -> Vec<String> {
    docs.iter()
        .map(|d| match d.get("id") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        })
        .collect()
}

pub fn encode(v: &Value) -> String {
    String::from_utf8(json::marshal(v).expect("encode")).expect("utf-8")
}

pub fn encode_map(m: &Map) -> String {
    String::from_utf8(json::marshal(m).expect("encode")).expect("utf-8")
}
