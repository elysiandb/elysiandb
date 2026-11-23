#![allow(dead_code)]

use elysiandb::gocompat::json::{self, Value};
use std::path::PathBuf;

pub fn golden_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(rel)
}

pub fn read_golden(rel: &str) -> Vec<u8> {
    std::fs::read(golden_path(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

pub fn lines(data: &[u8]) -> Vec<&[u8]> {
    let mut out: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    if out.last().is_some_and(|l| l.is_empty()) {
        out.pop();
    }
    out
}

pub fn read_jsonl(rel: &str) -> Vec<Value> {
    let data = read_golden(rel);
    lines(&data)
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            json::unmarshal_value(line).unwrap_or_else(|e| panic!("{rel}:{}: {e}", i + 1))
        })
        .collect()
}

pub fn read_json(rel: &str) -> Value {
    let data = read_golden(rel);
    json::unmarshal_value(&data).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

pub fn unhex(s: &str) -> Vec<u8> {
    assert!(s.len().is_multiple_of(2), "odd hex length: {s}");
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16).unwrap_or_else(|e| panic!("bad hex {s}: {e}"))
        })
        .collect()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

pub fn str_field<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {key} in {row:?}"))
}
