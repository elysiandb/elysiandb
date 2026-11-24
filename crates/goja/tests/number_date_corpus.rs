mod common;
mod jsrun;
mod tzchild;

use gocompat::json;
use gocompat::time::RFC3339_NANO;
use goja::date::DateObject;
use goja::object::ObjExt;
use goja::{ObjRef, Runtime, Value};
use jsrun::{new_runtime, on_big_stack, run_error_string};
use std::collections::BTreeSet;
use tzchild::{Zone, run_in_zone};

const INPUT: &str = r#"{id: "x", price: 12, ratio: 0.5, tags: ["a", "b"], meta: {k: "v", n: 1}, nul: null, s: "str", b: true, big: 12345678901234567890}"#;

const INPUT_KEYS: [&str; 9] = [
    "b", "big", "id", "meta", "nul", "price", "ratio", "s", "tags",
];

const READY: [&str; 3] = ["date", "date-parse", "numfmt"];

const PENDING: [&str; 13] = [
    "num2",
    "numbers",
    "dates2",
    "dates3",
    "date-now-type",
    "dates-ctor-string",
    "date-in-array-out",
    "number-tostring-radix",
    "parse-int-float",
    "date-setters",
    "date-tostring-variants",
    "date-utc-rollover",
    "date-strings",
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
    if let Some(d) = rt.ext_dyn::<DateObject>(o) {
        return match d.export() {
            Some(t) => json::Value::String(t.format(RFC3339_NANO)),
            None => json::Value::Null,
        };
    }
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

fn replay(file: &str, arch_file: &str, labels: &[&str]) {
    let rows = common::read_jsonl(file);
    let arch_rows = if cfg!(target_arch = "x86_64") {
        common::read_jsonl(arch_file)
    } else {
        Vec::new()
    };
    for label in labels {
        let row = rows
            .iter()
            .find(|r| common::s(r, "label") == *label)
            .unwrap_or_else(|| panic!("{file}: no case {label}"));
        let expected = arch_rows
            .iter()
            .find(|r| common::s(r, "label") == *label)
            .unwrap_or(row);
        let script = common::s(row, "script").to_string();
        let want_error = common::s(expected, "error");
        let output = common::s(expected, "output");
        assert!(row.get("expected_rust").is_none(), "{label}");
        assert_eq!(want_error, "", "{label}");
        let want = json::unmarshal_value(output.as_bytes()).expect("output");
        let want = want.as_object().expect("an object").clone();
        let label = label.to_string();
        on_big_stack(move || {
            let mut rt = new_runtime();
            let src =
                format!("var ctx = {{entity: {INPUT}}};\n{script}\npostRead(ctx);\nctx.entity");
            let entity = match rt.run_string(&src) {
                Ok(v) => v.as_object().expect("entity"),
                Err(e) => panic!("{label}: {}", run_error_string(&e)),
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
fn corpus_utc_number_and_date_cases() {
    run_in_zone("corpus_utc_number_and_date_cases", Zone::Env("UTC"), || {
        replay(
            "hooks/corpus-utc.jsonl",
            "hooks/arch-amd64-utc.jsonl",
            &READY,
        )
    });
}

#[test]
fn corpus_europe_paris_number_and_date_cases() {
    run_in_zone(
        "corpus_europe_paris_number_and_date_cases",
        Zone::Env("Europe/Paris"),
        || {
            replay(
                "hooks/corpus-europe-paris.jsonl",
                "hooks/arch-amd64-europe-paris.jsonl",
                &READY,
            )
        },
    );
}

#[test]
#[ignore = "requires the Array, JSON and Math builtins"]
fn corpus_utc_pending_number_and_date_cases() {
    run_in_zone(
        "corpus_utc_pending_number_and_date_cases",
        Zone::Env("UTC"),
        || {
            replay(
                "hooks/corpus-utc.jsonl",
                "hooks/arch-amd64-utc.jsonl",
                &PENDING,
            )
        },
    );
}

#[test]
#[ignore = "requires the Array, JSON and Math builtins"]
fn corpus_europe_paris_pending_number_and_date_cases() {
    run_in_zone(
        "corpus_europe_paris_pending_number_and_date_cases",
        Zone::Env("Europe/Paris"),
        || {
            replay(
                "hooks/corpus-europe-paris.jsonl",
                "hooks/arch-amd64-europe-paris.jsonl",
                &PENDING,
            )
        },
    );
}
