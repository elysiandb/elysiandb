mod deep;
mod rules;

use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::sync::MutexGuard;

fn serial() -> MutexGuard<'static, ()> {
    let guard = crate::serial();
    if globals::config().is_none() {
        globals::set_config(Some(Arc::new(Config::default())));
    }
    guard
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn obj(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn object(m: Map) -> Value {
    Value::Object(m)
}

fn sub_map<'a>(m: &'a Map, key: &str) -> &'a Map {
    match m.get(key) {
        Some(Value::Object(sub)) => sub,
        other => panic!("{key} is not an object: {other:?}"),
    }
}

fn set_schema_config(strict: bool) {
    let mut cfg = Config::default();
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = strict;
    globals::set_config(Some(Arc::new(cfg)));
}
