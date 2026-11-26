mod common;

use gocompat::json;
use std::collections::{BTreeMap, BTreeSet};
use talos::builtin_symbol::{
    SYM_HAS_INSTANCE, SYM_IS_CONCAT_SPREADABLE, SYM_ITERATOR, SYM_MATCH, SYM_MATCH_ALL,
    SYM_REPLACE, SYM_SEARCH, SYM_SPECIES, SYM_SPLIT, SYM_TO_PRIMITIVE, SYM_TO_STRING_TAG,
    SYM_UNSCOPABLES,
};
use talos::{ObjRef, Runtime, SymRef, Value};

const PREFIXES: [&str; 7] = [
    "globalThis.Map",
    "globalThis.Set",
    "globalThis.WeakMap",
    "globalThis.WeakSet",
    "globalThis.Promise",
    "%MapIteratorPrototype%",
    "%SetIteratorPrototype%",
];

fn on_big_stack<F: FnOnce() + Send + 'static>(f: F) {
    let h = std::thread::Builder::new()
        .name("talos-test".to_string())
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(e) = h.join() {
        std::panic::resume_unwind(e);
    }
}

fn in_scope(path: &str) -> bool {
    PREFIXES
        .iter()
        .any(|p| path == *p || path.starts_with(&format!("{p}.")))
}

fn tokens(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut depth = 0;
    for c in path.chars() {
        match c {
            '[' => {
                depth += 1;
                cur.push(c);
            }
            ']' => {
                depth -= 1;
                cur.push(c);
            }
            '.' if depth == 0 => out.push(std::mem::take(&mut cur)),
            _ => cur.push(c),
        }
    }
    out.push(cur);
    out
}

fn well_known(name: &str) -> SymRef {
    match name {
        "[Symbol.hasInstance]" => SYM_HAS_INSTANCE,
        "[Symbol.isConcatSpreadable]" => SYM_IS_CONCAT_SPREADABLE,
        "[Symbol.iterator]" => SYM_ITERATOR,
        "[Symbol.match]" => SYM_MATCH,
        "[Symbol.matchAll]" => SYM_MATCH_ALL,
        "[Symbol.replace]" => SYM_REPLACE,
        "[Symbol.search]" => SYM_SEARCH,
        "[Symbol.species]" => SYM_SPECIES,
        "[Symbol.split]" => SYM_SPLIT,
        "[Symbol.toPrimitive]" => SYM_TO_PRIMITIVE,
        "[Symbol.toStringTag]" => SYM_TO_STRING_TAG,
        "[Symbol.unscopables]" => SYM_UNSCOPABLES,
        other => panic!("unknown symbol key {other}"),
    }
}

fn own_prop(rt: &mut Runtime, o: ObjRef, key: &str) -> Option<Value> {
    if key.starts_with("[Symbol.") {
        return o.get_own_prop_sym(rt, well_known(key)).expect("own prop");
    }
    o.get_own_prop_str(rt, &key.into()).expect("own prop")
}

enum Node {
    Obj(ObjRef),
    Accessor(Option<ObjRef>, Option<ObjRef>),
}

fn resolve(rt: &mut Runtime, path: &str) -> ObjRef {
    let toks = tokens(path);
    let mut node = Node::Obj(match toks[0].as_str() {
        "globalThis" => rt.global_object(),
        "%MapIteratorPrototype%" => rt.get_map_iterator_prototype(),
        "%SetIteratorPrototype%" => rt.get_set_iterator_prototype(),
        "%IteratorPrototype%" => rt.get_iterator_prototype(),
        other => panic!("unknown root {other}"),
    });
    for t in &toks[1..] {
        node = match node {
            Node::Accessor(g, s) => match t.as_str() {
                "get" => Node::Obj(g.expect("getter")),
                "set" => Node::Obj(s.expect("setter")),
                other => panic!("bad accessor token {other} in {path}"),
            },
            Node::Obj(o) => {
                if t == "[[Prototype]]" {
                    Node::Obj(o.proto(rt).expect("proto").expect("non-null proto"))
                } else {
                    match own_prop(rt, o, t) {
                        Some(Value::Object(v)) => Node::Obj(v),
                        Some(Value::Property(p)) if p.accessor => {
                            Node::Accessor(p.getter_func, p.setter_func)
                        }
                        Some(Value::Property(p)) => match &p.value {
                            Value::Object(v) => Node::Obj(*v),
                            other => panic!("{path}: {t} is not an object: {other:?}"),
                        },
                        other => panic!("{path}: {t} is {other:?}"),
                    }
                }
            }
        };
    }
    match node {
        Node::Obj(o) => o,
        Node::Accessor(..) => panic!("{path} is an accessor"),
    }
}

