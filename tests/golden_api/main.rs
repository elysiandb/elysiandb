#[path = "../common/mod.rs"]
mod common;
#[path = "../internal/tempdir.rs"]
mod tempdir;

mod cache_keys;
mod include;
mod matcher;
mod planner;
mod query_errors;
mod scalars;
mod schema_corpus;
mod sort;
mod write;

use std::sync::{Arc, Once};

use elysiandb::api;
use elysiandb::cache;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::Duration;
use elysiandb::query::Filters;
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};

use tempdir::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());
static INIT: Once = Once::new();

pub struct GoldenStore {
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for GoldenStore {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

pub fn open_store(schema_on: bool, strict: bool, cache_on: bool) -> GoldenStore {
    let serial = SERIAL.lock();
    INIT.call_once(api::index::register);
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.api.cache.enabled = cache_on;
    cfg.api.schema.enabled = schema_on;
    cfg.api.schema.strict = strict;
    cfg.api.hooks.enabled = false;
    cfg.security.authentication.enabled = false;
    cfg.log.level = "error".to_string();
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();
    cache::init_cache(Duration(30 * Duration::SECOND.0));
    api::delete_all();

    GoldenStore {
        _dir: dir,
        _serial: serial,
    }
}

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

pub fn array<'a>(row: &'a Value, key: &str) -> &'a [Value] {
    match row.get(key) {
        Some(Value::Array(a)) => a,
        other => panic!("missing array {key}: {other:?}"),
    }
}

pub fn boolean(v: &Value) -> bool {
    match v {
        Value::Bool(b) => *b,
        other => panic!("not a boolean: {other:?}"),
    }
}

pub fn text(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v:?}"))
}

pub fn integer(row: &Value, key: &str) -> i64 {
    match row.get(key) {
        Some(Value::Int(n)) => *n,
        other => panic!("missing integer {key}: {other:?}"),
    }
}

pub fn go_decode(raw: &[u8]) -> Value {
    json::unmarshal_value(raw).unwrap_or_else(|e| panic!("{}: {e}", String::from_utf8_lossy(raw)))
}

pub fn go_map(raw: &[u8]) -> Map {
    match &mut go_decode(raw) {
        Value::Object(m) => std::mem::take(m),
        other => panic!("not an object: {other:?}"),
    }
}

pub fn go_maps(raw: &[u8]) -> Vec<Map> {
    match &mut go_decode(raw) {
        Value::Array(items) => std::mem::take(items)
            .into_iter()
            .map(|mut v| match &mut v {
                Value::Object(m) => std::mem::take(m),
                other => panic!("not an object: {other:?}"),
            })
            .collect(),
        other => panic!("not an array: {other:?}"),
    }
}

pub fn go_filters(v: &Value) -> Filters {
    let Value::Object(fields) = v else {
        panic!("filters must be an object: {v:?}");
    };
    fields
        .iter()
        .map(|(field, ops)| {
            let Value::Object(ops) = ops else {
                panic!("operators must be an object: {ops:?}");
            };
            let ops = ops
                .iter()
                .map(|(op, cmp)| (op.as_bytes().to_vec(), text(cmp).as_bytes().to_vec()))
                .collect();
            (field.as_bytes().to_vec(), ops)
        })
        .collect()
}
