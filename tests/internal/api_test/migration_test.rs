use elysiandb::api::{self, MigrationQuery};
use elysiandb::gocompat::json::{self, Value};

use super::{doc, setup, value};

#[test]
fn parse_migration_query_single_set() {
    let _s = setup();
    let result = api::parse_migration_query(br#"[{"set": [{"name": "test"}]}]"#, b"users");
    if result.len() != 1 {
        panic!("expected 1 query, got {}", result.len());
    }
    let q = &result[0];
    if q.entity != b"users" {
        panic!(
            "expected entity 'users', got {}",
            String::from_utf8_lossy(&q.entity)
        );
    }
    if q.action != "set" {
        panic!("expected action 'set', got {}", q.action);
    }
    let name = q.properties.as_ref().and_then(|p| p.get("name"));
    if name.and_then(Value::as_str) != Some("test") {
        panic!("expected property name 'test', got {name:?}");
    }
}

#[test]
fn parse_migration_query_invalid_json() {
    let _s = setup();
    let result = api::parse_migration_query(b"invalid json", b"users");
    if !result.is_empty() {
        panic!("expected nil for invalid JSON");
    }
}

#[test]
fn execute_migrations_unsupported_action() {
    let _s = setup();
    let q = vec![MigrationQuery {
        entity: b"users".to_vec(),
        action: "unknown".to_string(),
        properties: Some(doc(r#"{"name":"x"}"#)),
    }];
    match api::execute_migrations(&q) {
        Err(err) if err.to_string() == "unsupported action: unknown" => {}
        other => panic!("expected error for unsupported action, got {other:?}"),
    }
}

#[test]
fn parse_migration_query_object_syntax() {
    let _s = setup();
    let raw = json::marshal(&value(r#"[{"set":{"age":30}}]"#)).expect("encode");
    let result = api::parse_migration_query(&raw, b"users");
    if result.len() != 1 {
        panic!("expected 1 query, got {}", result.len());
    }
    let age = result[0].properties.as_ref().and_then(|p| p.get("age"));
    if age != Some(&Value::Number(30.0)) {
        panic!("expected property age 30, got {age:?}");
    }
}
