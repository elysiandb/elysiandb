mod common;

use elysiandb::gocompat::fmt::{Arg, sprint, sprintf, sprintln};
use elysiandb::gocompat::json::Value;
use std::collections::BTreeSet;

const VERBS: [&str; 25] = [
    "%v", "%s", "%d", "%q", "%x", "%X", "%g", "%+v", "%06d", "%016d", "%02X", "%-63s|", "%b",
    "% v", "% s", "%T", "%e", "%f", "%.2f", "%.3g", "%5.1f", "%t", "%c", "%U", "%#v",
];

enum Desc {
    Nil,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Float32(f32),
    Str(Vec<u8>),
    Bytes(Vec<u8>),
    Strs(Vec<Vec<u8>>),
    Arr(Vec<Desc>),
    Obj(Vec<(Vec<u8>, Desc)>),
    MapInt(Vec<(Vec<u8>, i64)>),
    MapStr(Vec<(Vec<u8>, Vec<u8>)>),
    Duration(i64, Vec<u8>),
    Error(Vec<u8>),
    StructAb(i64, Vec<u8>),
}

fn number(v: &Value) -> &str {
    v.as_str()
        .unwrap_or_else(|| panic!("expected a decimal string, got {v:?}"))
}

fn hex_bytes(v: &Value) -> Vec<u8> {
    common::unhex(v.as_str().expect("hex string"))
}

fn duration_string(d: i64) -> Vec<u8> {
    fn frac(buf: &mut Vec<u8>, v: u64, prec: usize) -> u64 {
        let mut v = v;
        let mut print = false;
        for _ in 0..prec {
            let digit = v % 10;
            print = print || digit != 0;
            if print {
                buf.push(digit as u8 + b'0');
            }
            v /= 10;
        }
        if print {
            buf.push(b'.');
        }
        v
    }
    fn int(buf: &mut Vec<u8>, v: u64) {
        if v == 0 {
            buf.push(b'0');
        }
        let mut v = v;
        while v > 0 {
            buf.push((v % 10) as u8 + b'0');
            v /= 10;
        }
    }
    const SECOND: u64 = 1_000_000_000;
    let mut rev = Vec::new();
    let neg = d < 0;
    let mut u = d.unsigned_abs();
    if u < SECOND {
        rev.push(b's');
        if u == 0 {
            return b"0s".to_vec();
        }
        let prec = if u < 1_000 {
            rev.push(b'n');
            0
        } else if u < 1_000_000 {
            rev.extend_from_slice(&[0xb5, 0xc2]);
            3
        } else {
            rev.push(b'm');
            6
        };
        u = frac(&mut rev, u, prec);
        int(&mut rev, u);
    } else {
        rev.push(b's');
        u = frac(&mut rev, u, 9);
        int(&mut rev, u % 60);
        u /= 60;
        if u > 0 {
            rev.push(b'm');
            int(&mut rev, u % 60);
            u /= 60;
            if u > 0 {
                rev.push(b'h');
                int(&mut rev, u);
            }
        }
    }
    if neg {
        rev.push(b'-');
    }
    rev.reverse();
    rev
}

fn parse_desc(v: &Value) -> Desc {
    let parts = v.as_array().expect("argument description");
    let tag = parts[0].as_str().expect("argument tag");
    match tag {
        "null" => Desc::Nil,
        "bool" => Desc::Bool(parts[1].as_bool().expect("bool")),
        "int" | "i64" | "i32" => Desc::Int(number(&parts[1]).parse().expect("signed")),
        "u64" | "u32" | "u8" => Desc::Uint(number(&parts[1]).parse().expect("unsigned")),
        "num" => Desc::Float(f64::from_bits(
            u64::from_str_radix(number(&parts[1]), 16).expect("float bits"),
        )),
        "f32" => Desc::Float32(f64::from_bits(
            u64::from_str_radix(number(&parts[1]), 16).expect("float bits"),
        ) as f32),
        "str" => Desc::Str(hex_bytes(&parts[1])),
        "bytes" => Desc::Bytes(if parts[1].is_null() {
            Vec::new()
        } else {
            hex_bytes(&parts[1])
        }),
        "strs" => Desc::Strs(match parts[1].as_array() {
            None => Vec::new(),
            Some(items) => items.iter().map(hex_bytes).collect(),
        }),
        "arr" => Desc::Arr(match parts[1].as_array() {
            None => Vec::new(),
            Some(items) => items.iter().map(parse_desc).collect(),
        }),
        "obj" => Desc::Obj(match parts[1].as_array() {
            None => Vec::new(),
            Some(pairs) => pairs
                .iter()
                .map(|p| {
                    let kv = p.as_array().expect("pair");
                    (hex_bytes(&kv[0]), parse_desc(&kv[1]))
                })
                .collect(),
        }),
        "map_int" => Desc::MapInt(
            parts[1]
                .as_array()
                .expect("pairs")
                .iter()
                .map(|p| {
                    let kv = p.as_array().expect("pair");
                    (hex_bytes(&kv[0]), number(&kv[1]).parse().expect("int"))
                })
                .collect(),
        ),
        "map_str" => Desc::MapStr(
            parts[1]
                .as_array()
                .expect("pairs")
                .iter()
                .map(|p| {
                    let kv = p.as_array().expect("pair");
                    (hex_bytes(&kv[0]), hex_bytes(&kv[1]))
                })
                .collect(),
        ),
        "duration" => {
            let d: i64 = number(&parts[1]).parse().expect("duration");
            Desc::Duration(d, duration_string(d))
        }
        "error" => Desc::Error(hex_bytes(&parts[1])),
        "struct_ab" => Desc::StructAb(
            number(&parts[1]).parse().expect("field A"),
            hex_bytes(&parts[2]),
        ),
        other => panic!("unknown argument tag {other}"),
    }
}

