mod common;

use elysiandb::gocompat::json::{
    self, Base64Bytes, Decoder, Encode, EncodeState, Encoder, Error, ErrorKind, Expected, Kind,
    Map, PathSegment, RawMessage, Subject, SyntaxReason, Token, Value,
};
use elysiandb::json_struct;
use std::collections::BTreeMap;
use std::path::Path;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    struct SampleInner {
        a: String => "a",
        b: Option<Vec<i64>> => "b",
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    struct Sample {
        name: String => "name",
        count: i64 => "count",
        big: i64 => "big",
        small: u32 => "small",
        ratio: f64 => "ratio",
        flag: bool => "flag",
        opt: Option<bool> => "opt" [omitempty],
        tags: Option<Vec<String>> => "tags",
        meta: Option<Map> => "meta",
        inner: SampleInner => "inner",
        raw: RawMessage => "raw",
        any: Value => "any",
        skip: String => -,
        empty: String => "empty" [omitempty],
        no_tag: String => "NoTag",
        nested: Option<SampleInner> => "nested" [omitempty],
    }
}

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

fn str_of<'a>(v: &'a Value, key: &str) -> &'a str {
    v.get(key)
        .and_then(Value::as_str)
        .unwrap_or_else(|| panic!("missing string {key} in {v:?}"))
}

fn int_of(v: &Value, key: &str) -> i64 {
    v.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer {key} in {v:?}"))
}

fn bits(f: f64) -> String {
    format!("{:016x}", f.to_bits())
}

fn from_bits(s: &str) -> f64 {
    f64::from_bits(u64::from_str_radix(s, 16).expect("hex bits"))
}

fn text(b: &[u8]) -> String {
    String::from_utf8(b.to_vec()).expect("utf-8")
}

fn check_desc(v: &Value, desc: &Value, ctx: &str) {
    let tag = desc.index(0).and_then(Value::as_str).expect("desc tag");
    let arg = desc.index(1);
    match (tag, v) {
        ("null", Value::Null) => {}
        ("bool", Value::Bool(b)) => assert_eq!(Some(*b), arg.and_then(Value::as_bool), "{ctx}"),
        ("num", Value::Number(f)) => {
            assert_eq!(bits(*f), arg.and_then(Value::as_str).unwrap(), "{ctx}")
        }
        ("str", Value::String(s)) => {
            assert_eq!(
                common::hex(s.as_bytes()),
                arg.and_then(Value::as_str).unwrap(),
                "{ctx}"
            )
        }
        ("arr", Value::Array(a)) => {
            let items = arg.and_then(Value::as_array).expect("array desc");
            assert_eq!(a.len(), items.len(), "{ctx}");
            for (x, d) in a.iter().zip(items) {
                check_desc(x, d, ctx);
            }
        }
        ("obj", Value::Object(m)) => {
            let pairs = arg.and_then(Value::as_array).expect("object desc");
            assert_eq!(m.len(), pairs.len(), "{ctx}");
            for pair in pairs {
                let key = text(&common::unhex(
                    pair.index(0).and_then(Value::as_str).unwrap(),
                ));
                let got = m
                    .get(&key)
                    .unwrap_or_else(|| panic!("{ctx}: missing key {key:?}"));
                check_desc(got, pair.index(1).unwrap(), ctx);
            }
        }
        _ => panic!("{ctx}: {v:?} does not match {desc:?}"),
    }
}

fn depth(v: &Value) -> usize {
    let mut max = 0;
    let mut stack = vec![(v, 0usize)];
    while let Some((v, d)) = stack.pop() {
        match v {
            Value::Array(a) => {
                max = max.max(d + 1);
                stack.extend(a.iter().map(|x| (x, d + 1)));
            }
            Value::Object(m) => {
                max = max.max(d + 1);
                stack.extend(m.values().map(|x| (x, d + 1)));
            }
            _ => {}
        }
    }
    max
}

fn canonical_path(err: &Error) -> String {
    let path = match err.kind() {
        ErrorKind::Type { path, .. } | ErrorKind::Range { path, .. } => path,
        other => panic!("not a typed error: {other:?}"),
    };
    let keys: Vec<&str> = path
        .segments()
        .iter()
        .filter_map(|s| match s {
            PathSegment::Key(k) => Some(k.as_str()),
            PathSegment::Index(_) => None,
        })
        .collect();
    keys.join(".")
}

fn expected_for(type_kind: &str, field: &str) -> Expected {
    match type_kind {
        "int" | "int8" | "int16" | "int32" | "int64" | "uint" | "uint8" | "uint16" | "uint32"
        | "uint64" => Expected::Integer,
        "float32" | "float64" => Expected::Number,
        "string" => Expected::String,
        "bool" => Expected::Boolean,
        "struct" | "map" => Expected::Object,
        "slice" if field == "tags" => Expected::ArrayOfStrings,
        "slice" => Expected::Array,
        other => panic!("unexpected Go kind {other}"),
    }
}

fn check_type_error(go: &Value, err: &Error, ctx: &str) {
    let go_value = str_of(go, "value");
    let type_kind = str_of(go, "type_kind");
    let field = str_of(go, "field");
    assert_eq!(err.offset(), int_of(go, "offset"), "{ctx}: offset");
    let ours = canonical_path(err);
    assert!(
        field.is_empty() || ours == field || ours.starts_with(&format!("{field}.")),
        "{ctx}: Go field {field:?}, path {ours:?}"
    );
    match err.kind() {
        ErrorKind::Type { expected, got, .. } => {
            let want_got = match go_value {
                "string" => Kind::String,
                "bool" => Kind::Boolean,
                "object" => Kind::Object,
                "array" => Kind::Array,
                v if v.starts_with("number") => Kind::Number,
                other => panic!("{ctx}: Go value {other}"),
            };
            assert_eq!(*got, want_got, "{ctx}");
            assert_eq!(*expected, expected_for(type_kind, field), "{ctx}");
            if let Some(lit) = go_value.strip_prefix("number ") {
                assert!(lit.contains(['.', 'e', 'E']), "{ctx}: {lit}");
            }
        }
        ErrorKind::Range { literal, .. } => {
            assert_eq!(go_value, format!("number {literal}"), "{ctx}");
            assert!(
                matches!(
                    expected_for(type_kind, field),
                    Expected::Integer | Expected::Number
                ),
                "{ctx}"
            );
        }
        other => panic!("{ctx}: {other:?}"),
    }
}

