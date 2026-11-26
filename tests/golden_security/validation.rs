use crate::assert_manifest_entry;
use crate::common::{read_jsonl, str_field, unhex};
use elysiandb::gocompat::json::Value;
use elysiandb::security;

const ROWS: usize = 21;

fn expected_error(row: &Value, key: &str) -> Option<String> {
    match row.get(key) {
        Some(Value::Null) => None,
        Some(Value::String(s)) => Some(s.clone()),
        other => panic!("unexpected {key} {other:?} in {row:?}"),
    }
}

#[test]
fn validation_rows_match_go() {
    assert_manifest_entry("validation.jsonl");
    let mut checked = 0;
    for row in &read_jsonl("security/validation.jsonl") {
        let value = unhex(str_field(row, "value_hex"));

        let username = security::validate_username(&value)
            .err()
            .map(|e| e.to_string());
        assert_eq!(username, expected_error(row, "username_err"), "{row:?}");

        let password = security::validate_password(&value)
            .err()
            .map(|e| e.to_string());
        assert_eq!(password, expected_error(row, "password_err"), "{row:?}");
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