fn to_arg(d: &Desc) -> Arg<'_> {
    match d {
        Desc::Nil => Arg::Nil,
        Desc::Bool(b) => Arg::Bool(*b),
        Desc::Int(i) => Arg::Int(*i),
        Desc::Uint(u) => Arg::Uint(*u),
        Desc::Float(f) => Arg::Float(*f),
        Desc::Float32(f) => Arg::Float32(*f),
        Desc::Str(s) => Arg::Str(s),
        Desc::Bytes(b) => Arg::Bytes(b),
        Desc::Strs(items) => Arg::Slice(items.iter().map(|s| Arg::Str(s)).collect()),
        Desc::Arr(items) => Arg::Slice(items.iter().map(to_arg).collect()),
        Desc::Obj(pairs) => Arg::Map(
            pairs
                .iter()
                .rev()
                .map(|(k, v)| (Arg::Str(k), to_arg(v)))
                .collect(),
        ),
        Desc::MapInt(pairs) => Arg::Map(
            pairs
                .iter()
                .rev()
                .map(|(k, v)| (Arg::Str(k), Arg::Int(*v)))
                .collect(),
        ),
        Desc::MapStr(pairs) => Arg::Map(
            pairs
                .iter()
                .rev()
                .map(|(k, v)| (Arg::Str(k), Arg::Str(v)))
                .collect(),
        ),
        Desc::Duration(d, text) => Arg::Stringer(text, Box::new(Arg::Int(*d))),
        Desc::Error(text) => Arg::Error(text),
        Desc::StructAb(a, b) => Arg::Struct(vec![("A", Arg::Int(*a)), ("B", Arg::Str(b))]),
    }
}

fn string_set(items: &[&str]) -> BTreeSet<String> {
    items.iter().map(|s| s.to_string()).collect()
}

fn keys_of(v: &Value) -> BTreeSet<String> {
    v.as_object()
        .unwrap_or_else(|| panic!("expected an object, got {v:?}"))
        .keys()
        .cloned()
        .collect()
}

fn check_single(row: &Value) -> usize {
    assert_eq!(keys_of(row), string_set(&["arg", "out"]));
    let desc = parse_desc(row.get("arg").expect("arg"));
    let arg = to_arg(&desc);
    let out = row.get("out").expect("out");
    let mut expected_keys = BTreeSet::new();
    let mut compared = 0;
    for verb in VERBS {
        if verb == "%T" || verb == "%#v" {
            continue;
        }
        let got = sprintf(verb, std::slice::from_ref(&arg));
        if got.windows(2).any(|w| w == b"%!") {
            assert!(
                out.get(verb).is_none() && out.get(&format!("{verb}_hex")).is_none(),
                "{verb} of {arg:?}: Go printed a value, got {:?}",
                String::from_utf8_lossy(&got)
            );
            continue;
        }
        match std::str::from_utf8(&got) {
            Ok(text) => {
                let want = out
                    .get(verb)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{verb} of {arg:?}: Go had no output, got {text:?}"));
                assert_eq!(text, want, "{verb} of {arg:?}");
                expected_keys.insert(verb.to_string());
            }
            Err(_) => {
                let key = format!("{verb}_hex");
                let want = out
                    .get(&key)
                    .and_then(Value::as_str)
                    .unwrap_or_else(|| panic!("{key} of {arg:?}: Go had no output"));
                assert_eq!(common::hex(&got), want, "{key} of {arg:?}");
                expected_keys.insert(key);
            }
        }
        compared += 1;
    }
    assert_eq!(keys_of(out), expected_keys, "output keys of {arg:?}");
    compared
}

