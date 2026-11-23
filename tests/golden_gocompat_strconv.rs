mod common;

use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strconv::{
    self, ErrorKind, NumError, Target, append_int, append_quote, atoi, format_bool, format_float,
    format_int, format_uint, itoa, parse_bool, parse_float, parse_int, parse_uint, quote,
    quote_to_ascii,
};
use std::collections::BTreeSet;

fn checked_rows(rel: &str) -> (Vec<Value>, usize) {
    let rows = common::read_jsonl(rel);
    let data = common::read_golden(rel);
    let count = common::lines(&data).len();
    assert!(count > 0, "{rel} is empty");
    (rows, count)
}

fn assert_keys(row: &Value, expected: &[&str]) {
    let keys: BTreeSet<&str> = row
        .as_object()
        .unwrap_or_else(|| panic!("row is not an object: {row:?}"))
        .keys()
        .map(String::as_str)
        .collect();
    let want: BTreeSet<&str> = expected.iter().copied().collect();
    assert_eq!(keys, want, "keys of {row:?}");
}

fn kind_name(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Syntax => "syntax",
        ErrorKind::Range => "range",
        ErrorKind::InvalidBase(_) | ErrorKind::InvalidBitSize(_) => "other",
    }
}

fn split<T: Copy>(r: &Result<T, NumError<T>>) -> (T, &'static str) {
    match r {
        Ok(v) => (*v, ""),
        Err(e) => (e.value, kind_name(e.kind)),
    }
}

fn check_message<T>(err: &NumError<T>, input: &[u8]) {
    let msg = err.to_string();
    assert!(msg.starts_with('"'), "message {msg:?}");
    let suffix = match (err.kind, err.target) {
        (ErrorKind::Range, _) => "\" is out of range",
        (ErrorKind::Syntax, Target::Number) => "\" is not a valid number",
        (ErrorKind::Syntax, Target::Integer) => "\" is not a valid integer",
        other => panic!("unexpected error {other:?} for {input:?}"),
    };
    assert!(msg.ends_with(suffix), "message {msg:?} for {input:?}");
    assert_eq!(err.num, input);
}

fn bits_hex(f: f64) -> String {
    format!("{:016x}", f.to_bits())
}

const FLOAT_FORMATS: [(u8, i32); 11] = [
    (b'f', -1),
    (b'g', -1),
    (b'e', -1),
    (b'f', 0),
    (b'f', 1),
    (b'f', 2),
    (b'f', 3),
    (b'f', 6),
    (b'e', 3),
    (b'g', 3),
    (b'g', 17),
];

#[test]
fn format_float_matches_go() {
    let (rows, count) = checked_rows("gocompat/strconv/format_float.jsonl");
    let keys: Vec<String> = FLOAT_FORMATS
        .iter()
        .map(|(fmt, prec)| format!("{}{prec}", char::from(*fmt)))
        .collect();
    let mut expected_keys: Vec<&str> = keys.iter().map(String::as_str).collect();
    expected_keys.push("bits");
    expected_keys.push("g-1_32");
    let mut checked = 0;
    for row in &rows {
        assert_keys(row, &expected_keys);
        let bits = u64::from_str_radix(common::str_field(row, "bits"), 16).expect("hex bits");
        let f = f64::from_bits(bits);
        for ((fmt, prec), key) in FLOAT_FORMATS.iter().zip(&keys) {
            assert_eq!(
                format_float(f, *fmt, *prec, 64),
                common::str_field(row, key),
                "{key} of {bits:016x}"
            );
        }
        assert_eq!(
            format_float(f64::from(f as f32), b'g', -1, 32),
            common::str_field(row, "g-1_32"),
            "g-1_32 of {bits:016x}"
        );
        checked += 1;
    }
    assert_eq!(checked, count);
}

#[test]
fn parse_float_matches_go() {
    let (rows, count) = checked_rows("gocompat/strconv/parse_float.jsonl");
    let mut checked = 0;
    let mut range_seen = 0;
    let mut syntax_seen = 0;
    for row in &rows {
        assert_keys(row, &["in", "bits", "err", "bits32", "err32"]);
        let input = common::unhex(common::str_field(row, "in"));
        for (bit_size, bits_key, err_key) in [(64, "bits", "err"), (32, "bits32", "err32")] {
            let r = parse_float(&input, bit_size);
            let (v, kind) = split(&r);
            assert_eq!(
                bits_hex(v),
                common::str_field(row, bits_key),
                "{bits_key} of {:?}",
                String::from_utf8_lossy(&input)
            );
            assert_eq!(
                kind,
                common::str_field(row, err_key),
                "{err_key} of {:?}",
                String::from_utf8_lossy(&input)
            );
            if let Err(e) = &r {
                assert_eq!(e.target, Target::Number);
                check_message(e, &input);
                match e.kind {
                    ErrorKind::Range => range_seen += 1,
                    _ => syntax_seen += 1,
                }
            }
        }
        checked += 1;
    }
    assert_eq!(checked, count);
    assert!(range_seen > 0 && syntax_seen > 0);
}

