use std::path::PathBuf;

use elysiandb::datastore::{self, CheckpointData};
use elysiandb::gocompat::json::{self, Map, Value};

use super::common;
use super::support::{array, field, string};

pub fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("test/compat/testdata")
        .join(rel)
}

fn obj(pairs: Vec<(&str, Value)>) -> Value {
    let mut m = Map::new();
    for (k, v) in pairs {
        m.insert(k.to_string(), v);
    }
    Value::Object(m)
}

fn hex(b: &[u8]) -> Value {
    Value::String(common::hex(b))
}

fn int(n: impl Into<i64>) -> Value {
    Value::Int(n.into())
}

fn dec(n: impl ToString) -> Value {
    Value::String(n.to_string())
}

fn dump(ck: &CheckpointData) -> Value {
    let ents: Vec<Value> = ck
        .entities
        .iter()
        .map(|e| {
            let refs = e
                .refs
                .iter()
                .map(|r| {
                    obj(vec![
                        ("path_hex", hex(&r.path)),
                        ("target_hex", hex(&r.target)),
                    ])
                })
                .collect();
            let slots = e
                .slots
                .iter()
                .map(|s| {
                    obj(vec![
                        ("seq", int(s.seq)),
                        ("id_hex", hex(&s.id)),
                        ("lsn", dec(s.lsn)),
                        ("segment", int(s.loc.segment)),
                        ("offset", int(s.loc.offset)),
                        ("length", int(s.loc.length)),
                    ])
                })
                .collect();
            let indexes = e
                .indexes
                .iter()
                .map(|ix| {
                    obj(vec![
                        (
                            "name",
                            Value::String(String::from_utf8_lossy(&ix.spec.name()).into_owned()),
                        ),
                        (
                            "keys_hex",
                            Value::Array(ix.keys.iter().map(|k| hex(k)).collect()),
                        ),
                    ])
                })
                .collect();
            obj(vec![
                ("name_hex", hex(&e.name)),
                ("gen", dec(e.gen_)),
                ("next_seq", int(e.next_seq)),
                ("version", dec(e.version)),
                ("refs", Value::Array(refs)),
                ("slots", Value::Array(slots)),
                ("indexes", Value::Array(indexes)),
            ])
        })
        .collect();
    let kv = ck
        .kv
        .iter()
        .map(|k| {
            obj(vec![
                ("key_hex", hex(&k.key)),
                ("lsn", dec(k.lsn)),
                ("expires_at", dec(k.expires_at)),
                ("segment", int(k.loc.segment)),
                ("offset", int(k.loc.offset)),
                ("length", int(k.loc.length)),
            ])
        })
        .collect();
    let extras = match &ck.extras {
        None => Value::Null,
        Some(x) => Value::Array(
            x.iter()
                .map(|(name, e)| {
                    obj(vec![
                        ("entity_hex", hex(name)),
                        (
                            "dormant",
                            Value::Array(
                                e.dormant
                                    .iter()
                                    .map(|d| {
                                        Value::String(
                                            String::from_utf8_lossy(&d.name()).into_owned(),
                                        )
                                    })
                                    .collect(),
                            ),
                        ),
                        ("search", Value::Bool(e.search)),
                    ])
                })
                .collect(),
        ),
    };
    obj(vec![
        ("last_lsn", dec(ck.last_lsn)),
        (
            "position",
            obj(vec![
                ("segment", int(ck.pos.segment)),
                ("offset", int(ck.pos.offset)),
            ]),
        ),
        (
            "types_hex",
            Value::Array(ck.types.iter().map(|t| hex(t)).collect()),
        ),
        ("entities", Value::Array(ents)),
        ("kv", Value::Array(kv)),
        ("extras", extras),
    ])
}

fn same(label: &str, got: &Value, want: &Value) {
    let a = json::marshal(got).unwrap_or_else(|e| panic!("{e}"));
    let b = json::marshal(want).unwrap_or_else(|e| panic!("{e}"));
    assert!(a == b, "{label} differs");
}

#[test]
fn format1_checkpoint_reads_like_go() {
    let golden = common::read_json("persistence/format1/checkpoint.json");
    let ck = datastore::read_checkpoint(&fixture(
        "pr-format1/format1/checkpoint-0000000000000192.ckpt",
    ))
    .unwrap_or_else(|e| panic!("{e}"));
    same("checkpoint.json", &dump(&ck), field(&golden, "dump"));

    let cases = common::read_json("persistence/format1/cases.json");
    let rows = cases.as_array().expect("cases.json is an array");
    assert_eq!(rows.len(), 8);
    let mut checked = 0;
    for row in rows {
        let file = string(row, "file");
        let path = common::golden_path(&format!("persistence/format1/{file}"));
        match datastore::read_checkpoint(&path) {
            Ok(ck) => same(file, &dump(&ck), field(row, "dump")),
            Err(err) => assert_eq!(err.to_string(), string(row, "error"), "{file}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 8);
    assert!(!array(field(&golden, "dump"), "entities").is_empty());
}
