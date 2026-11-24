#![allow(dead_code)]

pub mod dump;

use gocompat::json::{self, Value};
use std::path::PathBuf;

pub fn golden_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/golden")
        .join(rel)
}

pub fn read_golden(rel: &str) -> Vec<u8> {
    std::fs::read(golden_path(rel)).unwrap_or_else(|e| panic!("cannot read {rel}: {e}"))
}

pub fn read_jsonl(rel: &str) -> Vec<Value> {
    let data = read_golden(rel);
    let rows: Vec<Value> = data
        .split(|&b| b == b'\n')
        .filter(|l| !l.is_empty())
        .enumerate()
        .map(|(i, line)| {
            json::unmarshal_value(line).unwrap_or_else(|e| panic!("{rel}:{}: {e}", i + 1))
        })
        .collect();
    assert!(!rows.is_empty(), "{rel} is empty");
    rows
}

pub fn unhex(s: &str) -> Vec<u8> {
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

pub fn s<'a>(row: &'a Value, key: &str) -> &'a str {
    row.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string field {key} in {row:?}"))
}

pub fn i(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

pub fn b(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

pub fn opt_s<'a>(row: &'a Value, key: &str) -> Option<&'a str> {
    row.get(key).and_then(Value::as_str)
}

pub fn f64_bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).unwrap_or_else(|e| panic!("bad bits {s}: {e}")))
}

pub fn bits_f64(f: f64) -> String {
    format!("{:016x}", f.to_bits())
}
