mod analyzer_test;
mod merge_test;

use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::errors;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::schema::{self, Entity, Field, Fields, LoadSchemaForEntity};
use elysiandb::storage::{self, Hooks};
use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock();
    if globals::config().is_none() {
        globals::set_config(Some(Arc::new(Config::default())));
    }
    guard
}

pub fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn object(m: Map) -> Value {
    Value::Object(m)
}

pub fn fields(list: Vec<(&str, Field)>) -> Fields {
    list.into_iter().map(|(k, f)| (k.to_string(), f)).collect()
}

pub fn sub_map<'a>(m: &'a Map, key: &str) -> &'a Map {
    match m.get(key) {
        Some(Value::Object(sub)) => sub,
        other => panic!("{key} is not an object: {other:?}"),
    }
}

pub struct LoadSchemaGuard(Option<LoadSchemaForEntity>);

impl Drop for LoadSchemaGuard {
    fn drop(&mut self) {
        schema::set_load_schema_for_entity(self.0.take());
    }
}

pub fn patch_load_schema(mock: Option<Entity>) -> LoadSchemaGuard {
    let mock = mock.map(Arc::new);
    let f: LoadSchemaForEntity = Arc::new(move |_entity: &[u8], _data: Option<&Map>| mock.clone());
    LoadSchemaGuard(schema::set_load_schema_for_entity(Some(f)))
}

pub struct HooksGuard(Option<Hooks>);

impl Drop for HooksGuard {
    fn drop(&mut self) {
        storage::set_hooks(self.0.take().unwrap_or_default());
    }
}

pub fn patch_get_json_by_key(m: Option<Map>) -> HooksGuard {
    let orig = storage::hooks();
    let mut patched = orig.clone();
    patched.get_json_by_key = Some(Arc::new(move |_key: &[u8]| match &m {
        Some(m) => Ok(m.clone()),
        None => Err(errors::new("no document")),
    }));
    storage::set_hooks(patched);
    HooksGuard(Some(orig))
}

pub fn patch_manual_flag(val: bool) -> HooksGuard {
    if val {
        return patch_get_json_by_key(Some(obj(vec![("_manual", Value::Bool(true))])));
    }
    patch_get_json_by_key(Some(Map::new()))
}

pub struct StrictGuard(bool);

impl Drop for StrictGuard {
    fn drop(&mut self) {
        if let Some(cfg) = globals::config() {
            let mut cfg = (*cfg).clone();
            cfg.api.schema.strict = self.0;
            globals::set_config(Some(Arc::new(cfg)));
        }
    }
}

pub fn patch_strict(val: bool) -> StrictGuard {
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    let orig = cfg.api.schema.strict;
    cfg.api.schema.strict = val;
    globals::set_config(Some(Arc::new(cfg)));
    StrictGuard(orig)
}

pub fn set_schema_config(strict: bool) {
    let mut cfg = Config::default();
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = strict;
    globals::set_config(Some(Arc::new(cfg)));
}