fn check_error(go: &Value, err: &Error, ctx: &str) {
    match str_of(go, "kind") {
        "syntax" => {
            let se = match err.kind() {
                ErrorKind::Syntax(e) | ErrorKind::InvalidRaw(e) => e,
                other => panic!("{ctx}: expected a syntax error, got {other:?}"),
            };
            assert_eq!(se.offset, int_of(go, "offset"), "{ctx}: offset");
        }
        "type" => check_type_error(go, err, ctx),
        "eof" => assert!(matches!(err.kind(), ErrorKind::Eof), "{ctx}: {err:?}"),
        "unexpected_eof" => {
            assert!(
                matches!(err.kind(), ErrorKind::UnexpectedEof),
                "{ctx}: {err:?}"
            )
        }
        "unsupported_value" => assert!(
            matches!(err.kind(), ErrorKind::UnsupportedNumber(_)),
            "{ctx}: {err:?}"
        ),
        other => panic!("{ctx}: unexpected Go error kind {other}"),
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
fn decode_rows_match_go() {
    let rel = "gocompat/json/decode.jsonl";
    let rows = common::read_jsonl(rel);
    assert_eq!(rows.len(), common::lines(&common::read_golden(rel)).len());
    let mut checked = 0;
    for row in &rows {
        let input = common::unhex(str_of(row, "in"));
        let ctx = format!("{:?}", String::from_utf8_lossy(&input));
        let valid = row.get("valid").and_then(Value::as_bool).expect("valid");
        assert_eq!(json::valid(&input), valid, "{ctx}: validity");

        let mut v = Value::Null;
        let result = json::unmarshal(&input, &mut v);
        match (row.get("error"), &result) {
            (Some(go), Err(err)) => check_error(go, err, &ctx),
            (None, Ok(())) => {
                if let Some(d) = row.get("value_depth").and_then(Value::as_i64) {
                    assert_eq!(depth(&v) as i64, d, "{ctx}: depth");
                } else {
                    check_desc(&v, row.get("value").expect("value"), &ctx);
                }
            }
            (g, r) => panic!("{ctx}: Go {g:?}, Rust {r:?}"),
        }

        if let Some(want) = row.get("struct").and_then(Value::as_str) {
            let mut st = Sample::default();
            let result = json::unmarshal(&input, &mut st);
            match (row.get("struct_error"), &result) {
                (Some(go), Err(err)) => check_error(go, err, &format!("{ctx} struct")),
                (None, Ok(())) => {}
                (g, r) => panic!("{ctx}: struct Go {g:?}, Rust {r:?}"),
            }
            let got = json::marshal(&st).expect("struct encodes");
            assert_eq!(text(&got), want, "{ctx}: struct");
            assert!(st.skip.is_empty(), "{ctx}: skipped field");
        }

        let mut raw = RawMessage::default();
        let result = json::unmarshal(&input, &mut raw);
        match (row.get("raw_error"), &result) {
            (Some(go), Err(err)) => check_error(go, err, &format!("{ctx} raw")),
            (None, Ok(())) => assert_eq!(common::hex(&raw.0), str_of(row, "raw"), "{ctx}: raw"),
            (g, r) => panic!("{ctx}: raw Go {g:?}, Rust {r:?}"),
        }
        checked += 1;
    }
    assert_eq!(checked, 8716);
}

#[test]
fn decode_validity_and_syntax_offsets_match_go() {
    let rows = common::read_jsonl("gocompat/json/decode.jsonl");
    for row in &rows {
        let input = common::unhex(common::str_field(row, "in"));
        let checked = json::check_valid(&input);
        let go_error = row
            .get("raw_error")
            .filter(|e| e.get("kind").and_then(Value::as_str) == Some("syntax"));
        match (go_error, &checked) {
            (Some(e), Err(err)) => {
                assert_eq!(e.get("offset").and_then(Value::as_i64), Some(err.offset));
                assert_eq!(err.position(), err.offset);
            }
            (None, Ok(())) => {}
            (g, c) => panic!(
                "syntax result differs for {:?}: go {g:?}, rust {c:?}",
                String::from_utf8_lossy(&input)
            ),
        }
    }
}

enum Enc {
    V(Value),
    F32(f32),
    I64(i64),
    U64(u64),
    Text(Vec<u8>),
    Raw(RawMessage),
    B64(Option<Base64Bytes>),
    Strs(Option<Vec<String>>),
    Arr(Option<Vec<Value>>),
    Obj(Option<BTreeMap<Vec<u8>, Value>>),
    Sample(Box<Sample>),
    Inner(SampleInner),
}

impl Encode for Enc {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        match self {
            Enc::V(v) => v.encode(e),
            Enc::F32(f) => f.encode(e),
            Enc::I64(n) => n.encode(e),
            Enc::U64(n) => n.encode(e),
            Enc::Text(s) => s.encode(e),
            Enc::Raw(r) => r.encode(e),
            Enc::B64(b) => b.encode(e),
            Enc::Strs(s) => s.encode(e),
            Enc::Arr(a) => a.encode(e),
            Enc::Obj(o) => o.encode(e),
            Enc::Sample(s) => s.encode(e),
            Enc::Inner(s) => s.encode(e),
        }
    }
}

fn value_from_desc(desc: &Value) -> Value {
    let arg = desc.index(1);
    match desc.index(0).and_then(Value::as_str).expect("tag") {
        "null" => Value::Null,
        "bool" => Value::Bool(arg.and_then(Value::as_bool).unwrap()),
        "num" => Value::Number(from_bits(arg.and_then(Value::as_str).unwrap())),
        "str" => Value::String(text(&common::unhex(arg.and_then(Value::as_str).unwrap()))),
        "arr" => Value::Array(
            arg.and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(value_from_desc)
                .collect(),
        ),
        "obj" => Value::Object(
            arg.and_then(Value::as_array)
                .unwrap()
                .iter()
                .map(|p| {
                    let k = text(&common::unhex(p.index(0).and_then(Value::as_str).unwrap()));
                    (k, value_from_desc(p.index(1).unwrap()))
                })
                .collect(),
        ),
        other => panic!("nested {other}"),
    }
}

fn full_sample() -> Sample {
    Sample {
        name: "n<>&".to_string(),
        count: -3,
        big: i64::MAX,
        small: 7,
        ratio: 0.5,
        flag: true,
        opt: Some(true),
        tags: Some(vec!["x".to_string()]),
        meta: Some(Map::from([("k".to_string(), Value::Number(1.0))])),
        inner: SampleInner {
            a: "a".to_string(),
            b: Some(vec![1, 2]),
        },
        raw: RawMessage::new(b"[1]".to_vec()),
        any: Value::String("any".to_string()),
        skip: "skip".to_string(),
        empty: String::new(),
        no_tag: "notag".to_string(),
        nested: None,
    }
}

fn enc_from_desc(desc: &Value) -> Enc {
    let arg = desc.index(1);
    let s = arg.and_then(Value::as_str);
    match desc.index(0).and_then(Value::as_str).expect("tag") {
        "null" | "bool" | "num" => Enc::V(value_from_desc(desc)),
        "f32" => Enc::F32(from_bits(s.unwrap()) as f32),
        "int" | "i64" => Enc::I64(s.unwrap().parse().unwrap()),
        "u64" => Enc::U64(s.unwrap().parse().unwrap()),
        "str" => Enc::Text(common::unhex(s.unwrap())),
        "raw" => Enc::Raw(RawMessage(s.map(common::unhex).unwrap_or_default())),
        "bytes" => Enc::B64(s.map(|h| Base64Bytes(common::unhex(h)))),
        "strs" => Enc::Strs(arg.and_then(Value::as_array).map(|a| {
            a.iter()
                .map(|x| text(&common::unhex(x.as_str().unwrap())))
                .collect()
        })),
        "arr" => Enc::Arr(
            arg.and_then(Value::as_array)
                .map(|a| a.iter().map(value_from_desc).collect()),
        ),
        "obj" => Enc::Obj(arg.and_then(Value::as_array).map(|a| {
            a.iter()
                .map(|p| {
                    (
                        common::unhex(p.index(0).and_then(Value::as_str).unwrap()),
                        value_from_desc(p.index(1).unwrap()),
                    )
                })
                .collect()
        })),
        "struct" => {
            let json = s.unwrap();
            if json.contains("notag") {
                Enc::Sample(Box::new(full_sample()))
            } else if json.starts_with("{\"a\":\"p\"") {
                Enc::Inner(SampleInner {
                    a: "p".to_string(),
                    b: None,
                })
            } else {
                Enc::Sample(Box::default())
            }
        }
        other => panic!("unknown desc {other}"),
    }
}

fn golden_bytes(row: &Value, field: &str) -> Option<Vec<u8>> {
    if let Some(t) = row.get(field).and_then(Value::as_str) {
        return Some(t.as_bytes().to_vec());
    }
    row.get(&format!("{field}_hex"))
        .and_then(Value::as_str)
        .map(common::unhex)
}

fn encode_no_html<T: Encode + ?Sized>(v: &T) -> Result<Vec<u8>, Error> {
    let mut enc = Encoder::new(Vec::new());
    enc.set_escape_html(false);
    enc.encode(v)?;
    Ok(enc.into_inner())
}

#[test]
fn encode_rows_match_go() {
    let rel = "gocompat/json/encode.jsonl";
    let rows = common::read_jsonl(rel);
    assert_eq!(rows.len(), common::lines(&common::read_golden(rel)).len());
    let mut checked = 0;
    for row in &rows {
        let desc = row.get("value").expect("value");
        let ctx = format!("{desc:?}");
        let v = enc_from_desc(desc);
        match row.get("error") {
            Some(go) => {
                let err = json::marshal(&v).expect_err("Go fails");
                check_error(go, &err, &ctx);
                assert!(json::marshal_indent(&v, "", "  ").is_err(), "{ctx}");
                assert!(encode_no_html(&v).is_err(), "{ctx}");
                assert!(golden_bytes(row, "marshal").is_none(), "{ctx}");
            }
            None => {
                let marshal = json::marshal(&v).unwrap_or_else(|e| panic!("{ctx}: {e}"));
                assert_eq!(
                    Some(marshal),
                    golden_bytes(row, "marshal"),
                    "{ctx}: marshal"
                );
                let indent = json::marshal_indent(&v, "", "  ").expect("indent");
                assert_eq!(Some(indent), golden_bytes(row, "indent"), "{ctx}: indent");
                let prefixed = json::marshal_indent(&v, ">", "\t").expect("indent prefix");
                assert_eq!(
                    Some(prefixed),
                    golden_bytes(row, "indent_prefix"),
                    "{ctx}: indent prefix"
                );
                let no_html = encode_no_html(&v).expect("encoder");
                assert_eq!(
                    Some(no_html),
                    golden_bytes(row, "encode_no_html"),
                    "{ctx}: no HTML"
                );
            }
        }
        checked += 1;
    }
    assert_eq!(checked, 8780);
}

fn token_desc(t: &Token) -> Value {
    let pair = |tag: &str, v: Value| Value::Array(vec![Value::String(tag.to_string()), v]);
    match t {
        Token::Delim(c) => pair("delim", Value::String(c.to_string())),
        Token::Null => Value::Array(vec![Value::String("null".to_string())]),
        Token::Bool(b) => pair("bool", Value::Bool(*b)),
        Token::Number(f) => pair("num", Value::String(bits(*f))),
        Token::String(s) => pair("str", Value::String(common::hex(s.as_bytes()))),
    }
}

fn check_stream_values(input: &[u8], steps: &[Value], ctx: &str) {
    let mut d = Decoder::new(input);
    let mut n = 0;
    for step in steps {
        let mut v = Value::Null;
        let result = d.decode(&mut v);
        assert_eq!(
            d.input_offset(),
            int_of(step, "offset"),
            "{ctx}: offset #{n}"
        );
        match (step.get("error"), result) {
            (Some(go), Err(err)) => check_error(go, &err, ctx),
            (None, Ok(())) => check_desc(&v, step.get("value").unwrap(), ctx),
            (g, r) => panic!("{ctx}: step {n}: Go {g:?}, Rust {r:?}"),
        }
        n += 1;
    }
    assert!(n == 20 || steps.last().is_some_and(|s| s.get("error").is_some()));
}

fn check_stream_tokens(input: &[u8], steps: &[Value], ctx: &str) {
    let mut d = Decoder::new(input);
    for (n, step) in steps.iter().enumerate() {
        let more = d.more();
        let result = d.token();
        assert_eq!(
            Some(more),
            step.get("more").and_then(Value::as_bool),
            "{ctx}: more #{n}"
        );
        assert_eq!(
            d.input_offset(),
            int_of(step, "offset"),
            "{ctx}: offset #{n}"
        );
        match (step.get("error"), result) {
            (Some(go), Err(err)) => check_error(go, &err, ctx),
            (None, Ok(t)) => assert_eq!(&token_desc(&t), step.get("token").unwrap(), "{ctx}: #{n}"),
            (g, r) => panic!("{ctx}: token {n}: Go {g:?}, Rust {r:?}"),
        }
    }
}

fn check_object_walk(input: &[u8], walk: &Value, ctx: &str) {
    let mut d = Decoder::new(input);
    let first = d.token();
    let object = matches!(first, Ok(Token::Delim('{')));
    assert_eq!(
        walk.get("object").and_then(Value::as_bool),
        Some(object),
        "{ctx}"
    );
    if !object {
        return;
    }
    let keys = walk
        .get("keys")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let values = walk
        .get("values")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let mut n = 0;
    while d.more() {
        let t = d.token();
        let (key, is_string) = match &t {
            Ok(Token::String(s)) => (s.clone(), true),
            _ => (String::new(), false),
        };
        let want = &keys[n];
        assert_eq!(common::hex(key.as_bytes()), str_of(want, "key"), "{ctx}");
        assert_eq!(
            Some(is_string),
            want.get("string").and_then(Value::as_bool),
            "{ctx}"
        );
        assert_eq!(
            Some(t.is_err()),
            want.get("token_error").and_then(Value::as_bool),
            "{ctx}"
        );
        let mut raw = RawMessage::default();
        let r = d.decode(&mut raw);
        let want = &values[n];
        assert_eq!(common::hex(&raw.0), str_of(want, "raw"), "{ctx}");
        assert_eq!(
            Some(r.is_err()),
            want.get("error").and_then(Value::as_bool),
            "{ctx}"
        );
        n += 1;
        if n > 64 {
            break;
        }
    }
    assert_eq!(n, keys.len(), "{ctx}: key count");
}

#[test]
fn stream_rows_match_go() {
    let rel = "gocompat/json/stream.jsonl";
    let rows = common::read_jsonl(rel);
    assert_eq!(rows.len(), common::lines(&common::read_golden(rel)).len());
    let mut checked = 0;
    for row in &rows {
        let input = common::unhex(str_of(row, "in"));
        let ctx = format!("{:?}", String::from_utf8_lossy(&input));
        check_stream_values(
            &input,
            row.get("values").and_then(Value::as_array).unwrap(),
            &ctx,
        );
        check_stream_tokens(
            &input,
            row.get("tokens").and_then(Value::as_array).unwrap(),
            &ctx,
        );
        check_object_walk(&input, row.get("object_walk").unwrap(), &ctx);
        checked += 1;
    }
    assert_eq!(checked, 25);
}

#[test]
fn syntax_messages_follow_the_catalog() {
    let cases: [(&[u8], &str, &str); 9] = [
        (
            b"not json",
            "invalid JSON at byte 2: unexpected character 'o'",
            "byte 2: unexpected character 'o'",
        ),
        (
            b"",
            "invalid JSON: unexpected end of input",
            "unexpected end of input",
        ),
        (
            b"{\"entity\":\"b\",\"limit\":5,}",
            "invalid JSON at byte 25: unexpected character '}'",
            "byte 25: unexpected character '}'",
        ),
        (
            b"{\"entity\":\"b\"} x",
            "invalid JSON at byte 16: unexpected character 'x'",
            "byte 16: unexpected character 'x'",
        ),
        (
            b"{\"entity\":\"a\x01b\"}",
            "invalid JSON at byte 13: unexpected character '\\^A'",
            "byte 13: unexpected character '\\^A'",
        ),
        (
            b"\"\\x\"",
            "invalid JSON at byte 3: invalid escape sequence",
            "byte 3: invalid escape sequence",
        ),
        (
            b"-x",
            "invalid JSON at byte 2: invalid number",
            "byte 2: invalid number",
        ),
        (
            b"1e",
            "invalid JSON: unexpected end of input",
            "unexpected end of input",
        ),
        (
            b"[1,2",
            "invalid JSON: unexpected end of input",
            "unexpected end of input",
        ),
    ];
    for (input, want, bare) in cases {
        let err = json::check_valid(input).expect_err("invalid input");
        assert_eq!(err.to_string(), want);
        let mut v = Value::Null;
        let err = json::unmarshal(input, &mut v).expect_err("invalid input");
        assert_eq!(err.message(Subject::Body), want);
        assert_eq!(err.reason(Subject::Body), bare);
        assert_eq!(err.to_string(), want);
    }
    let deep = format!("{}{}", "[".repeat(10001), "]".repeat(10001));
    let err = json::check_valid(deep.as_bytes()).expect_err("too deep");
    assert_eq!(err.reason, SyntaxReason::NestingTooDeep);
    assert_eq!(
        err.to_string(),
        "invalid JSON at byte 10001: nesting too deep"
    );
    let err = json::check_valid(b"\"\xff\x01\"").expect_err("control byte");
    assert_eq!(
        err.to_string(),
        "invalid JSON at byte 3: unexpected character '\\^A'"
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
    assert_eq!(v.get("c"), Some(&Value::Number(-1500.0)));
    assert_eq!(v.as_object().map(|m| m.len()), Some(3));
    let lone = json::unmarshal_value(b"\"\\ud83dx\"").expect("valid");
    assert_eq!(lone.as_str(), Some("\u{FFFD}x"));
    let big = json::unmarshal_value(b"12345678901234567890").expect("valid");
    assert_eq!(
        json::marshal(&big).unwrap(),
        b"12345678901234567000".to_vec()
    );
    let n = json::unmarshal_value(b"[1.0,1e2,-0,0.000001,1e21]").expect("valid");
    assert_eq!(
        json::marshal(&n).unwrap(),
        b"[1,100,-0,0.000001,1e+21]".to_vec()
    );
    let exact = json::unmarshal_value_with_integers(b"[9223372036854775807,1.5,1e2]").unwrap();
    assert_eq!(exact.index(0), Some(&Value::Int(i64::MAX)));
    assert_eq!(exact.index(1), Some(&Value::Number(1.5)));
    assert_eq!(exact.index(2), Some(&Value::Number(100.0)));
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
        let mut s = String::from("x");
        json::append_quote_text(&mut s, &input);
        assert_eq!(format!("{s}\n"), format!("x{want}"));
        checked += 1;
    }
    assert_eq!(checked, 1716);
}

json_struct! {
    #[derive(Debug, Default)]
    struct QueryBody {
        entity: String => "entity",
        offset: i64 => "offset",
        limit: i64 => "limit",
        filters: Option<Map> => "filters",
        sorts: Option<BTreeMap<String, String>> => "sorts",
        count_only: bool => "countOnly",
        fields: String => "fields",
    }
}

fn query_error(body: &[u8]) -> String {
    let mut q = QueryBody::default();
    let err = json::unmarshal(body, &mut q).expect_err("rejected");
    let answer = BTreeMap::from([("error".to_string(), err.message(Subject::Body))]);
    text(&json::marshal(&answer).unwrap())
}

#[test]
fn query_api_errors_follow_the_catalog() {
    let cases: [(&[u8], &str); 19] = [
        (
            b"{\"entity\":\"b\",\"limit\":\"5\"}",
            r#"{"error":"invalid JSON: \"limit\" must be an integer, got a string"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"limit\":1.5}",
            r#"{"error":"invalid JSON: \"limit\" must be an integer, got a number"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"limit\":1e3}",
            r#"{"error":"invalid JSON: \"limit\" must be an integer, got a number"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"limit\":1.0}",
            r#"{"error":"invalid JSON: \"limit\" must be an integer, got a number"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"limit\":99999999999999999999}",
            r#"{"error":"invalid JSON: \"limit\": 99999999999999999999 is out of range"}"#,
        ),
        (
            b"{\"entity\":5}",
            r#"{"error":"invalid JSON: \"entity\" must be a string, got a number"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"countOnly\":\"true\"}",
            r#"{"error":"invalid JSON: \"countOnly\" must be a boolean, got a string"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"fields\":[\"a\"]}",
            r#"{"error":"invalid JSON: \"fields\" must be a string, got an array"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"sorts\":{\"a\":1}}",
            r#"{"error":"invalid JSON: \"sorts.a\" must be a string, got a number"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"sorts\":[]}",
            r#"{"error":"invalid JSON: \"sorts\" must be an object of strings, got an array"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"filters\":[]}",
            r#"{"error":"invalid JSON: \"filters\" must be an object, got an array"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"filters\":{\"x\":{\"eq\":1e400}}}",
            r#"{"error":"invalid JSON: \"filters.x.eq\": 1e400 is out of range"}"#,
        ),
        (
            b"{\"LIMIT\":\"x\"}",
            r#"{"error":"invalid JSON: \"LIMIT\" must be an integer, got a string"}"#,
        ),
        (
            b"[1]",
            r#"{"error":"invalid JSON: the body must be an object, got an array"}"#,
        ),
        (
            b"not json",
            r#"{"error":"invalid JSON at byte 2: unexpected character 'o'"}"#,
        ),
        (b"", r#"{"error":"invalid JSON: unexpected end of input"}"#),
        (
            b"{\"entity\":\"b\",\"limit\":5,}",
            r#"{"error":"invalid JSON at byte 25: unexpected character '}'"}"#,
        ),
        (
            b"{\"entity\":\"a\x01b\"}",
            r#"{"error":"invalid JSON at byte 13: unexpected character '\\^A'"}"#,
        ),
        (
            b"{\"entity\":\"b\",\"limit\":\"x\",\"offset\":\"y\"}",
            r#"{"error":"invalid JSON: \"limit\" must be an integer, got a string"}"#,
        ),
    ];
    for (body, want) in cases {
        assert_eq!(
            query_error(body),
            want,
            "{:?}",
            String::from_utf8_lossy(body)
        );
    }
    let mut q = QueryBody::default();
    json::unmarshal(
        b"{\"entity\":\"\xff\",\"LIMIT\":3,\"limit\":4,\"Limit\":5}",
        &mut q,
    )
    .expect("accepted");
    assert_eq!(q.entity, "\u{FFFD}");
    assert_eq!(q.limit, 5);
    json::unmarshal(b"null", &mut q).expect("null body");
    assert_eq!(q.limit, 5);
    assert_eq!(q.offset, 0);
    assert!(q.filters.is_none() && q.sorts.is_none() && !q.count_only && q.fields.is_empty());
}

json_struct! {
    #[derive(Debug, Default)]
    struct Slots {
        id: u64 => "id",
    }
}

json_struct! {
    #[derive(Debug, Default)]
    struct EntityEntry {
        name: String => "name",
        slots: Slots => "slots",
    }
}

json_struct! {
    #[derive(Debug, Default)]
    struct ManifestDoc {
        format: u32 => "format",
        lsn: u64 => "lsn",
        entities: Vec<EntityEntry> => "entities" [omitempty],
    }
}

fn manifest_error(input: &[u8]) -> Error {
    let mut m = ManifestDoc::default();
    json::unmarshal(input, &mut m).expect_err("rejected")
}

#[test]
fn paths_contexts_and_prefixes() {
    let err = manifest_error(b"{\"lsn\":1.0}");
    assert_eq!(
        format!("cannot read MANIFEST: {err}"),
        "cannot read MANIFEST: invalid JSON: \"lsn\" must be an integer, got a number"
    );
    let err = manifest_error(b"{\"lsn\":-1}");
    assert_eq!(err.to_string(), "invalid JSON: \"lsn\": -1 is out of range");
    let err = manifest_error(b"{\"format\":4294967296}");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"format\": 4294967296 is out of range"
    );
    let err = manifest_error(b"{\"entities\":[{\"name\":\"a\"},{\"SLOTS\":{\"Id\":\"x\"}}]}");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"entities[1].SLOTS.Id\" must be an integer, got a string"
    );
    assert_eq!(
        err.reason(Subject::Value),
        "\"entities[1].SLOTS.Id\" must be an integer, got a string"
    );
    let err = manifest_error(b"{\"entities\":[{\"slots\":{\"id\":1e400}}]}");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"entities[0].slots.id\": 1e400 is out of range"
    );
    let err = manifest_error(b"\"x\"");
    assert_eq!(
        err.message(Subject::Body),
        "invalid JSON: the body must be an object, got a string"
    );
    assert_eq!(
        err.message(Subject::Value),
        "invalid JSON: the value must be an object, got a string"
    );
    assert_eq!(
        err.message(Subject::File),
        "invalid JSON: the file must be an object, got a string"
    );
    assert_eq!(
        err.reason(Subject::File),
        "the file must be an object, got a string"
    );

    let mut legacy: BTreeMap<String, Base64Bytes> = BTreeMap::new();
    let err = json::unmarshal(b"{\"ok\":\"aGk=\",\"session:1\":\"abc!\"}", &mut legacy)
        .expect_err("bad base64");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"session:1\": invalid base64 at byte 3"
    );
    assert_eq!(
        err.reason(Subject::File),
        "\"session:1\": invalid base64 at byte 3"
    );
    assert_eq!(legacy.get("ok").map(|b| b.0.clone()), Some(b"hi".to_vec()));
    let err = json::unmarshal(b"[1]", &mut legacy).expect_err("not an object");
    assert_eq!(
        err.message(Subject::File),
        "invalid JSON: the file must be an object, got an array"
    );

    let mut list: Vec<Vec<String>> = Vec::new();
    let err = json::unmarshal(b"[[\"a\"],[\"b\",2],\"c\"]", &mut list).expect_err("mismatch");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"[1][1]\" must be a string, got a number"
    );
    assert_eq!(
        list,
        vec![
            vec!["a".to_string()],
            vec!["b".to_string(), String::new()],
            vec![]
        ]
    );
    let err = json::unmarshal(b"{\"a\":\"x\"}", &mut list).expect_err("mismatch");
    assert_eq!(
        err.to_string(),
        "invalid JSON: the value must be an array, got an object"
    );
    let mut strings: Vec<String> = Vec::new();
    let err = json::unmarshal(b"true", &mut strings).expect_err("mismatch");
    assert_eq!(
        err.to_string(),
        "invalid JSON: the value must be an array of strings, got a boolean"
    );
    let mut doc: Map = Map::new();
    let err = json::unmarshal(b"null", &mut strings).map(|_| 0);
    assert!(err.is_ok());
    let err = json::unmarshal(b"[1]", &mut doc).expect_err("not an object");
    assert_eq!(
        err.to_string(),
        "invalid JSON: the value must be an object, got an array"
    );
    let mut f: f64 = 2.0;
    let err = json::unmarshal(b"1e400", &mut f).expect_err("range");
    assert_eq!(err.to_string(), "invalid JSON: 1e400 is out of range");
    assert_eq!(f, 2.0);
    let mut small: f32 = 0.0;
    let err = json::unmarshal(b"1e39", &mut small).expect_err("range");
    assert_eq!(err.to_string(), "invalid JSON: 1e39 is out of range");
    let mut byte: u8 = 0;
    let err = json::unmarshal(b"256", &mut byte).expect_err("range");
    assert_eq!(err.reason(Subject::Value), "256 is out of range");
    let mut short: i32 = 0;
    let err = json::unmarshal(b"[2147483648]", &mut vec![short]).expect_err("range");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"[0]\": 2147483648 is out of range"
    );
    json::unmarshal(b"-2147483648", &mut short).expect("in range");
    assert_eq!(short, i32::MIN);
    let mut flag = true;
    let err = json::unmarshal(b"null", &mut flag).map(|_| flag);
    assert_eq!(err.ok(), Some(true));
    let err = json::unmarshal(b"{}", &mut flag).expect_err("mismatch");
    assert_eq!(
        err.to_string(),
        "invalid JSON: the value must be a boolean, got an object"
    );
}

