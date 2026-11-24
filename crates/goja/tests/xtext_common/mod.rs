#![allow(dead_code)]

use gocompat::json::{self, Value};
use gocompat::utf8::{self, Rune};
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
    let mut lines: Vec<&[u8]> = data.split(|&b| b == b'\n').collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
        .into_iter()
        .enumerate()
        .map(|(i, line)| {
            json::unmarshal_value(line).unwrap_or_else(|e| panic!("{rel}:{}: {e}", i + 1))
        })
        .collect()
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

pub fn hex_field(row: &Value, key: &str) -> Vec<u8> {
    unhex(str_field(row, key))
}

pub fn int_field(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing int field {key} in {row:?}"))
}

pub fn array_field<'a>(row: &'a Value, key: &str) -> &'a Vec<Value> {
    row.get(key)
        .and_then(Value::as_array)
        .unwrap_or_else(|| panic!("missing array field {key} in {row:?}"))
}

pub fn ints(v: &Value) -> Vec<i64> {
    v.as_array()
        .unwrap_or_else(|| panic!("not an array: {v:?}"))
        .iter()
        .map(|x| x.as_i64().unwrap_or_else(|| panic!("not an int: {x:?}")))
        .collect()
}

pub fn bools(v: &Value) -> Vec<bool> {
    v.as_array()
        .unwrap_or_else(|| panic!("not an array: {v:?}"))
        .iter()
        .map(|x| x.as_bool().unwrap_or_else(|| panic!("not a bool: {x:?}")))
        .collect()
}

pub fn strs(v: &Value) -> Vec<&str> {
    v.as_array()
        .unwrap_or_else(|| panic!("not an array: {v:?}"))
        .iter()
        .map(|x| x.as_str().unwrap_or_else(|| panic!("not a string: {x:?}")))
        .collect()
}

pub fn go_string(r: Rune) -> Vec<u8> {
    let mut b = Vec::with_capacity(4);
    let r = if utf8::valid_rune(r) {
        r
    } else {
        utf8::RUNE_ERROR
    };
    utf8::append_rune(&mut b, r);
    b
}

pub fn runes(lo: Rune, hi: Rune, mut f: impl FnMut(Rune)) {
    for c in lo..hi {
        if (0xD800..=0xDFFF).contains(&c) {
            continue;
        }
        f(c);
    }
}

pub struct Digest(u64);

impl Default for Digest {
    fn default() -> Self {
        Self::new()
    }
}

impl Digest {
    pub fn new() -> Digest {
        Digest(0xcbf2_9ce4_8422_2325)
    }

    fn write(&mut self, b: &[u8]) {
        for &x in b {
            self.0 ^= u64::from(x);
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }

    pub fn add(&mut self, b: &[u8]) {
        self.write(&(b.len() as u32).to_le_bytes());
        self.write(b);
    }

    pub fn add_int(&mut self, v: i32) {
        self.add(&[(v + 1) as u8]);
    }

    pub fn sum(&self) -> String {
        format!("{:016x}", self.0)
    }
}

pub fn concat(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for p in parts {
        out.extend_from_slice(p);
    }
    out
}
