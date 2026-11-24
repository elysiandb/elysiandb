use std::collections::BTreeMap;

use elysiandb::api::{self, BatchOperation, MigrationError};
use elysiandb::gocompat::json::{self, Map, Value};

use super::common::{read_jsonl, str_field};
use super::{go_map, integer, open_store, text};

pub struct UuidMasker {
    seen: BTreeMap<Vec<u8>, String>,
}

fn is_hex(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c)
}

fn uuid_at(b: &[u8]) -> bool {
    if b.len() < 36 {
        return false;
    }
    for (i, &c) in b[..36].iter().enumerate() {
        let ok = match i {
            8 | 13 | 18 | 23 => c == b'-',
            14 => c == b'4',
            19 => matches!(c, b'8' | b'9' | b'a' | b'b'),
            _ => is_hex(c),
        };
        if !ok {
            return false;
        }
    }
    true
}

impl UuidMasker {
    pub fn new() -> UuidMasker {
        UuidMasker {
            seen: BTreeMap::new(),
        }
    }

    pub fn mask(&mut self, b: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(b.len());
        let mut i = 0;
        while i < b.len() {
            if uuid_at(&b[i..]) {
                let key = b[i..i + 36].to_vec();
                let next = self.seen.len() + 1;
                let name = self
                    .seen
                    .entry(key)
                    .or_insert_with(|| format!("<uuid#{next}>"));
                out.extend_from_slice(name.as_bytes());
                i += 36;
                continue;
            }
            out.push(b[i]);
            i += 1;
        }
        out
    }

    pub fn text(&mut self, b: &[u8]) -> String {
        String::from_utf8(self.mask(b)).expect("utf-8")
    }
}

fn marshal<T: json::Encode + ?Sized>(v: &T) -> Vec<u8> {
    json::marshal(v).expect("encode")
}

pub fn stored_entity(entity: &str, m: &mut UuidMasker) -> Vec<(String, String)> {
    let mut out = Vec::new();
    for d in api::list_entities(entity.as_bytes(), 0, 0, b"", true, None, b"", b"") {
        let id = d
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let Some(b) = api::read_entity_bytes_by_id(entity.as_bytes(), id.as_bytes()) else {
            continue;
        };
        out.push((m.text(id.as_bytes()), m.text(&b)));
    }
    out
}

fn golden_stored(row: &Value, key: &str) -> Vec<(String, String)> {
    match row.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|i| {
                (
                    str_field(i, "id").to_string(),
                    str_field(i, "bytes").to_string(),
                )
            })
            .collect(),
        Some(Value::Null) | None => Vec::new(),
        other => panic!("bad stored {other:?}"),
    }
}

const BODIES: [&str; 8] = [
    r#"{"id":"w1","author":{"@entity":"person","id":"p1","name":"Ann"}}"#,
    r#"{"id":"w2","author":{"@entity":"person","name":"No id"}}"#,
    r#"{"id":"w3","author":{"@entity":"person","id":"p2"}}"#,
    r#"{"id":"w4","co":[{"@entity":"person","id":"p3","name":"C"},{"@entity":"person","name":"D"},"x",{"plain":1}]}"#,
    r#"{"id":"w5","nested":{"deep":{"@entity":"person","id":"p4","name":"Deep"}}}"#,
    r#"{"id":"w6","author":{"@entity":"person","id":"p5","pet":{"@entity":"pet","id":"pt1","name":"Rex"}}}"#,
    r#"{"id":"w7","bad":{"@entity":"","id":"x"},"num":{"@entity":5}}"#,
    r#"{"title":"no id at all"}"#,
];

const MIGRATIONS: [&str; 7] = [
    r#"[{"set":{"touched":true}}]"#,
    r#"[{"set":{"title.sub":"x"}}]"#,
    r#"[{"set":{"a.b.c":1}}]"#,
    r#"[{"set":[{"z":1},{"y":2}]}]"#,
    r#"[{"unknown":{"x":1}},{"set":{"never":1}}]"#,
    "[]",
    "[null]",
];

fn check_write(row: &Value, name: &str, result: Vec<u8>, m: &mut UuidMasker) {
    if str_field(row, "fn") != name {
        panic!("unexpected row {row:?} for {name}");
    }
    let got = m.text(&result);
    if got != str_field(row, "result") {
        panic!("{name}: result {got}");
    }
    let w = stored_entity("w", m);
    if w != golden_stored(row, "stored_w") {
        panic!("{name}: stored w {w:?}");
    }
    let p = stored_entity("person", m);
    if p != golden_stored(row, "stored_person") {
        panic!("{name}: stored person {p:?}");
    }
}

fn m(text: &str) -> Map {
    go_map(text.as_bytes())
}