#[test]
fn parse_int_matches_go() {
    let (rows, count) = checked_rows("gocompat/strconv/parse_int.jsonl");
    let mut checked = 0;
    for row in &rows {
        assert_keys(
            row,
            &[
                "in",
                "int_10_64",
                "int_10_64_err",
                "uint_10_32",
                "uint_10_32_err",
                "uint_10_64",
                "uint_10_64_err",
                "atoi",
                "atoi_err",
                "int_0_64",
                "int_0_64_err",
            ],
        );
        let input = common::unhex(common::str_field(row, "in"));
        let shown = String::from_utf8_lossy(&input).into_owned();

        let signed = [
            ("int_10_64", parse_int(&input, 10, 64)),
            ("atoi", atoi(&input)),
            ("int_0_64", parse_int(&input, 0, 64)),
        ];
        for (key, r) in &signed {
            let (v, kind) = split(r);
            assert_eq!(
                v.to_string(),
                common::str_field(row, key),
                "{key} of {shown:?}"
            );
            let err_key = format!("{key}_err");
            assert_eq!(
                kind,
                common::str_field(row, &err_key),
                "{err_key} of {shown:?}"
            );
            if let Err(e) = r {
                assert_eq!(e.target, Target::Integer);
                check_message(e, &input);
            }
        }

        let unsigned = [
            ("uint_10_32", parse_uint(&input, 10, 32)),
            ("uint_10_64", parse_uint(&input, 10, 64)),
        ];
        for (key, r) in &unsigned {
            let (v, kind) = split(r);
            assert_eq!(
                v.to_string(),
                common::str_field(row, key),
                "{key} of {shown:?}"
            );
            let err_key = format!("{key}_err");
            assert_eq!(
                kind,
                common::str_field(row, &err_key),
                "{err_key} of {shown:?}"
            );
            if let Err(e) = r {
                assert_eq!(e.target, Target::Integer);
                check_message(e, &input);
            }
        }
        checked += 1;
    }
    assert_eq!(checked, count);
}

#[test]
fn format_int_matches_go() {
    let (rows, count) = checked_rows("gocompat/strconv/format_int.jsonl");
    let mut checked = 0;
    for row in &rows {
        assert_keys(
            row,
            &[
                "in", "b10", "b36", "b16", "b2", "u10", "u36", "itoa", "append", "bool",
            ],
        );
        let v: i64 = common::str_field(row, "in").parse().expect("decimal input");
        assert_eq!(format_int(v, 10), common::str_field(row, "b10"));
        assert_eq!(format_int(v, 36), common::str_field(row, "b36"));
        assert_eq!(format_int(v, 16), common::str_field(row, "b16"));
        assert_eq!(format_int(v, 2), common::str_field(row, "b2"));
        assert_eq!(format_uint(v as u64, 10), common::str_field(row, "u10"));
        assert_eq!(format_uint(v as u64, 36), common::str_field(row, "u36"));
        assert_eq!(itoa(v), common::str_field(row, "itoa"));
        let mut appended = b"x".to_vec();
        append_int(&mut appended, v, 10);
        assert_eq!(appended, common::str_field(row, "append").as_bytes());
        assert_eq!(format_bool(v != 0), common::str_field(row, "bool"));
        checked += 1;
    }
    assert_eq!(checked, count);
}

#[test]
fn quote_matches_go() {
    let (rows, count) = checked_rows("gocompat/strconv/quote.jsonl");
    let mut checked = 0;
    for row in &rows {
        assert_keys(row, &["in", "quote", "append", "ascii"]);
        let input = common::unhex(common::str_field(row, "in"));
        let q = quote(&input);
        assert_eq!(q, common::str_field(row, "quote"), "quote of {input:?}");
        let mut appended = b"x".to_vec();
        append_quote(&mut appended, &input);
        assert_eq!(
            appended,
            common::str_field(row, "append").as_bytes(),
            "append of {input:?}"
        );
        assert_eq!(
            quote_to_ascii(&input),
            common::str_field(row, "ascii"),
            "ascii of {input:?}"
        );
        assert_eq!(
            strconv::unquote(q.as_bytes()).as_deref(),
            Some(input.as_slice()),
            "unquote of {q}"
        );
        checked += 1;
    }
    assert_eq!(checked, count);
}

fn describe<T>(e: &NumError<T>) -> (String, ErrorKind, Target, Vec<u8>) {
    (e.to_string(), e.kind, e.target, e.num.clone())
}