fn type_name(rt: &mut Runtime, v: &Value) -> String {
    match v {
        Value::Undefined => "undefined".into(),
        Value::Null => "object".into(),
        Value::Bool(_) => "boolean".into(),
        Value::Int(_) | Value::Float(_) => "number".into(),
        Value::String(_) => "string".into(),
        Value::Symbol(_) => "symbol".into(),
        Value::Object(o) => o.type_of(rt).to_rust_string(),
        other => panic!("unexpected {other:?}"),
    }
}

fn get_str(rt: &mut Runtime, o: ObjRef, k: &str) -> Value {
    o.get_str(rt, &k.into(), None)
        .expect("get")
        .unwrap_or(Value::Undefined)
}

fn check_key_record(rt: &mut Runtime, rec: &json::Value) {
    let path = rec.get("path").and_then(json::Value::as_str).expect("path");
    let key = rec.get("key").and_then(json::Value::as_str).expect("key");
    let o = resolve(rt, path);
    let prop = own_prop(rt, o, key).unwrap_or_else(|| panic!("{path}.{key} missing"));
    let (enumerable, configurable, writable, accessor, value, getter, setter) = match &prop {
        Value::Property(p) => (
            p.enumerable,
            p.configurable,
            p.writable,
            p.accessor,
            p.value.clone(),
            p.getter_func,
            p.setter_func,
        ),
        v => (true, true, true, false, v.clone(), None, None),
    };
    let ctx = format!("{path} {key}");
    let b = |k: &str| rec.get(k).and_then(json::Value::as_bool);
    assert_eq!(b("enumerable"), Some(enumerable), "{ctx} enumerable");
    assert_eq!(b("configurable"), Some(configurable), "{ctx} configurable");
    let kind = rec.get("kind").and_then(json::Value::as_str).expect("kind");
    if kind == "accessor" {
        assert!(accessor, "{ctx} accessor");
        let g = rec.get("get").and_then(json::Value::as_str).expect("get");
        let s = rec.get("set").and_then(json::Value::as_str).expect("set");
        let gt = match getter {
            Some(g) => type_name(rt, &Value::Object(g)),
            None => "undefined".into(),
        };
        let st = match setter {
            Some(s) => type_name(rt, &Value::Object(s)),
            None => "undefined".into(),
        };
        assert_eq!(g, gt, "{ctx} get");
        assert_eq!(s, st, "{ctx} set");
        return;
    }
    assert!(!accessor, "{ctx} data");
    assert_eq!(b("writable"), Some(writable), "{ctx} writable");
    let t = rec.get("type").and_then(json::Value::as_str).expect("type");
    assert_eq!(t, type_name(rt, &value), "{ctx} type");
    if t == "function" {
        let f = value.as_object().expect("function");
        let len = get_str(rt, f, "length");
        let name = get_str(rt, f, "name").to_string(rt).expect("name");
        let want_len = rec
            .get("length")
            .and_then(json::Value::as_i64)
            .expect("length");
        let want_name = rec.get("name").and_then(json::Value::as_str).expect("name");
        assert!(
            matches!(len, Value::Int(l) if l == want_len),
            "{ctx} length {len:?}"
        );
        assert_eq!(name.to_rust_string(), want_name, "{ctx} name");
    }
    if let Some(want) = rec.get("value").and_then(json::Value::as_str) {
        let got = value.to_string(rt).expect("value").to_rust_string();
        assert_eq!(got, want, "{ctx} value");
    }
}

fn own_keys(rt: &mut Runtime, o: ObjRef) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for k in o.keys(rt, true, Vec::new()).expect("keys") {
        match k {
            Value::Symbol(s) => {
                let d = rt.symbol_desc(s).expect("desc").to_rust_string();
                out.insert(format!("[{d}]"));
            }
            v => {
                out.insert(v.to_string(rt).expect("key").to_rust_string());
            }
        }
    }
    out
}