#[test]
fn write_corpus() {
    let rows = read_jsonl("api/write_corpus.jsonl");
    let mut masker = UuidMasker::new();
    let mut next = 0;
    let mut checked = 0;

    {
        let _store = open_store(true, false, false);
        for (i, b) in BODIES.iter().enumerate() {
            let row = &rows[next];
            next += 1;
            let mut data = m(b);
            let subs = api::extract_sub_entities(b"w", &mut data);
            if str_field(row, "fn") != "ExtractSubEntities" || integer(row, "case") != i as i64 {
                panic!("unexpected row {row:?}");
            }
            let encoded_subs = if subs.is_empty() {
                b"null".to_vec()
            } else {
                marshal(&subs)
            };
            let got_subs = masker.text(&encoded_subs);
            let got_doc = masker.text(&marshal(&data));
            if got_subs != str_field(row, "subs") || got_doc != str_field(row, "doc_after") {
                panic!("case {i}: subs {got_subs} doc {got_doc}");
            }
            checked += 1;
        }
    }

    {
        let _store = open_store(true, false, false);
        for (i, b) in BODIES.iter().enumerate() {
            let mut data = m(b);
            let res = api::write_entity(b"w", &mut data);
            check_write(
                &rows[next],
                &format!("WriteEntity#{i}"),
                marshal(&res),
                &mut masker,
            );
            next += 1;
            checked += 1;
        }

        let res = api::update_entity_by_id(
            b"w",
            b"w1",
            &m(r#"{"title":"T","author":{"@entity":"person","id":"p1","name":"Ann B"}}"#),
        );
        check_write(&rows[next], "UpdateEntityById", marshal(&res), &mut masker);
        next += 1;
        let res = api::update_entity_by_id(b"w", b"zz", &m(r#"{"x":1}"#));
        check_write(
            &rows[next],
            "UpdateEntityById-missing",
            marshal(&res),
            &mut masker,
        );
        next += 1;
        let mut list = vec![
            m(r#"{"id":"l1","n":1}"#),
            m(r#"{"n":2}"#),
            m(r#"{"id":"l1","n":3}"#),
        ];
        let res = api::write_list_of_entities(b"w", &mut list);
        check_write(
            &rows[next],
            "WriteListOfEntities",
            marshal(&res),
            &mut masker,
        );
        next += 1;
        let res = api::update_list_of_entities(
            b"w",
            &[
                m(r#"{"id":"l1","n":4}"#),
                m(r#"{"id":"zz"}"#),
                m(r#"{"n":5}"#),
            ],
        );
        check_write(
            &rows[next],
            "UpdateListOfEntities",
            marshal(&res),
            &mut masker,
        );
        next += 1;
        let err = api::commit_batch(&[
            BatchOperation {
                kind: "create".to_string(),
                entity: b"w".to_vec(),
                id: b"bt1".to_vec(),
                data: m(r#"{"id":"bt1","v":1}"#),
            },
            BatchOperation {
                kind: "update".to_string(),
                entity: b"w".to_vec(),
                id: b"l1".to_vec(),
                data: m(r#"{"v":2}"#),
            },
            BatchOperation {
                kind: "delete".to_string(),
                entity: b"w".to_vec(),
                id: b"w7".to_vec(),
                data: Map::new(),
            },
        ]);
        let mut result = Map::new();
        result.insert("error".to_string(), Value::Bool(err.is_err()));
        check_write(&rows[next], "CommitBatch", marshal(&result), &mut masker);
        next += 1;
        checked += 5;
    }

    {
        let _store = open_store(true, false, false);
        api::write_entity(b"person", &mut m(r#"{"id":"p1","name":"Ann"}"#));
        api::write_entity(
            b"team",
            &mut m(
                r#"{"id":"t1","lead":{"@entity":"person","id":"p1"},"members":[{"@entity":"person","id":"p1"}],"title":"x"}"#,
            ),
        );
        for body in MIGRATIONS {
            let row = &rows[next];
            next += 1;
            if str_field(row, "fn") != "Migration" || str_field(row, "body") != body {
                panic!("unexpected row {row:?}");
            }
            let queries = api::parse_migration_query(body.as_bytes(), b"team");
            let got_queries = String::from_utf8(marshal(&queries)).expect("utf-8");
            if got_queries != str_field(row, "queries") {
                panic!("{body}: queries {got_queries}");
            }
            let (n, err) = api::execute_migrations_count(&queries);
            match err {
                Err(MigrationError::NotAnObject(e)) => {
                    if row.get("panic") != Some(&Value::Bool(true)) || row.get("count").is_some() {
                        panic!("{body}: unexpected failure {e}");
                    }
                    if e.to_string()
                        != r#"cannot set "title.sub": "title" is a string, not an object"#
                    {
                        panic!("{body}: message {e}");
                    }
                }
                other => {
                    if row.get("panic").is_some() || n as i64 != integer(row, "count") {
                        panic!("{body}: count {n} {other:?}");
                    }
                    let message = other.err().map(|e| e.to_string());
                    let want = row.get("error").map(|e| text(e).to_string());
                    if message != want {
                        panic!("{body}: error {message:?}");
                    }
                }
            }
            let team = stored_entity("team", &mut masker);
            if team != golden_stored(row, "stored_team") {
                panic!("{body}: stored team {team:?}");
            }
            let person = stored_entity("person", &mut masker);
            if person != golden_stored(row, "stored_person") {
                panic!("{body}: stored person {person:?}");
            }
            let schema = api::read_entity_bytes_by_id(b"_elysiandb_core_schema", b"team")
                .map(|b| String::from_utf8_lossy(&b).into_owned());
            let want = row.get("schema_document").map(|v| text(v).to_string());
            if schema != want {
                panic!("{body}: schema {schema:?}");
            }
            checked += 1;
        }
    }

    assert_eq!(checked, 28);
    assert_eq!(checked, rows.len());
}
