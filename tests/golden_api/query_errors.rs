use std::collections::BTreeMap;

use elysiandb::api;
use elysiandb::gocompat::json::{self, Map, Subject, Value};
use elysiandb::query::Query;
use elysiandb::transport::http::api::{decode_query_payload, parse_filter_node};

use super::common::{read_jsonl, str_field, unhex};
use super::open_store;

const G3: [(&str, &str); 14] = [
    (
        r#"{"entity":"b","limit":"5"}"#,
        r#"invalid JSON: "limit" must be an integer, got a string"#,
    ),
    (
        r#"{"entity":"b","limit":1.5}"#,
        r#"invalid JSON: "limit" must be an integer, got a number"#,
    ),
    (
        r#"{"entity":"b","limit":1e3}"#,
        r#"invalid JSON: "limit" must be an integer, got a number"#,
    ),
    (
        r#"{"entity":"b","limit":99999999999999999999}"#,
        r#"invalid JSON: "limit": 99999999999999999999 is out of range"#,
    ),
    (
        r#"{"entity":5}"#,
        r#"invalid JSON: "entity" must be a string, got a number"#,
    ),
    (
        r#"{"entity":"b","countOnly":"true"}"#,
        r#"invalid JSON: "countOnly" must be a boolean, got a string"#,
    ),
    (
        r#"{"entity":"b","fields":["a"]}"#,
        r#"invalid JSON: "fields" must be a string, got an array"#,
    ),
    (
        r#"{"entity":"b","sorts":{"a":1}}"#,
        r#"invalid JSON: "sorts.a" must be a string, got a number"#,
    ),
    (
        r#"{"entity":"b","sorts":[]}"#,
        r#"invalid JSON: "sorts" must be an object of strings, got an array"#,
    ),
    (
        r#"{"entity":"b","filters":[]}"#,
        r#"invalid JSON: "filters" must be an object, got an array"#,
    ),
    (
        r#"{"entity":"b","filters":{"x":{"eq":1e400}}}"#,
        r#"invalid JSON: "filters.x.eq": 1e400 is out of range"#,
    ),
    (
        r#"{"LIMIT":"x"}"#,
        r#"invalid JSON: "LIMIT" must be an integer, got a string"#,
    ),
    (
        "[1]",
        "invalid JSON: the body must be an object, got an array",
    ),
    (
        "not json",
        "invalid JSON at byte 2: unexpected character 'o'",
    ),
];

fn error_body(message: &str) -> String {
    let mut m = BTreeMap::new();
    m.insert("error".to_string(), message.to_string());
    String::from_utf8(json::marshal(&m).expect("encode")).expect("utf-8")
}

pub fn respond(body: &[u8]) -> (i64, String) {
    let payload = match decode_query_payload(body) {
        Ok(p) => p,
        Err(err) => return (400, error_body(&err.message(Subject::Body))),
    };

    if api::is_core_entity_type(payload.entity.as_bytes()) {
        return (
            403,
            error_body("core entities cannot be accessed through the generic entity routes"),
        );
    }

    let filters = payload.filters.clone().unwrap_or_default();
    let filter = match parse_filter_node(Some(&filters)) {
        Ok(f) => f,
        Err(err) => return (400, error_body(&err.to_string())),
    };

    let q = Query {
        entity: payload.entity.as_bytes().to_vec(),
        offset: payload.offset,
        limit: payload.limit,
        filter,
        sorts: payload.query_for_sorts().sorts,
        sort_order: payload
            .sort_order
            .iter()
            .map(|k| k.as_bytes().to_vec())
            .collect(),
    };

    if payload.count_only {
        let res = api::plan_query_guarded(&q, true, None);
        let paged = q.offset > 0 || q.limit > 0 || !q.ordered_sorts().is_empty();
        let n = if paged { res.docs.len() } else { res.total };
        return (200, format!("{{\"count\":{n}}}"));
    }

    let res = api::plan_query_guarded(&q, false, None);
    let mut out = b"[".to_vec();
    for (i, d) in res.docs.iter().enumerate() {
        if i > 0 {
            out.push(b',');
        }
        out.extend_from_slice(&d.bytes().expect("stored bytes"));
    }
    out.push(b']');

    (200, String::from_utf8(out).expect("utf-8"))
}

fn error_message(body: &str) -> String {
    let v = json::unmarshal_value(body.as_bytes()).expect("error body");
    v.get("error")
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("no error in {body}"))
        .to_string()
}

pub fn check_row(row: &Value) {
    let body = match row.get("request_body_hex") {
        Some(Value::String(h)) => unhex(h),
        _ => str_field(row, "request_body").as_bytes().to_vec(),
    };
    let status = match row.get("status") {
        Some(Value::Int(n)) => *n,
        other => panic!("no status: {other:?}"),
    };

    let (got_status, got_body) = respond(&body);
    if got_status != status {
        panic!("{row:?}: status {got_status} body {got_body}");
    }

    for (input, want) in G3 {
        if body == input.as_bytes() && error_message(&got_body) != want {
            panic!("{input}: got {got_body}, want {want}");
        }
    }

    match row.get("message_kind").and_then(Value::as_str) {
        Some("go") => {
            let message = error_message(&got_body);
            if let Some(Value::Int(offset)) = row.get("go_offset") {
                let prefix = format!("invalid JSON at byte {offset}: ");
                let eof = *offset == body.len() as i64
                    && message == "invalid JSON: unexpected end of input";
                if !eof && !message.starts_with(&prefix) {
                    panic!("{row:?}: {message}");
                }
            } else if let Some(Value::String(field)) = row.get("go_field")
                && field.is_empty()
            {
                if !message.starts_with("invalid JSON: the body must be ") {
                    panic!("{row:?}: {message}");
                }
            } else if let Some(Value::String(field)) = row.get("go_field") {
                let rest = message
                    .strip_prefix("invalid JSON: \"")
                    .unwrap_or_else(|| panic!("{row:?}: {message}"));
                if !rest.to_lowercase().starts_with(&field.to_lowercase()) {
                    panic!("{row:?}: {message}");
                }
            } else if !message.starts_with("invalid JSON") {
                panic!("{row:?}: {message}");
            }
        }
        Some(other) => panic!("unknown message kind {other}"),
        None => {
            let want = match row.get("body") {
                Some(Value::String(b)) => b.clone(),
                other => panic!("{row:?}: no body {other:?}"),
            };
            if body.is_empty() {
                let expected = error_body("invalid JSON: unexpected end of input");
                if got_body != expected {
                    panic!("empty body: got {got_body}");
                }
            } else if got_body != want {
                panic!("{row:?}: got {got_body}");
            }
        }
    }
}

#[test]
fn query_payload_errors() {
    let rows = read_jsonl("api/query_payload_errors.jsonl");
    let _store = open_store(false, false, false);
    let mut seed = Map::new();
    seed.insert("id".to_string(), Value::String("x".to_string()));
    seed.insert("t".to_string(), Value::String("v".to_string()));
    api::write_entity(b"b", &mut seed);

    let mut checked = 0;
    for row in &rows {
        check_row(row);
        checked += 1;
    }

    assert_eq!(checked, 94);
    assert_eq!(checked, rows.len());
}
