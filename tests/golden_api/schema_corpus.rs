use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::schema::{
    self, INCLUDED_DOCUMENTS_DISPLAY, INCLUDED_DOCUMENTS_TYPE, ValidationError,
};

use super::common::{read_jsonl, str_field};
use super::serial;

const REL: &str = "api/schema_corpus.jsonl";
const ROWS: usize = 79;
const ANALYZE_ROWS: usize = 4;
const MERGE_ROWS: usize = 16;
const ROUND_TRIP_ROWS: usize = 3;
const VALIDATE_ROWS: usize = 28;
const UPDATE_ROWS: usize = 28;

const DOCS: [&str; 4] = [
    r#"{"id":"1","s":"x","n":1,"b":true,"nil":null,"a":["x"],"ae":[],"o":{"k":"v","n":{"d":1}},"mixed":[1,"x"]}"#,
    r#"{"id":"2","s":5,"n":"x","b":"true","a":"x","o":[],"extra":1}"#,
    r#"{"id":"3"}"#,
    r#"{"id":"4","o":{"k":1,"n":null},"a":[{"x":1}]}"#,
];

const SCHEMAS: [&str; 2] = [
    r#"{"id":"ent","fields":{"title":{"name":"title","type":"string","required":true},"meta":{"name":"meta","type":"object","required":false,"fields":{"k":{"name":"k","type":"string","required":true}}},"n":{"name":"n","type":"number","required":false}}}"#,
    r#"{"id":"ent","_manual":true,"fields":{"title":{"name":"title","type":"string","required":true}}}"#,
];

const TYPE_ERRORS: &[(&str, &str)] = &[
    ("meta", "expected type object but got string"),
    ("n", "expected type number but got string"),
    ("title", "expected type string but got number"),
];

const STRICT_MISSING: &[(&str, &str)] = &[
    ("n", "field not allowed by strict schema"),
    ("title", "required field missing"),
];

const STRICT_EXTRA: &[(&str, &str)] = &[
    ("extra", "field not allowed by strict schema"),
    ("meta", "field not allowed by strict schema"),
    ("n", "field not allowed by strict schema"),
    ("title", "expected type string but got number"),
];

const D20_ORDER: &[(usize, &[(&str, &str)])] = &[
    (36, TYPE_ERRORS),
    (37, TYPE_ERRORS),
    (64, TYPE_ERRORS),
    (65, TYPE_ERRORS),
    (68, STRICT_MISSING),
    (78, STRICT_EXTRA),
    (79, STRICT_EXTRA),
];

fn decode(text: &str) -> Map {
    let mut v = json::unmarshal_value(text.as_bytes())
        .unwrap_or_else(|e| panic!("cannot decode {text}: {e}"));
    match &mut v {
        Value::Object(m) => std::mem::take(m),
        other => panic!("not an object: {other:?}"),
    }
}

fn redecode(v: &Value) -> Map {
    let bytes = json::marshal(v).unwrap_or_else(|e| panic!("{e}"));
    decode(&String::from_utf8_lossy(&bytes))
}

fn encode(m: &Map) -> String {
    let bytes = json::marshal(m).unwrap_or_else(|e| panic!("{e}"));
    String::from_utf8(bytes).unwrap_or_else(|e| panic!("{e}"))
}

fn set_config(strict: bool) {
    let mut cfg = Config::default();
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = strict;
    globals::set_config(Some(Arc::new(cfg)));
}

