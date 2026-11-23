mod common;

use elysiandb::gocompat::json::{self, SyntaxReason, Value};
use std::path::Path;

fn walk(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable golden folder") {
        let path = entry.expect("folder entry").path();
        if path.is_dir() {
            walk(&path, files);
        } else if path
            .extension()
            .is_some_and(|e| e == "jsonl" || e == "json")
        {
            files.push(path);
        }
    }
}

#[test]
fn every_golden_json_file_parses() {
    let mut files = Vec::new();
    walk(&common::golden_path(""), &mut files);
    files.sort();
    let mut rows = 0;
    for path in &files {
        let data = std::fs::read(path).expect("readable golden file");
        if path.extension().is_some_and(|e| e == "jsonl") {
            for (i, line) in common::lines(&data).into_iter().enumerate() {
                json::unmarshal_value(line)
                    .unwrap_or_else(|e| panic!("{}:{}: {e}", path.display(), i + 1));
                rows += 1;
            }
        } else if json::check_valid(&data).is_ok() {
            json::unmarshal_value(&data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
            rows += 1;
        }
    }
    assert!(files.len() > 100, "only {} golden JSON files", files.len());
    assert!(rows > 80_000, "only {rows} golden JSON rows");
}

#[test]
fn decode_validity_and_syntax_offsets_match_go() {
    let rows = common::read_jsonl("gocompat/json/decode.jsonl");
    let data = common::read_golden("gocompat/json/decode.jsonl");
    assert_eq!(rows.len(), common::lines(&data).len());
    for row in &rows {
        let input = common::unhex(common::str_field(row, "in"));
        let valid = row
            .get("valid")
            .and_then(Value::as_bool)
            .expect("valid flag");
        let checked = json::check_valid(&input);
        assert_eq!(
            checked.is_ok(),
            valid,
            "validity of {:?}",
            String::from_utf8_lossy(&input)
        );
        let go_error = row
            .get("raw_error")
            .filter(|e| e.get("kind").and_then(Value::as_str) == Some("syntax"));
        match (go_error, &checked) {
            (Some(e), Err(err)) => {
                assert_eq!(
                    e.get("offset").and_then(Value::as_i64),
                    Some(err.offset),
                    "offset for {:?}",
                    String::from_utf8_lossy(&input)
                );
            }
            (None, Ok(())) => {}
            (g, c) => panic!(
                "syntax result differs for {:?}: go {g:?}, rust {c:?}",
                String::from_utf8_lossy(&input)
            ),
        }
    }
}

#[test]
fn syntax_messages_follow_the_catalog() {
    let cases: [(&[u8], &str); 9] = [
        (
            b"not json",
            "invalid JSON at byte 2: unexpected character 'o'",
        ),
        (b"", "invalid JSON: unexpected end of input"),
        (
            b"{\"entity\":\"b\",\"limit\":5,}",
            "invalid JSON at byte 25: unexpected character '}'",
        ),
        (
            b"{\"entity\":\"b\"} x",
            "invalid JSON at byte 16: unexpected character 'x'",
        ),
        (
            b"{\"entity\":\"a\x01b\"}",
            "invalid JSON at byte 13: unexpected character '\\^A'",
        ),
        (
            b"\"\\x\"",
            "invalid JSON at byte 3: invalid escape sequence",
        ),
        (b"-x", "invalid JSON at byte 2: invalid number"),
        (b"1e", "invalid JSON: unexpected end of input"),
        (b"[1,2", "invalid JSON: unexpected end of input"),
    ];
    for (input, want) in cases {
        let err = json::check_valid(input).expect_err("invalid input");
        assert_eq!(err.to_string(), want);
    }
    let deep = format!("{}{}", "[".repeat(10001), "]".repeat(10001));
    let err = json::check_valid(deep.as_bytes()).expect_err("too deep");
    assert_eq!(err.reason, SyntaxReason::NestingTooDeep);
    assert_eq!(
        err.to_string(),
        "invalid JSON at byte 10001: nesting too deep"
    );
}

#[test]
fn values_decode_with_go_string_rules() {
    let v = json::unmarshal_value(
        b"{\"a\":1,\"a\":\"\\ud83d\\ude00\",\"b\":[true,null,\"\xff\xe2\x82x\"],\"c\":-1.5e3}",
    )
    .expect("valid");
    assert_eq!(v.get("a").and_then(Value::as_str), Some("\u{1F600}"));
    let b = v.get("b").and_then(Value::as_array).expect("array");
    assert_eq!(b[0], Value::Bool(true));
    assert!(b[1].is_null());
    assert_eq!(b[2].as_str(), Some("\u{FFFD}\u{FFFD}\u{FFFD}x"));
    assert_eq!(
        v.get("c").and_then(Value::as_number).map(|n| n.as_str()),
        Some("-1.5e3")
    );
    assert_eq!(v.as_object().map(|m| m.len()), Some(3));
    let lone = json::unmarshal_value(b"\"\\ud83dx\"").expect("valid");
    assert_eq!(lone.as_str(), Some("\u{FFFD}x"));
}

#[test]
fn quoted_text_matches_go_without_html_escaping() {
    let rel = "gocompat/json/encode.jsonl";
    let mut checked = 0;
    for row in common::read_jsonl(rel) {
        let value = row.get("value").unwrap();
        if value.index(0).and_then(Value::as_str) != Some("str") {
            continue;
        }
        let input = common::unhex(value.index(1).unwrap().as_str().unwrap());
        let want = row.get("encode_no_html").unwrap().as_str().unwrap();
        let got = json::quote_text(&input) + "\n";
        assert_eq!(got, want, "{:?}", value.index(1));
        checked += 1;
    }
    assert_eq!(checked, 1716);
}