#[test]
fn collections_and_promise_surface_matches_golden() {
    on_big_stack(|| {
        let data = common::read_golden("hooks/surface.json");
        let records = json::unmarshal_value(&data).expect("surface.json");
        let records = records.as_array().expect("array").clone();
        let mut rt = Runtime::new();
        let mut keys_by_path: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut checked = 0;
        for rec in &records {
            let path = rec.get("path").and_then(json::Value::as_str).expect("path");
            if !in_scope(path) {
                continue;
            }
            checked += 1;
            if let Some(key) = rec.get("key").and_then(json::Value::as_str) {
                keys_by_path
                    .entry(path.to_string())
                    .or_default()
                    .insert(key.to_string());
                check_key_record(&mut rt, rec);
            } else if let Some(p) = rec.get("prototype").and_then(json::Value::as_str) {
                let o = resolve(&mut rt, path);
                let proto = o.proto(&mut rt).expect("proto").expect("proto");
                assert_eq!(proto, resolve(&mut rt, p), "{path} prototype");
            } else if let Some(a) = rec.get("alias").and_then(json::Value::as_str) {
                assert_eq!(resolve(&mut rt, path), resolve(&mut rt, a), "{path} alias");
            }
        }
        assert_eq!(checked, 206);
        for (path, keys) in keys_by_path {
            let o = resolve(&mut rt, &path);
            assert_eq!(own_keys(&mut rt, o), keys, "{path} own keys");
        }
    });
}

#[test]
fn collections_and_promise_own_key_order() {
    on_big_stack(|| {
        let mut rt = Runtime::new();
        let expected: [(&str, &[&str]); 12] = [
            (
                "globalThis.Map",
                &["length", "name", "prototype", "[Symbol.species]"],
            ),
            (
                "globalThis.Map.prototype",
                &[
                    "constructor",
                    "clear",
                    "set",
                    "delete",
                    "forEach",
                    "has",
                    "get",
                    "size",
                    "keys",
                    "values",
                    "entries",
                    "[Symbol.iterator]",
                    "[Symbol.toStringTag]",
                ],
            ),
            (
                "globalThis.Set",
                &["length", "name", "prototype", "[Symbol.species]"],
            ),
            (
                "globalThis.Set.prototype",
                &[
                    "constructor",
                    "add",
                    "clear",
                    "delete",
                    "forEach",
                    "has",
                    "size",
                    "values",
                    "keys",
                    "entries",
                    "[Symbol.iterator]",
                    "[Symbol.toStringTag]",
                ],
            ),
            ("globalThis.WeakMap", &["length", "name", "prototype"]),
            (
                "globalThis.WeakMap.prototype",
                &[
                    "constructor",
                    "set",
                    "delete",
                    "has",
                    "get",
                    "[Symbol.toStringTag]",
                ],
            ),
            ("globalThis.WeakSet", &["length", "name", "prototype"]),
            (
                "globalThis.WeakSet.prototype",
                &[
                    "constructor",
                    "add",
                    "delete",
                    "has",
                    "[Symbol.toStringTag]",
                ],
            ),
            (
                "globalThis.Promise",
                &[
                    "length",
                    "name",
                    "prototype",
                    "all",
                    "allSettled",
                    "any",
                    "race",
                    "reject",
                    "resolve",
                    "[Symbol.species]",
                ],
            ),
            (
                "globalThis.Promise.prototype",
                &[
                    "constructor",
                    "catch",
                    "finally",
                    "then",
                    "[Symbol.toStringTag]",
                ],
            ),
            ("%MapIteratorPrototype%", &["next", "[Symbol.toStringTag]"]),
            ("%SetIteratorPrototype%", &["next", "[Symbol.toStringTag]"]),
        ];
        for (path, want) in expected {
            let o = resolve(&mut rt, path);
            let mut got = Vec::new();
            for k in o.keys(&mut rt, true, Vec::new()).expect("keys") {
                match k {
                    Value::Symbol(s) => {
                        let d = rt.symbol_desc(s).expect("desc").to_rust_string();
                        got.push(format!("[{d}]"));
                    }
                    v => got.push(v.to_string(&mut rt).expect("key").to_rust_string()),
                }
            }
            assert_eq!(got, want, "{path} key order");
        }
    });
}