fn bool_field(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

fn index_field(row: &Value, key: &str) -> usize {
    row.get(key)
        .and_then(Value::as_u64)
        .and_then(|n| usize::try_from(n).ok())
        .unwrap_or_else(|| panic!("missing index field {key} in {row:?}"))
}

fn display(message: &str) -> String {
    message.replace(INCLUDED_DOCUMENTS_TYPE, INCLUDED_DOCUMENTS_DISPLAY)
}

fn expected_errors(row: &Value) -> Vec<(String, String)> {
    let Some(Value::Array(list)) = row.get("errors") else {
        panic!("missing errors in {row:?}");
    };

    list.iter()
        .map(|pair| match pair.as_array().map(Vec::as_slice) {
            Some([Value::String(field), Value::String(message)]) => {
                (field.clone(), display(message))
            }
            _ => panic!("bad error pair {pair:?}"),
        })
        .collect()
}

fn pairs(errs: &[ValidationError]) -> Vec<(String, String)> {
    errs.iter()
        .map(|e| (e.field.clone(), e.message.clone()))
        .collect()
}

fn sorted(errs: &[ValidationError]) -> Vec<(String, String)> {
    let mut out = pairs(errs);
    out.sort();
    out
}

fn check_errors(line: usize, row: &Value, errs: &[ValidationError], ordered: &mut usize) {
    let want = expected_errors(row);
    assert_eq!(sorted(errs), want, "line {line}: {row:?}");

    if errs.len() < 2 {
        return;
    }

    let Some((_, order)) = D20_ORDER.iter().find(|(l, _)| *l == line) else {
        panic!("line {line}: no D20 order for several errors");
    };
    let order: Vec<(String, String)> = order
        .iter()
        .map(|(f, m)| (f.to_string(), m.to_string()))
        .collect();
    assert_eq!(pairs(errs), order, "line {line}: D20 order");
    *ordered += 1;
}

#[test]
fn schema_corpus_matches_go() {
    let _serial = serial();
    let rows = read_jsonl(REL);
    assert_eq!(rows.len(), ROWS);

    let mut analyzed: Vec<Map> = Vec::new();
    let (mut analyze, mut merge, mut round_trip, mut validate, mut update) = (0, 0, 0, 0, 0);
    let mut ordered = 0;
    let mut checked = 0;

    for (i, row) in rows.iter().enumerate() {
        let line = i + 1;
        match str_field(row, "fn") {
            "AnalyzeEntitySchema" => {
                set_config(false);
                let doc = str_field(row, "doc");
                assert_eq!(doc, DOCS[analyze], "line {line}");
                let out = schema::analyze_entity_schema(b"ent", &decode(doc));
                assert_eq!(encode(&out), str_field(row, "out"), "line {line}");
                analyzed.push(decode(str_field(row, "out")));
                analyze += 1;
            }
            "MergeAnalyzedSchema" => {
                set_config(false);
                let existing = &analyzed[index_field(row, "existing")];
                let doc = decode(str_field(row, "doc"));
                let out = schema::merge_analyzed_schema(b"ent", Some(existing), &doc);
                assert_eq!(encode(&out), str_field(row, "out"), "line {line}");
                merge += 1;
            }
            "MapToFields+FieldsToMap" => {
                let input = decode(str_field(row, "in"));
                let back = schema::fields_to_map(&schema::map_to_fields(&input));
                assert_eq!(encode(&back), str_field(row, "out"), "line {line}");
                round_trip += 1;
            }
            "ValidateEntity" => {
                let strict = bool_field(row, "strict");
                set_config(strict);
                let sc = decode(SCHEMAS[index_field(row, "schema")]);
                assert_eq!(
                    schema::is_manual_schema(b"ent", Some(&sc)),
                    bool_field(row, "manual"),
                    "line {line}"
                );
                let doc = decode(str_field(row, "doc"));
                let errs = schema::validate_entity(b"ent", &doc, Some(&sc));
                check_errors(line, row, &errs, &mut ordered);
                validate += 1;
            }
            "ValidateEntityUpdate" => {
                let strict = bool_field(row, "strict");
                set_config(strict);
                let sc = decode(SCHEMAS[index_field(row, "schema")]);
                let existing = redecode(row.get("existing").unwrap_or(&Value::Null));
                assert_eq!(encode(&existing), r#"{"n":1,"title":"old"}"#, "line {line}");
                let updates = decode(str_field(row, "updates"));
                let errs =
                    schema::validate_entity_update(b"ent", Some(&existing), &updates, Some(&sc));
                check_errors(line, row, &errs, &mut ordered);
                update += 1;
            }
            other => panic!("line {line}: unknown function {other}"),
        }
        checked += 1;
    }

    assert_eq!(analyze, ANALYZE_ROWS);
    assert_eq!(merge, MERGE_ROWS);
    assert_eq!(round_trip, ROUND_TRIP_ROWS);
    assert_eq!(validate, VALIDATE_ROWS);
    assert_eq!(update, UPDATE_ROWS);
    assert_eq!(ordered, D20_ORDER.len());
    assert_eq!(checked, ROWS);
}