json_struct! {
    #[derive(Debug, Default, PartialEq)]
    struct KvEntry {
        key: String => "key",
        value: String => "value",
        encoding: String => "encoding" [omitempty],
        expires_at: i64 => "expires_at" [omitempty],
    }
}

#[test]
fn stream_decoder_errors_and_options() {
    let input = b"{\"key\":\"a\",\"value\":\"1\"}\n{\"key\":\"b\",\"value\":\"2\",\"x\":1}\n";
    let mut d = Decoder::new(&input[..]);
    d.disallow_unknown_fields();
    let mut e = KvEntry::default();
    d.decode(&mut e).expect("first entry");
    assert_eq!(e.key, "a");
    let mut e = KvEntry::default();
    let err = d.decode(&mut e).expect_err("unknown field");
    assert_eq!(err.to_string(), "invalid JSON: unknown field \"x\"");
    assert_eq!(err.reason(Subject::Value), "unknown field \"x\"");
    assert_eq!(e.key, "b");
    let mut e = KvEntry::default();
    assert!(d.decode(&mut e).expect_err("end").is_eof());

    let mut d = Decoder::new(&b"{\"key\":1}{\"expires_at\":1e400}"[..]);
    let err = d.decode(&mut KvEntry::default()).expect_err("type");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"key\" must be a string, got a number"
    );
    let err = d.decode(&mut KvEntry::default()).expect_err("range");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"expires_at\": 1e400 is out of range"
    );

    let mut d = Decoder::new(&b"{\"sessions\":[]} trailing garbage"[..]);
    let mut v = Value::Null;
    d.decode(&mut v).expect("first value only");
    assert_eq!(d.buffered(), b" trailing garbage");
    let err =
        json::unmarshal(b"{\"sessions\":[]} trailing garbage", &mut v).expect_err("Unmarshal");
    assert_eq!(
        err.to_string(),
        "invalid JSON at byte 17: unexpected character 't'"
    );

    let mut d = Decoder::new(&b"{\"a\":1}\n{\"a\":"[..]);
    d.decode(&mut v).expect("first");
    let err = d.decode(&mut v).expect_err("truncated");
    assert!(matches!(err.kind(), ErrorKind::UnexpectedEof));
    assert_eq!(err.to_string(), "invalid JSON: unexpected end of input");
    assert_eq!(err.reason(Subject::Value), "unexpected end of input");
    let again = d.decode(&mut v).expect_err("sticky");
    assert!(matches!(again.kind(), ErrorKind::UnexpectedEof));

    let mut d = Decoder::new(&b"[1,2] x"[..]);
    d.decode(&mut v).expect("array");
    let err = d.decode(&mut v).expect_err("syntax");
    assert_eq!(
        err.to_string(),
        "invalid JSON at byte 7: unexpected character 'x'"
    );

    let mut d = Decoder::new(&b"[1 2]"[..]);
    assert_eq!(d.token().unwrap(), Token::Delim('['));
    assert_eq!(d.token().unwrap(), Token::Number(1.0));
    let err = d.token().expect_err("missing comma");
    assert_eq!(err.syntax().map(|e| e.offset), Some(3));
    assert_eq!(
        err.to_string(),
        "invalid JSON at byte 4: unexpected character '2'"
    );

    let mut d = Decoder::new(&b"{\"title\":\"asc\",\"year\":\"desc\",\"title\":\"desc\"}"[..]);
    assert_eq!(d.token().unwrap(), Token::Delim('{'));
    let mut keys = Vec::new();
    while d.more() {
        if let Ok(Token::String(k)) = d.token() {
            keys.push(k);
        }
        let mut raw = RawMessage::default();
        d.decode(&mut raw).expect("value");
    }
    assert_eq!(keys, ["title", "year", "title"]);

    let mut chunks = Decoder::new(ChunkReader(b"[\"abc\",{\"k\":[1,2,3]}] 7".to_vec(), 0));
    let mut v = Value::Null;
    chunks.decode(&mut v).expect("chunked value");
    assert_eq!(
        text(&json::marshal(&v).unwrap()),
        "[\"abc\",{\"k\":[1,2,3]}]"
    );
    chunks.decode(&mut v).expect("second value");
    assert_eq!(v, Value::Number(7.0));
    assert_eq!(chunks.input_offset(), 23);
}

