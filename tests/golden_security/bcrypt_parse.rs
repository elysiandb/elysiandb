use crate::assert_manifest_entry;
use crate::common::{read_jsonl, str_field, unhex};
use elysiandb::crypto::bcrypt::{self, Error};
use elysiandb::gocompat::json::Value;

const ROWS: usize = 16;
const NAMES: [&str; ROWS] = [
    "orig",
    "2",
    "2b",
    "2x",
    "2y",
    "3a",
    "cost03",
    "cost04",
    "cost31",
    "cost32",
    "trunc58",
    "trunc59",
    "alter-salt",
    "alter-hash",
    "empty",
    "garbage",
];

fn rejection(name: &str) -> Error {
    match name {
        "3a" => Error::HashVersionTooNew(b'3'),
        "cost03" => Error::InvalidCost(3),
        "cost32" => Error::InvalidCost(32),
        "trunc58" | "empty" | "garbage" => Error::HashTooShort,
        _ => Error::MismatchedHashAndPassword,
    }
}

#[test]
fn bcrypt_parse_rows_match_go() {
    assert_manifest_entry("bcrypt_parse.jsonl");
    let mut checked = 0;
    for (i, row) in read_jsonl("security/bcrypt_parse.jsonl").iter().enumerate() {
        assert_eq!(str_field(row, "name"), NAMES[i], "{row:?}");
        let hash = str_field(row, "hash").as_bytes();
        let sum = unhex(str_field(row, "digest_hex"));
        match str_field(row, "result") {
            "not computed" => {
                let parsed = row.get("parsed_cost").and_then(Value::as_i64).unwrap();
                let parse_error = row.get("parse_error").and_then(Value::as_bool).unwrap();
                assert!(!parse_error, "{row:?}");
                assert_eq!(bcrypt::cost(hash), Ok(parsed), "{row:?}");
            }
            "match" => {
                assert_eq!(
                    bcrypt::compare_hash_and_password(hash, &sum),
                    Ok(()),
                    "{row:?}"
                );
            }
            "no match" => {
                assert_eq!(
                    bcrypt::compare_hash_and_password(hash, &sum),
                    Err(rejection(NAMES[i])),
                    "{row:?}"
                );
            }
            other => panic!("unexpected result {other} in {row:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
