mod common;

use common::{f64_bits, i, read_golden, read_jsonl, s};
use gocompat::json::{self, Value};
use std::panic::{AssertUnwindSafe, catch_unwind};
use talos::ftoa::{self, FToStrMode, internal::fast};

const MAX_PRECISION: i64 = 101;

fn arm64_records(name: &str) -> Vec<(usize, Value)> {
    let manifest =
        json::unmarshal_value(&read_golden("talos-ftoa/MANIFEST.json")).expect("manifest");
    let file = format!("arch-arm64-{name}");
    let listed = manifest
        .get("files")
        .and_then(Value::as_array)
        .expect("files")
        .iter()
        .any(|f| f.get("path").and_then(Value::as_str) == Some(file.as_str()));
    if !listed {
        return Vec::new();
    }
    read_jsonl(&format!("talos-ftoa/{file}"))
        .into_iter()
        .map(|d| {
            assert_eq!(s(&d, "goarch"), "arm64");
            let record = d.get("record").cloned().expect("record");
            (i(&d, "line") as usize, record)
        })
        .collect()
}

fn rows(name: &str) -> Vec<Value> {
    let rel = format!("talos-ftoa/{name}");
    let lines = read_golden(&rel).iter().filter(|&&b| b == b'\n').count();
    let mut rows = read_jsonl(&rel);
    assert_eq!(rows.len(), lines, "{rel}");
    for (line, record) in arm64_records(name) {
        assert_eq!(s(&rows[line], "v"), s(&record, "v"), "{rel}:{line}");
        assert_eq!(s(&rows[line], "group"), s(&record, "group"), "{rel}:{line}");
        if cfg!(target_arch = "aarch64") {
            rows[line] = record;
        }
    }
    rows
}

fn f_to_str(v: f64, mode: FToStrMode, precision: i64) -> String {
    let mut buf = Vec::new();
    ftoa::f_to_str(v, mode, precision, &mut buf);
    String::from_utf8(buf).expect("ascii")
}

fn fnv(outputs: impl IntoIterator<Item = String>) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for out in outputs {
        for &b in out.as_bytes().iter().chain(std::iter::once(&0u8)) {
            h ^= u64::from(b);
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    format!("{h:016x}")
}

fn sweep(v: f64) -> String {
    let mut outs = vec![
        f_to_str(v, ftoa::MODE_STANDARD, 0),
        f_to_str(v, ftoa::MODE_STANDARD_EXPONENTIAL, 0),
    ];
    for mode in [
        ftoa::MODE_FIXED,
        ftoa::MODE_EXPONENTIAL,
        ftoa::MODE_PRECISION,
    ] {
        for p in 0..=MAX_PRECISION {
            outs.push(f_to_str(v, mode, p));
        }
    }
    fnv(outs)
}

fn entries(row: &Value, key: &str) -> Vec<(i64, Value)> {
    row.get(key)
        .and_then(Value::as_object)
        .unwrap_or_else(|| panic!("missing object {key} in {row:?}"))
        .iter()
        .map(|(k, v)| (k.parse().expect("precision"), v.clone()))
        .collect()
}

fn fast_call(v: f64, mode: fast::Mode, n: i64) -> Value {
    let mut buf = Vec::new();
    let (dec_pt, ok) = fast::dtoa(v, mode, n, &mut buf);
    Value::Array(vec![
        Value::String(String::from_utf8(buf).expect("ascii")),
        Value::Int(dec_pt),
        Value::Bool(ok),
    ])
}

fn big_call(v: f64, mode: i64, bias_up: bool, n: i64) -> Option<Value> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut buf = Vec::new();
        let dec_pt = ftoa::ftoa(v, mode, bias_up, n, &mut buf);
        Value::Array(vec![
            Value::String(String::from_utf8_lossy(&buf).into_owned()),
            Value::Int(dec_pt),
        ])
    }))
    .ok()
}

fn same(expected: &Value, actual: &Value) -> bool {
    match (expected, actual) {
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(x, y)| same(x, y))
        }
        (Value::String(a), Value::String(b)) => a == b,
        (Value::Bool(a), Value::Bool(b)) => a == b,
        (x, y) => match (x.as_i64(), y.as_i64()) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        },
    }
}

