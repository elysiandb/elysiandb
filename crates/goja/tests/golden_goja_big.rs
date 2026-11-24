mod common;

use common::{b, bits_f64, f64_bits, hex, i, opt_s, read_jsonl, s};
use gocompat::json::Value;
use goja::deps::big::{Accuracy, Int, cmp_float64, float_int, float_is_int};
use std::panic::{AssertUnwindSafe, catch_unwind};

fn int(dec: &str) -> Int {
    Int::set_string(dec.as_bytes(), 10).unwrap_or_else(|| panic!("bad value {dec}"))
}

fn values() -> Vec<Int> {
    read_jsonl("goja-front/big/values.jsonl")
        .iter()
        .map(|row| int(s(row, "dec")))
        .collect()
}

fn acc(a: Accuracy) -> i64 {
    a as i64
}

fn try_op(f: impl FnOnce() -> Int) -> Option<String> {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let r = catch_unwind(AssertUnwindSafe(f));
    std::panic::set_hook(prev);
    r.ok().map(|z| z.text(16))
}

fn expected_op(row: &Value, key: &str) -> Option<String> {
    match row.get(key) {
        Some(Value::String(v)) => Some(v.clone()),
        Some(Value::Object(_)) => None,
        other => panic!("bad {key}: {other:?}"),
    }
}

#[test]
fn big_values_follow_go() {
    let rows = read_jsonl("goja-front/big/values.jsonl");
    for row in &rows {
        let v = int(s(row, "dec"));
        let ctx = s(row, "dec");
        assert_eq!(v.string(), ctx);
        assert_eq!(i64::from(v.sign()), i(row, "sign"), "{ctx}");
        assert_eq!(v.bit_len() as i64, i(row, "bitlen"), "{ctx}");
        assert_eq!(v.trailing_zero_bits() as i64, i(row, "tz"), "{ctx}");
        assert_eq!(v.int64().to_string(), s(row, "int64"), "{ctx}");
        assert_eq!(v.uint64().to_string(), s(row, "uint64"), "{ctx}");
        assert_eq!(v.is_int64(), b(row, "isint64"), "{ctx}");
        assert_eq!(v.is_uint64(), b(row, "isuint"), "{ctx}");
        let texts = row.get("text").and_then(Value::as_object).expect("text");
        for (base, text) in texts.iter() {
            let base: u32 = base.parse().expect("base");
            assert_eq!(
                v.text(base),
                text.as_str().expect("text"),
                "{ctx} base {base}"
            );
            assert_eq!(
                Int::set_string(text.as_str().expect("text").as_bytes(), base),
                Some(v.clone()),
                "{ctx} parse base {base}"
            );
        }
        assert_eq!(hex(&v.bytes()), s(row, "bytes"), "{ctx}");
        assert_eq!(Int::set_bytes(&v.bytes()), v.abs(), "{ctx}");
        let (f, a) = v.float64();
        assert_eq!(bits_f64(f), s(row, "f64"), "{ctx}");
        assert_eq!(acc(a), i(row, "acc"), "{ctx}");
        assert_eq!(v.neg().text(16), s(row, "neg"), "{ctx}");
        assert_eq!(v.not().text(16), s(row, "not"), "{ctx}");
        assert_eq!(v.abs().text(16), s(row, "abs"), "{ctx}");
    }
}

