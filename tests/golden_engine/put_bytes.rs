use std::collections::BTreeMap;

use crate::common;
use crate::keys::corpus;
use elysiandb::datastore;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Base64Bytes, Map, Value};

fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    common::str_field(row, key)
}

fn expect_bytes(row: &Value, got: &[u8]) {
    let want = STD_ENCODING
        .decode_string(s(row, "bytes").as_bytes())
        .unwrap_or_else(|e| panic!("{row:?}: {e:?}"));
    assert_eq!(
        String::from_utf8_lossy(got),
        String::from_utf8_lossy(&want),
        "{row:?}"
    );
    assert!(got == want.as_slice(), "{row:?}");
    assert_eq!(
        Some(datastore::decode_doc(got).is_some()),
        row.get("decoded_ok").and_then(Value::as_bool),
        "{row:?}"
    );
}

fn one(key: &str, v: Value) -> Map {
    let mut m = Map::new();
    m.insert(key.to_string(), v);
    m
}

fn go_value(name: &str) -> Result<Vec<u8>, json::Error> {
    match name {
        "int" => json::marshal(&one("v", Value::Int(7))),
        "int64" => json::marshal(&one("v", Value::Int(i64::MAX))),
        "strings" => json::marshal(&one(
            "v",
            Value::Array(vec![Value::String("a".into()), Value::String("<b>".into())]),
        )),
        "negzero" => json::marshal(&one("v", Value::Number(-0.0))),
        "html" => json::marshal(&one("v", Value::String("<script>&amp;</script>".into()))),
        "separators" => json::marshal(&one("v", Value::String("\u{2028}\u{2029}".into()))),
        "invalid" => {
            let mut m: BTreeMap<Vec<u8>, json::RawMessage> = BTreeMap::new();
            m.insert(
                b"v".to_vec(),
                json::RawMessage::new(json::marshal(&b"\xff\xfe".to_vec())?),
            );
            m.insert(
                b"\xffkey".to_vec(),
                json::RawMessage::new(json::marshal(&1.0f64)?),
            );
            json::marshal(&m)
        }
        "nested" => json::marshal(&one(
            "a",
            Value::Object(one(
                "b",
                Value::Array(vec![Value::Object(one("c", Value::Null))]),
            )),
        )),
        "nan" => json::marshal(&one("v", Value::Number(f64::NAN))),
        "inf" => json::marshal(&one("v", Value::Number(f64::INFINITY))),
        "uint8s" => {
            let mut m: BTreeMap<String, Base64Bytes> = BTreeMap::new();
            m.insert("v".into(), Base64Bytes(b"raw".to_vec()));
            json::marshal(&m)
        }
        other => panic!("unknown go value {other}"),
    }
}

#[test]
fn golden_put_bytes() {
    let corpus = corpus();
    let rows = common::read_jsonl("engine/put_bytes.jsonl");
    assert_eq!(rows.len(), 148);

    let mut checked = 0;
    for row in &rows {
        let input = s(row, "in");
        if let Some(n) = input.strip_prefix("corpus:") {
            let i: usize = n.parse().unwrap_or_else(|e| panic!("{e}"));
            match datastore::decode_doc(&corpus[i]) {
                None => {
                    assert_eq!(s(row, "site"), "decodeDoc", "{row:?}");
                }
                Some(doc) => {
                    let b = json::marshal(&*doc).unwrap_or_else(|e| panic!("{row:?}: {e}"));
                    expect_bytes(row, &b);
                }
            }
        } else if let Some(name) = input.strip_prefix("go:") {
            match go_value(name) {
                Err(err) => {
                    assert_eq!(s(row, "site"), "Tx.Put", "{row:?}");
                    let want = if name == "nan" {
                        "cannot encode JSON: NaN is not a valid JSON number"
                    } else {
                        "cannot encode JSON: Infinity is not a valid JSON number"
                    };
                    assert_eq!(err.to_string(), want);
                }
                Ok(b) => expect_bytes(row, &b),
            }
        } else if let Some(h) = input.strip_prefix("raw:") {
            let raw = common::unhex(h);
            match datastore::decode_doc(&raw) {
                None => {
                    assert_eq!(s(row, "site"), "decodeDoc", "{row:?}");
                    assert_eq!(row.get("decoded_ok").and_then(Value::as_bool), Some(false));
                }
                Some(doc) => {
                    let b = json::marshal(&*doc).unwrap_or_else(|e| panic!("{row:?}: {e}"));
                    expect_bytes(row, &b);
                }
            }
        } else {
            panic!("unknown input {input}");
        }
        checked += 1;
    }

    assert_eq!(checked, 148);
}
