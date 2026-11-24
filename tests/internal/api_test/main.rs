#[path = "../tempdir.rs"]
mod tempdir;

mod field_test;
mod filter_test;
mod include_test;
mod relationship_test;
mod search_bytes_test;

use std::sync::{Arc, Once};

use elysiandb::api;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::query::{Filters, Ops};
use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());
static MOCK: Once = Once::new();

pub fn setup() -> MutexGuard<'static, ()> {
    let guard = SERIAL.lock();
    MOCK.call_once(|| {
        api::set_read_entity_by_id_func(Some(Arc::new(include_test::mock_read_entity_by_id)));
    });
    guard
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
