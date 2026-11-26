mod common;
mod jsrun;

use gocompat::json;
use jsrun::{new_runtime, on_big_stack};
use std::collections::BTreeSet;
use talos::object::ObjExt;
use talos::{ObjRef, Runtime, Value};

const INPUT: &str = r#"{id: "x", price: 12, ratio: 0.5, tags: ["a", "b"], meta: {k: "v", n: 1}, nul: null, s: "str", b: true, big: 12345678901234567890}"#;

const INPUT_KEYS: [&str; 9] = [
    "b", "big", "id", "meta", "nul", "price", "ratio", "s", "tags",
];

const LABELS: [&str; 12] = [
    "json-parse",
    "json-bad",
    "json-bad-2",
    "json-circular",
    "json-stringify-opts",
    "big-ints-out",
    "math",
    "proxy-reflect",
    "es6",
    "fn-tostring",
    "numbers",
    "num2",
];

fn export(rt: &mut Runtime, v: &Value) -> json::Value {
    match v {
        Value::Undefined | Value::Null => json::Value::Null,
        Value::Bool(b) => json::Value::Bool(*b),
        Value::Int(i) => json::Value::Int(*i),
        Value::Float(f) => json::Value::Number(*f),
        Value::String(s) => json::Value::String(s.to_rust_string()),
        Value::Object(o) => export_object(rt, *o),
        other => panic!("unexpected value {other:?}"),
    }
}

fn export_object(rt: &mut Runtime, o: ObjRef) -> json::Value {
    if let ObjExt::Primitive(p) = &rt.obj(o).ext {
        let p = p.clone();
        return export(rt, &p);
    }
    if o.class_name(rt) == "Array" {
        let len = o
            .get_str(rt, &"length".into(), None)
            .expect("length")
            .unwrap_or(Value::Undefined);
        let len = len.to_integer(rt).expect("length");
        let mut out = Vec::new();
        for i in 0..len {
            let v = o
                .get_idx(rt, i, None)
                .expect("element")
                .unwrap_or(Value::Undefined);
            out.push(export(rt, &v));
        }
        return json::Value::Array(out);
    }
    let mut m = json::Map::new();
    for k in o.string_keys(rt, false, Vec::new()).expect("keys") {
        let name = k.to_string(rt).expect("key");
        let v = o
            .get_str(rt, &name.string(), None)
            .expect("get")
            .unwrap_or(Value::Undefined);
        m.insert(name.to_rust_string(), export(rt, &v));
    }
    json::Value::Object(m)
}

fn encode(v: &json::Value) -> String {
    String::from_utf8(json::marshal(v).expect("marshal")).expect("utf8")
}

fn expected(row: &json::Value) -> (String, String) {
    if let Some(r) = row.get("expected_rust") {
        return (
            common::s(r, "error").to_string(),
            common::s(r, "output").to_string(),
        );
    }
    (
        common::s(row, "error").to_string(),
        common::s(row, "output").to_string(),
    )
}

fn replay(file: &str, arch_file: &str) {
    let rows = common::read_jsonl(file);
    let arch_rows = if cfg!(target_arch = "x86_64") {
        common::read_jsonl(arch_file)
    } else {
        Vec::new()
    };
    for label in LABELS {
        let row = rows
            .iter()
            .find(|r| common::s(r, "label") == label)
            .unwrap_or_else(|| panic!("{file}: no case {label}"));
        let row = arch_rows
            .iter()
            .find(|r| common::s(r, "label") == label)
            .unwrap_or(row);
        let script = common::s(row, "script").to_string();
        let (want_error, output) = expected(row);
        let want = json::unmarshal_value(output.as_bytes()).expect("output");
        let want = want.as_object().expect("an object").clone();
        let label = label.to_string();
        on_big_stack(move || {
            let mut rt = new_runtime();
            let src =
                format!("var ctx = {{entity: {INPUT}}};\n{script}\npostRead(ctx);\nctx.entity");
            let entity = match rt.run_string(&src) {
                Ok(v) => {
                    assert_eq!(want_error, "", "{label}: expected an error");
                    v.as_object().expect("entity")
                }
                Err(e) => {
                    assert_eq!(e.error(), want_error, "{label}: error");
                    let v = rt.get("ctx").expect("ctx").expect("ctx");
                    let ctx = v.as_object().expect("ctx object");
                    match ctx.get_str(&mut rt, &"entity".into(), None) {
                        Ok(Some(Value::Object(o))) => o,
                        other => panic!("{label}: entity {other:?}"),
                    }
                }
            };
            let mut got_keys = BTreeSet::new();
            for k in entity
                .string_keys(&mut rt, false, Vec::new())
                .expect("keys")
            {
                let k = k.to_string(&mut rt).expect("key").to_rust_string();
                if !INPUT_KEYS.contains(&k.as_str()) {
                    got_keys.insert(k);
                }
            }
            let want_keys: BTreeSet<String> = want
                .keys()
                .filter(|k| !INPUT_KEYS.contains(&k.as_str()))
                .cloned()
                .collect();
            assert_eq!(got_keys, want_keys, "{label}: keys");
            for k in &want_keys {
                let v = entity
                    .get_str(&mut rt, &k.as_str().into(), None)
                    .expect("get")
                    .unwrap_or(Value::Undefined);
                let got = encode(&export(&mut rt, &v));
                assert_eq!(got, encode(&want[k]), "{label}: {k}");
            }
        });
    }
}

#[test]
fn corpus_utc_proxy_reflect_math_json_cases() {
    replay("hooks/corpus-utc.jsonl", "hooks/arch-amd64-utc.jsonl");
}

#[test]
fn corpus_europe_paris_proxy_reflect_math_json_cases() {
    replay(
        "hooks/corpus-europe-paris.jsonl",
        "hooks/arch-amd64-europe-paris.jsonl",
    );
}