#[test]
fn error_messages_follow_the_catalog() {
    let float_range = parse_float(b"1e400", 64).unwrap_err();
    assert_eq!(float_range.value, f64::INFINITY);
    let float_range32 = parse_float(b"-1e400", 32).unwrap_err();
    assert_eq!(float_range32.value, f64::NEG_INFINITY);
    let uint_range = parse_uint(b"18446744073709551616", 10, 64).unwrap_err();
    assert_eq!(uint_range.value, u64::MAX);
    let int_range = parse_int(b"-9223372036854775809", 10, 64).unwrap_err();
    assert_eq!(int_range.value, i64::MIN);
    let escaped: &[u8] = b"a<b>&\xff\xe2\x80\xa8\"\\\n\x01";
    let cases = [
        describe(&float_range),
        describe(&float_range32),
        describe(&parse_float(b"x", 64).unwrap_err()),
        describe(&parse_int(b"abc", 10, 64).unwrap_err()),
        describe(&atoi(b"abc").unwrap_err()),
        describe(&uint_range),
        describe(&int_range),
        describe(&parse_int(b"1", 1, 64).unwrap_err()),
        describe(&parse_int(b"1", 10, 65).unwrap_err()),
        describe(&parse_bool(b"yes").unwrap_err()),
        describe(&parse_float(escaped, 64).unwrap_err()),
    ];
    let expected: [(&str, ErrorKind, Target, &[u8]); 11] = [
        (
            "\"1e400\" is out of range",
            ErrorKind::Range,
            Target::Number,
            b"1e400",
        ),
        (
            "\"-1e400\" is out of range",
            ErrorKind::Range,
            Target::Number,
            b"-1e400",
        ),
        (
            "\"x\" is not a valid number",
            ErrorKind::Syntax,
            Target::Number,
            b"x",
        ),
        (
            "\"abc\" is not a valid integer",
            ErrorKind::Syntax,
            Target::Integer,
            b"abc",
        ),
        (
            "\"abc\" is not a valid integer",
            ErrorKind::Syntax,
            Target::Integer,
            b"abc",
        ),
        (
            "\"18446744073709551616\" is out of range",
            ErrorKind::Range,
            Target::Integer,
            b"18446744073709551616",
        ),
        (
            "\"-9223372036854775809\" is out of range",
            ErrorKind::Range,
            Target::Integer,
            b"-9223372036854775809",
        ),
        (
            "\"1\" cannot be read in base 1",
            ErrorKind::InvalidBase(1),
            Target::Integer,
            b"1",
        ),
        (
            "\"1\" cannot be read with a bit size of 65",
            ErrorKind::InvalidBitSize(65),
            Target::Integer,
            b"1",
        ),
        (
            "\"yes\" is not a valid boolean",
            ErrorKind::Syntax,
            Target::Boolean,
            b"yes",
        ),
        (
            "\"a<b>&\\ufffd\\u2028\\\"\\\\\\n\\u0001\" is not a valid number",
            ErrorKind::Syntax,
            Target::Number,
            escaped,
        ),
    ];
    for ((msg, kind, target, num), (want, want_kind, want_target, want_num)) in
        cases.iter().zip(&expected)
    {
        assert_eq!(msg, want);
        assert_eq!(kind, want_kind);
        assert_eq!(target, want_target);
        assert_eq!(num.as_slice(), *want_num);
        assert!(
            !msg.contains("strconv") && !msg.contains("parsing"),
            "{msg}"
        );
    }
}

#[test]
fn other_formats_match_go() {
    let f = f64::from_bits(0x3FF0000000000001);
    assert_eq!(format_float(f, b'b', -1, 64), "4503599627370497p-52");
    assert_eq!(format_float(f, b'x', -1, 64), "0x1.0000000000001p+00");
    assert_eq!(format_float(f, b'X', -1, 64), "0X1.0000000000001P+00");
    assert_eq!(format_float(f, b'E', -1, 64), "1.0000000000000002E+00");
    assert_eq!(format_float(1e21, b'G', -1, 64), "1E+21");
    assert_eq!(format_float(0.1, b'x', 3, 32), "0x1.99ap-04");
    assert_eq!(format_float(0.0, b'x', -1, 64), "0x0p+00");
    assert_eq!(format_float(5e-324, b'x', -1, 64), "0x1p-1074");
    assert_eq!(format_float(1.0, b'z', 2, 64), "%z");
    assert_eq!(strconv::parse_float(b"0x1.8p1", 64), Ok(3.0));
    assert_eq!(strconv::parse_float(b"1_000.5", 64), Ok(1000.5));
    assert_eq!(strconv::quote_rune(0x263A), "'\u{263A}'");
    assert_eq!(strconv::quote_rune_to_ascii(0x263A), "'\\u263a'");
    assert_eq!(strconv::quote_rune(-1), "'\u{FFFD}'");
    assert!(strconv::can_backquote(b"a\tb"));
    assert!(!strconv::can_backquote(b"a`b"));
    assert_eq!(strconv::unquote(b"`a\rb`").as_deref(), Some(&b"ab"[..]));
    assert_eq!(strconv::unquote(b"'\\x41'").as_deref(), Some(&b"A"[..]));
    assert_eq!(
        strconv::unquote(b"\"\\101\\u00e9\"").as_deref(),
        Some(&b"A\xc3\xa9"[..])
    );
    assert_eq!(strconv::unquote(b"\"a\nb\""), None);
    assert_eq!(strconv::parse_bool(b"True"), Ok(true));
}