fn check_multi(row: &Value) {
    assert_eq!(
        keys_of(row),
        string_set(&["args", "format", "sprintf", "sprint", "sprintln"])
    );
    let format = common::str_field(row, "format");
    let descs: Vec<Desc> = row
        .get("args")
        .and_then(Value::as_array)
        .expect("args")
        .iter()
        .map(parse_desc)
        .collect();
    let args: Vec<Arg> = descs.iter().map(to_arg).collect();
    assert_eq!(
        common::hex(&sprintf(format, &args)),
        common::str_field(row, "sprintf"),
        "sprintf {format}"
    );
    assert_eq!(
        common::hex(&sprint(&args)),
        common::str_field(row, "sprint"),
        "sprint {format}"
    );
    assert_eq!(
        common::hex(&sprintln(&args)),
        common::str_field(row, "sprintln"),
        "sprintln {format}"
    );
}

#[test]
fn fmt_cases_match_go() {
    let rel = "gocompat/fmt/cases.jsonl";
    let rows = common::read_jsonl(rel);
    let data = common::read_golden(rel);
    let count = common::lines(&data).len();
    assert!(count > 0);
    let mut checked = 0;
    let mut single = 0;
    let mut multi = 0;
    let mut outputs = 0;
    for row in &rows {
        if row.get("arg").is_some() {
            outputs += check_single(row);
            single += 1;
        } else {
            check_multi(row);
            multi += 1;
        }
        checked += 1;
    }
    assert_eq!(checked, count);
    assert!(single > 0 && multi > 0 && outputs > single);
}

#[test]
fn printf_edge_cases_match_go() {
    let cases: Vec<(&str, Vec<Arg>, &str)> = vec![
        ("%d %d", vec![Arg::Int(1)], "1 %!d(MISSING)"),
        ("%d", vec![Arg::Int(1), Arg::Int(2)], "1%!(EXTRA 2)"),
        ("%[3]d", vec![Arg::Int(1)], "%!d(BADINDEX)"),
        ("%", vec![], "%!(NOVERB)"),
        ("100%%", vec![], "100%"),
        ("%.*f", vec![Arg::Int(2), Arg::Float(1.23456)], "1.23"),
        ("%*d", vec![Arg::Str(b"x"), Arg::Int(3)], "%!(BADWIDTH)3"),
        (
            "%-5s|%5s",
            vec![Arg::Str(b"ab"), Arg::Str(b"cd")],
            "ab   |   cd",
        ),
        ("%.2s", vec![Arg::Str("héllo".as_bytes())], "hé"),
        ("%+q", vec![Arg::Str("é".as_bytes())], "\"\\u00e9\""),
        ("%#q", vec![Arg::Str(b"a\"b")], "`a\"b`"),
        ("%#x", vec![Arg::Int(255)], "0xff"),
        ("%#o", vec![Arg::Int(8)], "010"),
        ("%O", vec![Arg::Int(8)], "0o10"),
        ("%x", vec![Arg::Str(b"hi")], "6869"),
        ("% x", vec![Arg::Bytes(b"hi")], "68 69"),
        ("%#U", vec![Arg::Int(0x263A)], "U+263A '\u{263A}'"),
        ("%08.3f", vec![Arg::Float(-1.23456)], "-001.235"),
        ("%+d", vec![Arg::Int(5)], "+5"),
        ("%t", vec![Arg::Int(1)], "%!t(1)"),
        ("%d", vec![Arg::Nil], "%!d(<nil>)"),
        (
            "%v",
            vec![Arg::Map(vec![
                (Arg::Int(2), Arg::Str(b"b")),
                (Arg::Int(-1), Arg::Str(b"a")),
            ])],
            "map[-1:a 2:b]",
        ),
        ("%s", vec![Arg::Error(b"boom")], "boom"),
        ("%d", vec![Arg::Error(b"boom")], "&{%!d(boom)}"),
    ];
    for (format, args, want) in &cases {
        assert_eq!(
            String::from_utf8_lossy(&sprintf(format, args)),
            *want,
            "{format}"
        );
    }
}