struct ChunkReader(Vec<u8>, usize);

impl std::io::Read for ChunkReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.1 >= self.0.len() {
            return Ok(0);
        }
        let n = 3.min(buf.len()).min(self.0.len() - self.1);
        buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
        self.1 += n;
        Ok(n)
    }
}

json_struct! {
    #[derive(Debug, Default)]
    struct Stats {
        keys_count: u64 => "keys_count" [string],
        entities_count: i64 => "entities_count" [string],
        ratio: f64 => "ratio" [omitempty, string],
        label: String => "label" [string],
        flag: Option<bool> => "flag" [string],
    }
}

#[test]
fn encoding_rules_and_errors() {
    let s = Stats {
        keys_count: 18446744073709551615,
        entities_count: -2,
        ratio: 0.0,
        label: "a<b".to_string(),
        flag: Some(true),
    };
    assert_eq!(
        text(&json::marshal(&s).unwrap()),
        r#"{"keys_count":"18446744073709551615","entities_count":"-2","label":"\"a\\u003cb\"","flag":"true"}"#
    );
    let mut back = Stats::default();
    json::unmarshal(
        br#"{"keys_count":"7","entities_count":"-3","ratio":"1.5","label":"\"x\"","flag":null}"#,
        &mut back,
    )
    .expect("quoted fields");
    assert_eq!(back.keys_count, 7);
    assert_eq!(back.entities_count, -3);
    assert_eq!(back.ratio, 1.5);
    assert_eq!(back.label, "x");
    assert_eq!(back.flag, None);
    let err = json::unmarshal(br#"{"keys_count":7}"#, &mut back).expect_err("unquoted");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"keys_count\" must be a string holding an integer"
    );
    let err = json::unmarshal(br#"{"entities_count":"x"}"#, &mut back).expect_err("abort");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"entities_count\" must be a string holding an integer"
    );
    let err = json::unmarshal(br#"{"entities_count":"1.5"}"#, &mut back).expect_err("fraction");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"entities_count\" must be an integer, got a number"
    );

    for (f, name) in [
        (f64::NAN, "NaN"),
        (f64::INFINITY, "Infinity"),
        (f64::NEG_INFINITY, "-Infinity"),
    ] {
        let err = json::marshal(&Value::Number(f)).expect_err("unsupported");
        assert_eq!(
            err.to_string(),
            format!("cannot encode JSON: {name} is not a valid JSON number")
        );
        assert_eq!(
            err.reason(Subject::Value),
            format!("{name} is not a valid JSON number")
        );
    }
    let err = json::marshal(&RawMessage::new(b"{".to_vec())).expect_err("raw");
    assert_eq!(
        err.to_string(),
        "cannot encode JSON: a raw value is not valid JSON"
    );
    let err = json::marshal(&RawMessage::new(b"x".to_vec())).expect_err("raw");
    assert!(matches!(err.kind(), ErrorKind::InvalidRaw(_)));

    let doc = Value::Object(Map::from([
        ("z".to_string(), Value::Int(9007199254740993)),
        (
            "a".to_string(),
            Value::DocList(vec![
                Map::new(),
                Map::from([("k".to_string(), Value::Null)]),
            ]),
        ),
        (
            "\u{2028}<".to_string(),
            Value::String("\u{2029}&\x7f".to_string()),
        ),
    ]));
    assert_eq!(
        text(&json::marshal(&doc).unwrap()),
        "{\"a\":[{},{\"k\":null}],\"z\":9007199254740993,\"\\u2028\\u003c\":\"\\u2029\\u0026\x7f\"}"
    );
    let mut out = Vec::new();
    let mut enc = Encoder::new(&mut out);
    enc.set_escape_html(false);
    enc.set_indent("", " ");
    enc.encode(&doc).unwrap();
    enc.encode(&Vec::<u8>::from(&b"\xffok"[..])).unwrap();
    assert_eq!(
        text(&out),
        "{\n \"a\": [\n  {},\n  {\n   \"k\": null\n  }\n ],\n \"z\": 9007199254740993,\n \"\\u2028<\": \"\\u2029&\x7f\"\n}\n\"\\ufffdok\"\n"
    );

    let mut byte_keys: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    byte_keys.insert(b"\xff".to_vec(), b"v\xfe".to_vec());
    byte_keys.insert(b"b".to_vec(), Vec::new());
    assert_eq!(
        text(&json::marshal(&byte_keys).unwrap()),
        "{\"b\":\"\",\"\\ufffd\":\"v\\ufffd\"}"
    );
    let mut decoded: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    json::unmarshal(b"{\"k\":\"\xff\"}", &mut decoded).unwrap();
    assert_eq!(
        decoded.get(b"k".as_slice()),
        Some(&"\u{FFFD}".as_bytes().to_vec())
    );

    let nil: Option<Vec<String>> = None;
    let empty: Option<Vec<String>> = Some(Vec::new());
    assert_eq!(json::marshal(&nil).unwrap(), b"null");
    assert_eq!(json::marshal(&empty).unwrap(), b"[]");
    assert!(nil.is_empty_value() && empty.is_empty_value());
    assert!(!Some(String::new()).is_empty_value());
    assert!(!Some(false).is_empty_value());
    assert!(Value::Null.is_empty_value() && !Value::Bool(false).is_empty_value());
    assert!((-0.0f64).is_empty_value());
    let mut b64: Option<Base64Bytes> = None;
    json::unmarshal(b"\"\"", &mut b64).unwrap();
    assert_eq!(b64, Some(Base64Bytes(Vec::new())));
    json::unmarshal(b"\"YWJj\\r\\nZA==\"", &mut b64).unwrap();
    assert_eq!(b64, Some(Base64Bytes(b"abcd".to_vec())));
    json::unmarshal(b"[1,2]", &mut b64).unwrap();
    assert_eq!(b64, Some(Base64Bytes(vec![1, 2])));
    assert_eq!(json::marshal(&b64).unwrap(), b"\"AQI=\"");

    let mut out = Vec::new();
    json::indent(&mut out, b" { \"a\" : [ ] , \"b\":{}} ", ">", "-").unwrap();
    assert_eq!(text(&out), "{\n>-\"a\": [],\n>-\"b\": {}\n>} ");
    let mut out = Vec::new();
    json::compact(&mut out, b" { \"a<\" : [ 1 , 2 ] } ").unwrap();
    assert_eq!(text(&out), "{\"a<\":[1,2]}");
    assert!(json::compact(&mut out, b"{").is_err());
}

