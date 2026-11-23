mod common;

use elysiandb::gocompat::flag::{FlagSet, ParseError, Value as FlagValue};
use elysiandb::gocompat::json::{Value, quote_text};
use elysiandb::gocompat::time::Duration;

const USAGE: &str = "Usage: test\n";

fn elysiandb_set() -> FlagSet {
    let mut fs = FlagSet::new(USAGE);
    fs.string("config", b"elysian.yaml");
    fs
}

fn bench_set() -> FlagSet {
    let mut fs = FlagSet::new(USAGE);
    fs.bool("pair", false)
        .string("addr", b"127.0.0.1:8088")
        .int("vus", 4)
        .duration("duration", Duration(20_000_000_000))
        .int("rps", 0)
        .int("keys", 20000)
        .float64("setratio", 0.10)
        .int("payload", 16)
        .bool("warmup", true)
        .bool("live", true)
        .string("auth", b"");
    fs
}

const ELYSIANDB_FLAGS: &[&str] = &["config"];

const BENCH_FLAGS: &[&str] = &[
    "pair", "addr", "vus", "duration", "rps", "keys", "setratio", "payload", "warmup", "live",
    "auth",
];

fn value_text(v: &FlagValue) -> String {
    match v {
        FlagValue::Bool(b) => b.to_string(),
        FlagValue::String(s) => common::hex(s),
        FlagValue::Int(i) => i.to_string(),
        FlagValue::Float64(f) => format!("{:016x}", f.to_bits()),
        FlagValue::Duration(d) => d.0.to_string(),
    }
}

fn hex_list(v: &Value) -> Vec<Vec<u8>> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|a| common::unhex(a.as_str().unwrap()))
        .collect()
}

fn kind_of(fs: &FlagSet, name: &str) -> &'static str {
    match fs.value(name).unwrap() {
        FlagValue::Bool(_) => "bool",
        FlagValue::String(_) => "string",
        FlagValue::Int(_) => "int",
        FlagValue::Float64(_) => "float64",
        FlagValue::Duration(_) => "duration",
    }
}

fn expected_reason(kind: &str, class: &str, value: &[u8]) -> String {
    let q = quote_text(value);
    match (kind, class) {
        (_, "range") => format!("{q} is out of range"),
        ("int", "syntax") => format!("{q} is not a valid integer"),
        ("float64", "syntax") => format!("{q} is not a valid number"),
        ("bool", "syntax") => format!("{q} is not a valid boolean"),
        ("duration", "syntax") => format!("{q} is not a valid duration"),
        _ => panic!("unexpected reason {kind} {class}"),
    }
}

#[test]
fn flag_parsing_matches_go() {
    let rel = "gocompat/flag/cases.jsonl";
    let rows = common::read_jsonl(rel);
    let mut checked = 0;
    for row in &rows {
        let set = common::str_field(row, "set");
        let (mut fs, names) = match set {
            "elysiandb" => (elysiandb_set(), ELYSIANDB_FLAGS),
            "bench" => (bench_set(), BENCH_FLAGS),
            _ => panic!("unknown set {set}"),
        };
        let args = hex_list(row.get("args").unwrap());
        let result = fs.parse(args.clone());
        let ctx = format!("{set} {args:?}");
        match row.get("error").and_then(Value::as_str) {
            None => {
                assert_eq!(result, Ok(()), "{ctx}");
                let values = row.get("values").unwrap().as_object().unwrap();
                assert_eq!(values.len(), names.len(), "{ctx}");
                for name in names {
                    let want = values.get(name).unwrap().as_str().unwrap();
                    assert_eq!(value_text(fs.value(name).unwrap()), want, "{ctx} -{name}");
                }
                assert_eq!(fs.args(), hex_list(row.get("rest").unwrap()), "{ctx}");
            }
            Some(class) => {
                let err = result.expect_err(&ctx);
                let field = |k: &str| common::unhex(common::str_field(row, k));
                match class {
                    "help" => assert_eq!(err, ParseError::Help, "{ctx}"),
                    "syntax" => assert_eq!(err, ParseError::BadSyntax(field("arg")), "{ctx}"),
                    "unknown" => assert_eq!(err, ParseError::Unknown(field("name")), "{ctx}"),
                    "missing" => {
                        assert_eq!(err, ParseError::MissingValue(field("name")), "{ctx}")
                    }
                    "invalid" => {
                        let name = field("name");
                        let value = field("value");
                        let kind = kind_of(&fs, std::str::from_utf8(&name).unwrap());
                        let reason =
                            expected_reason(kind, common::str_field(row, "reason"), &value);
                        assert_eq!(
                            err,
                            ParseError::InvalidValue {
                                name,
                                value,
                                reason
                            },
                            "{ctx}"
                        );
                    }
                    _ => panic!("unknown error class {class}"),
                }
                assert_eq!(err.exit_code(), if class == "help" { 0 } else { 2 });
            }
        }
        checked += 1;
    }
    assert_eq!(checked, common::lines(&common::read_golden(rel)).len());
    assert_eq!(checked, 147);
}

