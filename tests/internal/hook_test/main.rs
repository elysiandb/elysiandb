#[path = "../tempdir.rs"]
mod tempdir;

mod fold_fuzz_test;
mod fold_test;
mod hook_test;
mod natives_test;
mod prepare_test;
mod query_view_test;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use elysiandb::api;
use elysiandb::configuration::{
    AdminUiConfig, ApiCacheConfig, ApiConfig, ApiIndexConfig, ApiSchemaConfig,
    AuthenticationConfig, Config, CrashRecoveryConfig, HooksConfig, SecurityConfig, StatsConfig,
    StoreConfig,
};
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::gocompat::strconv;
use elysiandb::hook;
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};
use talos::host::{DocMap, DocValue, MapRef};

use tempdir::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());

pub struct Setup {
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for Setup {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

pub fn config(folder: &str, hooks_enabled: bool) -> Config {
    Config {
        store: StoreConfig {
            folder: folder.to_string(),
            shards: 2,
            crash_recovery: CrashRecoveryConfig {
                enabled: false,
                max_log_mb: 1,
            },
            ..StoreConfig::default()
        },
        security: SecurityConfig {
            authentication: AuthenticationConfig {
                enabled: false,
                mode: "basic".to_string(),
                token: String::new(),
            },
        },
        stats: StatsConfig { enabled: false },
        api: ApiConfig {
            schema: ApiSchemaConfig {
                enabled: false,
                strict: false,
            },
            cache: ApiCacheConfig {
                enabled: false,
                cleanup_interval_seconds: 0,
                ..ApiCacheConfig::default()
            },
            index: ApiIndexConfig { workers: 0 },
            hooks: HooksConfig {
                enabled: hooks_enabled,
                ..HooksConfig::default()
            },
            ..ApiConfig::default()
        },
        admin_ui: AdminUiConfig { enabled: false },
        ..Config::default()
    }
}

pub fn setup(hooks_enabled: bool) -> Setup {
    setup_with(|cfg| cfg.api.hooks.enabled = hooks_enabled)
}

pub fn setup_with(f: impl FnOnce(&mut Config)) -> Setup {
    let serial = SERIAL.lock();
    let _ = storage::close();
    let dir = TempDir::new();
    let mut cfg = config(&dir.path().to_string_lossy(), true);
    f(&mut cfg);
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();

    Setup {
        _dir: dir,
        _serial: serial,
    }
}

pub fn value(text: &str) -> Value {
    json::unmarshal_value(text.as_bytes())
        .unwrap_or_else(|e| panic!("invalid test JSON {text}: {e}"))
}

pub fn doc(text: &str) -> Map {
    match &mut value(text) {
        Value::Object(m) => std::mem::take(m),
        _ => panic!("not an object: {text}"),
    }
}

pub fn entity(text: &str) -> MapRef {
    elysiandb::hook::document_from_json(&doc(text))
}

pub fn entity_of(items: Vec<(&str, DocValue)>) -> MapRef {
    let mut m = DocMap::new();
    for (k, v) in items {
        m.insert(k.to_string(), v);
    }
    Rc::new(RefCell::new(m))
}

pub fn field(e: &MapRef, key: &str) -> DocValue {
    e.borrow().get(key).cloned().unwrap_or_default()
}

pub fn text(e: &MapRef, key: &str) -> Option<String> {
    match &field(e, key) {
        DocValue::String(s) => Some(s.to_string()),
        _ => None,
    }
}

pub fn sprint(v: &DocValue) -> String {
    talos::host::json::format_v(v)
}

pub fn fmt_field(e: &MapRef, key: &str) -> String {
    match e.borrow().get(key) {
        Some(v) => sprint(v),
        None => "<nil>".to_string(),
    }
}

pub fn rng(seed: i64) -> Rand {
    Rand::new(rand::new_source(seed))
}

pub fn obj(items: Vec<(&str, Value)>) -> Map {
    items.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

pub fn write(entity: &str, mut d: Map) {
    api::write_entity(entity.as_bytes(), &mut d);
}

pub fn run_post_read(script: &str, e: &MapRef) {
    if let Err(err) = hook::apply_post_read_script("", script, e, true) {
        panic!("hook failed: {err}");
    }
}

pub fn go_float(f: f64) -> String {
    strconv::format_float(f, b'g', -1, 64)
}

pub fn sprint_json(v: Option<&Value>) -> String {
    match v {
        None | Some(Value::Null) => "<nil>".to_string(),
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(f)) => go_float(*f),
        Some(Value::Int(i)) => i.to_string(),
        Some(Value::Bool(b)) => b.to_string(),
        Some(other) => sprint(&talos::host::json::from_json(other)),
    }
}