#[test]
fn big_binary_operations_follow_go() {
    let vals = values();
    let rows = read_jsonl("goja-front/big/binary.jsonl");
    assert_eq!(rows.len(), vals.len() * vals.len());
    for row in &rows {
        let x = &vals[i(row, "x") as usize];
        let y = &vals[i(row, "y") as usize];
        let ctx = format!("{x} {y}");
        assert_eq!(x.add(y).text(16), s(row, "add"), "add {ctx}");
        assert_eq!(x.sub(y).text(16), s(row, "sub"), "sub {ctx}");
        assert_eq!(x.mul(y).text(16), s(row, "mul"), "mul {ctx}");
        assert_eq!(try_op(|| x.quo(y)), expected_op(row, "quo"), "quo {ctx}");
        assert_eq!(try_op(|| x.rem(y)), expected_op(row, "rem"), "rem {ctx}");
        assert_eq!(try_op(|| x.div(y)), expected_op(row, "div"), "div {ctx}");
        assert_eq!(try_op(|| x.modulo(y)), expected_op(row, "mod"), "mod {ctx}");
        assert_eq!(x.and(y).text(16), s(row, "and"), "and {ctx}");
        assert_eq!(x.or(y).text(16), s(row, "or"), "or {ctx}");
        assert_eq!(x.xor(y).text(16), s(row, "xor"), "xor {ctx}");
        assert_eq!(x.and_not(y).text(16), s(row, "andnot"), "andnot {ctx}");
        assert_eq!(i64::from(x.cmp(y)), i(row, "cmp"), "cmp {ctx}");
        assert_eq!(i64::from(x.cmp_abs(y)), i(row, "cmpabs"), "cmpabs {ctx}");
        if y.sign() != 0 {
            let (q, r) = x.quo_rem(y);
            assert_eq!(q.text(16), s(row, "quo"), "quorem {ctx}");
            assert_eq!(r.text(16), s(row, "rem"), "quorem {ctx}");
        }
    }
}

#[test]
fn big_shifts_follow_go() {
    let vals = values();
    for row in &read_jsonl("goja-front/big/shift.jsonl") {
        let x = &vals[i(row, "x") as usize];
        let n = i(row, "n") as u64;
        assert_eq!(x.lsh(n).text(16), s(row, "lsh"), "{x} << {n}");
        assert_eq!(x.rsh(n).text(16), s(row, "rsh"), "{x} >> {n}");
    }
}

#[test]
fn big_exp_follows_go() {
    for row in &read_jsonl("goja-front/big/exp.jsonl") {
        let x = int(s(row, "x"));
        let y = int(s(row, "y"));
        let m = opt_s(row, "m").map(int);
        let z = x.exp(&y, m.as_ref()).map(|z| z.string());
        assert_eq!(z.as_deref(), opt_s(row, "z"), "{x} ** {y} mod {m:?}");
    }
}

#[test]
fn big_set_string_follows_go() {
    for row in &read_jsonl("goja-front/big/setstring.jsonl") {
        let input = s(row, "s");
        let base = i(row, "base") as u32;
        let got = Int::set_string(input.as_bytes(), base);
        assert_eq!(got.is_some(), b(row, "ok"), "{input:?} base {base}");
        if let Some(v) = got {
            assert_eq!(v.string(), s(row, "v"), "{input:?} base {base}");
        }
    }
}

#[test]
fn big_float_to_int_follows_go() {
    for row in &read_jsonl("goja-front/big/from_float.jsonl") {
        let f = f64_bits(s(row, "f"));
        assert_eq!(float_is_int(f), b(row, "isint"), "{f}");
        let (n, a) = float_int(f);
        assert_eq!(n.map(|n| n.string()).as_deref(), opt_s(row, "int"), "{f}");
        assert_eq!(acc(a), i(row, "acc"), "{f}");
    }
}

#[test]
fn big_compare_with_float_follows_go() {
    let vals = values();
    for row in &read_jsonl("goja-front/big/cmp_float.jsonl") {
        let x = &vals[i(row, "x") as usize];
        let f = f64_bits(s(row, "f"));
        assert_eq!(i64::from(cmp_float64(x, f)), i(row, "cmp"), "{x} vs {f}");
    }
}

#[test]
fn big_int_to_float_rounds_like_go() {
    for row in &read_jsonl("goja-front/big/to_float.jsonl") {
        let x = Int::set_string(s(row, "x").as_bytes(), 16).expect("hex value");
        let (f, a) = x.float64();
        assert_eq!(bits_f64(f), s(row, "f"), "{x}");
        assert_eq!(acc(a), i(row, "acc"), "{x}");
    }
}

#[test]
fn big_nan_and_invalid_bases_panic_like_go() {
    assert!(catch_unwind(|| float_is_int(f64::NAN)).is_err());
    assert!(catch_unwind(|| Int::new_int(1).text(1)).is_err());
    assert!(catch_unwind(|| Int::new_int(1).text(63)).is_err());
    assert!(catch_unwind(|| Int::set_string(b"1", 1)).is_err());
    assert_eq!(Int::set_string(b"", 1), None);
    assert!(catch_unwind(|| Int::new_int(1).quo(&Int::new())).is_err());
}