fn message(err: &ParseError) -> String {
    String::from_utf8(err.message().unwrap()).unwrap()
}

#[test]
fn error_messages_follow_the_catalog() {
    let mut fs = elysiandb_set();
    let err = fs.parse(["-zzz"]).unwrap_err();
    assert_eq!(message(&err), "unknown flag: -zzz");
    let err = fs.parse(["--zzz=1"]).unwrap_err();
    assert_eq!(message(&err), "unknown flag: -zzz");
    let err = fs.parse(["-config"]).unwrap_err();
    assert_eq!(message(&err), "missing value for flag: -config");
    let err = fs.parse(["---config"]).unwrap_err();
    assert_eq!(message(&err), "invalid flag syntax: ---config");
    let err = fs.parse(["-h"]).unwrap_err();
    assert_eq!(err.message(), None);

    let mut fs = bench_set();
    let cases: &[(&[&str], &str)] = &[
        (
            &["-vus", "abc"],
            "invalid value \"abc\" for -vus: \"abc\" is not a valid integer",
        ),
        (
            &["-vus", "9223372036854775808"],
            "invalid value \"9223372036854775808\" for -vus: \"9223372036854775808\" is out of range",
        ),
        (
            &["-setratio", "x"],
            "invalid value \"x\" for -setratio: \"x\" is not a valid number",
        ),
        (
            &["-setratio", "1e400"],
            "invalid value \"1e400\" for -setratio: \"1e400\" is out of range",
        ),
        (
            &["-duration", "10"],
            "invalid value \"10\" for -duration: \"10\" is not a valid duration",
        ),
        (
            &["-pair=maybe"],
            "invalid value \"maybe\" for -pair: \"maybe\" is not a valid boolean",
        ),
        (
            &["-auth", "x", "-vus", "<\u{1}>"],
            "invalid value \"<\\u0001>\" for -vus: \"<\\u0001>\" is not a valid integer",
        ),
    ];
    for (args, want) in cases {
        let err = fs.parse(args.iter().copied()).unwrap_err();
        assert_eq!(message(&err), *want);
        assert_eq!(err.exit_code(), 2);
        assert_eq!(err.to_string(), *want);
    }
}

#[test]
fn error_output_is_the_message_then_the_usage() {
    let fs = elysiandb_set();
    let mut out = Vec::new();
    fs.write_error(&ParseError::Unknown(b"x".to_vec()), &mut out)
        .unwrap();
    assert_eq!(out, b"unknown flag: -x\nUsage: test\n");
    let mut out = Vec::new();
    fs.write_error(&ParseError::Help, &mut out).unwrap();
    assert_eq!(out, b"Usage: test\n");
    let mut out = Vec::new();
    fs.write_error(&ParseError::Unknown(b"\xff".to_vec()), &mut out)
        .unwrap();
    assert_eq!(out, b"unknown flag: -\xff\nUsage: test\n");
}

#[test]
fn getters_return_the_parsed_values() {
    let mut fs = bench_set();
    fs.parse([
        "-pair",
        "-addr=h:1",
        "-vus",
        "0x10",
        "-duration",
        "1m30s",
        "-setratio",
        "0.5",
        "rest",
    ])
    .unwrap();
    assert!(fs.get_bool("pair"));
    assert!(fs.get_bool("live"));
    assert_eq!(fs.get_string("addr"), b"h:1");
    assert_eq!(fs.get_int("vus"), 16);
    assert_eq!(fs.get_int("keys"), 20000);
    assert_eq!(fs.get_duration("duration"), Duration(90_000_000_000));
    assert_eq!(fs.get_float64("setratio"), 0.5);
    assert_eq!(fs.args(), [b"rest".to_vec()]);
    assert_eq!(fs.usage(), USAGE);
    assert!(fs.value("nope").is_none());
}

#[test]
#[should_panic(expected = "flag defined twice: -config")]
fn defining_a_flag_twice_panics() {
    let mut fs = elysiandb_set();
    fs.string("config", b"");
}