#[test]
fn decoding_merges_like_go() {
    let mut s = Sample {
        name: "keep".to_string(),
        tags: Some(vec!["old".to_string(), "older".to_string()]),
        meta: Some(Map::from([("old".to_string(), Value::Bool(true))])),
        inner: SampleInner {
            a: "x".to_string(),
            b: Some(vec![5, 6, 7]),
        },
        any: Value::Number(3.0),
        nested: Some(SampleInner {
            a: "n".to_string(),
            b: None,
        }),
        ..Sample::default()
    };
    json::unmarshal(
        br#"{"tags":["new"],"meta":{"k":1},"inner":{"b":[null,8]},"nested":{"b":[]},"name":null,"any":1e400}"#,
        &mut s,
    )
    .expect_err("out of range any");
    assert_eq!(s.name, "keep");
    assert_eq!(s.tags, Some(vec!["new".to_string()]));
    assert_eq!(
        s.meta,
        Some(Map::from([
            ("k".to_string(), Value::Number(1.0)),
            ("old".to_string(), Value::Bool(true)),
        ]))
    );
    assert_eq!(s.inner.a, "x");
    assert_eq!(s.inner.b, Some(vec![5, 8]));
    assert_eq!(
        s.nested,
        Some(SampleInner {
            a: "n".to_string(),
            b: Some(Vec::new()),
        })
    );
    assert_eq!(s.any, Value::Number(3.0));

    let mut arr = vec![Value::Bool(true)];
    let err = json::unmarshal(b"[1e400, {\"a\":[1e999]}]", &mut arr).expect_err("range");
    assert_eq!(
        err.to_string(),
        "invalid JSON: \"[0]\": 1e400 is out of range"
    );
    assert_eq!(arr[0], Value::Bool(true));
    assert_eq!(text(&json::marshal(&arr).unwrap()), "[true,{\"a\":[null]}]");

    let deep = format!("{}1{}", "[".repeat(10000), "]".repeat(10000));
    let v = json::unmarshal_value(deep.as_bytes()).expect("deep value");
    assert_eq!(depth(&v), 10000);
    assert_eq!(json::marshal(&v).unwrap(), deep.as_bytes());
    let mut st = Sample::default();
    json::unmarshal(deep.as_bytes(), &mut st).expect_err("not an object");
    drop(v);

    let mut st = Sample::default();
    json::unmarshal(
        br#"{"NoTag":"a","notag":"b","NOTAG":"c","nOtAg":"d"}"#,
        &mut st,
    )
    .unwrap();
    assert_eq!(st.no_tag, "d");
    json::unmarshal(
        "{\"\u{212A}\":1}".as_bytes(),
        &mut BTreeMap::<String, i64>::new(),
    )
    .unwrap();
    let mut k = KelvinOnly::default();
    json::unmarshal("{\"\u{212A}\":7}".as_bytes(), &mut k).unwrap();
    assert_eq!(k.k, 7);
    json::unmarshal(br#"{"-":"dash","skip":"s","Skip":"t"}"#, &mut st).unwrap();
    assert!(st.skip.is_empty());
}

json_struct! {
    #[derive(Debug, Default)]
    struct KelvinOnly {
        k: i64 => "k",
    }
}

#[test]
fn custom_decoders_see_raw_values() {
    #[derive(Debug, Default)]
    struct Wrapper {
        body: QueryBody,
        order: Vec<String>,
        raw: Vec<u8>,
    }

    impl json::Decode for Wrapper {
        const EXPECTED: Expected = Expected::Object;

        fn decode(&mut self, d: &mut json::DecodeState<'_>) -> Result<(), Error> {
            let raw = d.raw_value();
            self.raw = raw.to_vec();
            json::unmarshal(raw, &mut self.body)?;
            let mut keys = Decoder::new(raw);
            if keys.token().ok() == Some(Token::Delim('{')) {
                while keys.more() {
                    if let Ok(Token::String(k)) = keys.token() {
                        self.order.push(k);
                    }
                    let _ = keys.decode(&mut RawMessage::default());
                }
            }
            Ok(())
        }
    }

    let mut w = Wrapper::default();
    json::unmarshal(b" {\"limit\":2,\"entity\":\"b\"} ", &mut w).unwrap();
    assert_eq!(w.order, ["limit", "entity"]);
    assert_eq!(w.raw, b"{\"limit\":2,\"entity\":\"b\"}");
    assert_eq!(w.body.limit, 2);
    let err = json::unmarshal(b"[1]", &mut Wrapper::default()).expect_err("array body");
    assert_eq!(
        err.message(Subject::Body),
        "invalid JSON: the body must be an object, got an array"
    );
}

#[test]
fn deep_values_clone_compare_and_print() {
    let deep = format!(
        "{}{{\"k\":[1,\"s\",null]}}{}",
        "[".repeat(9998),
        "]".repeat(9998)
    );
    let v = json::unmarshal_value(deep.as_bytes()).expect("deep value");
    let c = v.clone();
    assert!(c == v);
    assert!(format!("{v:?}").len() > 20000);
    let mixed = Value::Object(Map::from([
        (
            "d".to_string(),
            Value::DocList(vec![
                Map::from([("x".to_string(), Value::Int(1))]),
                Map::new(),
            ]),
        ),
        (
            "a".to_string(),
            Value::Array(vec![Value::Number(1.5), Value::Bool(false)]),
        ),
    ]));
    let copy = mixed.clone();
    assert_eq!(copy, mixed);
    assert_eq!(
        format!("{copy:?}"),
        "Object({\"a\": Array([Number(1.5), Bool(false)]), \"d\": DocList([{\"x\": Int(1)}, {}])})"
    );
    assert_ne!(Value::Int(5), Value::Number(5.0));
    assert_ne!(
        Value::DocList(vec![Map::new()]),
        Value::Array(vec![Value::Object(Map::new())])
    );
    assert_ne!(copy, Value::Object(Map::new()));
}