fn check_big(expected: &Value, v: f64, mode: i64, bias_up: bool, n: i64, ctx: &str) {
    let actual = big_call(v, mode, bias_up, n);
    if expected.get("panic").is_some() {
        assert!(actual.is_none(), "{ctx}: expected a panic, got {actual:?}");
        return;
    }
    let actual = actual.unwrap_or_else(|| panic!("{ctx}: unexpected panic"));
    assert!(same(expected, &actual), "{ctx}: {expected:?} != {actual:?}");
}

#[test]
fn ftostr_golden() {
    let rows = rows("ftostr.jsonl");
    let mut replayed = 0;
    let mut sweeps = 0;
    for row in &rows {
        let v = f64_bits(s(row, "v"));
        let ctx = format!("{} {}", s(row, "v"), s(row, "group"));
        assert_eq!(
            f_to_str(v, ftoa::MODE_STANDARD, 0),
            s(row, "std"),
            "{ctx} std"
        );
        assert_eq!(
            f_to_str(v, ftoa::MODE_STANDARD_EXPONENTIAL, 0),
            s(row, "stdexp"),
            "{ctx} stdexp"
        );
        for (key, mode) in [
            ("fixed", ftoa::MODE_FIXED),
            ("exp", ftoa::MODE_EXPONENTIAL),
            ("prec", ftoa::MODE_PRECISION),
        ] {
            for (p, out) in entries(row, key) {
                assert_eq!(
                    f_to_str(v, mode, p),
                    out.as_str().expect("output"),
                    "{ctx} {key} {p}"
                );
            }
        }
        let want = s(row, "sweep");
        if !want.is_empty() {
            assert_eq!(sweep(v), want, "{ctx} sweep");
            sweeps += 1;
        }
        replayed += 1;
    }
    assert_eq!(replayed, rows.len());
    assert!(sweeps > 0);
}

#[test]
fn ftobasestr_golden() {
    let rows = rows("ftobasestr.jsonl");
    let mut replayed = 0;
    for row in &rows {
        let v = f64_bits(s(row, "v"));
        for (radix, out) in entries(row, "base") {
            assert_eq!(
                ftoa::f_to_base_str(v, radix),
                out.as_str().expect("output"),
                "{} radix {radix}",
                s(row, "v")
            );
        }
        replayed += 1;
    }
    assert_eq!(replayed, rows.len());
}

#[test]
fn paths_golden() {
    let rows = rows("paths.jsonl");
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut replayed = 0;
    for row in &rows {
        let v = f64_bits(s(row, "v"));
        let ctx = s(row, "v");
        let fast_shortest = row.get("fast").expect("fast");
        let actual = fast_call(v, fast::MODE_SHORTEST, 0);
        assert!(
            same(fast_shortest, &actual),
            "{ctx} fast: {fast_shortest:?} != {actual:?}"
        );
        check_big(
            row.get("big").expect("big"),
            v,
            0,
            false,
            0,
            &format!("{ctx} big"),
        );
        for (n, expected) in entries(row, "fastp") {
            let actual = fast_call(v, fast::MODE_PRECISION, n);
            assert!(
                same(&expected, &actual),
                "{ctx} fastp {n}: {expected:?} != {actual:?}"
            );
        }
        for (n, expected) in entries(row, "bigp") {
            check_big(&expected, v, 2, true, n, &format!("{ctx} bigp {n}"));
        }
        let modes = row.get("modes").and_then(Value::as_object).expect("modes");
        for (key, expected) in modes.iter() {
            let parts: Vec<&str> = key.split(',').collect();
            let mode: i64 = parts[0].parse().expect("mode");
            let n: i64 = parts[1].parse().expect("ndigits");
            let bias_up = parts[2] == "true";
            check_big(expected, v, mode, bias_up, n, &format!("{ctx} mode {key}"));
        }
        replayed += 1;
    }
    std::panic::set_hook(prev);
    assert_eq!(replayed, rows.len());
}
