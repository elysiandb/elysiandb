use std::path::Path;

use elysiandb::configuration::{Config, LoadError, load_config};
use elysiandb::gocompat::json::{self, Value};
use elysiandb::log::{self, Options};

use super::common::{self, str_field, unhex};
use super::tempdir::TempDir;
use super::{Capture, Restore, log_state};

const PARSE_LITERAL: &str = "cannot parse the configuration file: ";
const READ_LITERAL: &str = "cannot read the configuration file: ";
const WARNING: &str = "unknown configuration keys are ignored";

const TYPE_MESSAGES: [(&str, &str); 23] = [
    (
        "bool \"true\"",
        "line 2: \"stats.enabled\" must be a boolean, got \"true\"",
    ),
    (
        "bool 1",
        "line 2: \"stats.enabled\" must be a boolean, got \"1\"",
    ),
    (
        "bool 0",
        "line 2: \"stats.enabled\" must be a boolean, got \"0\"",
    ),
    (
        "bool !!str true",
        "line 2: \"stats.enabled\" must be a boolean, got \"true\"",
    ),
    (
        "int 99999999999999999999",
        "line 2: \"store.shards\": 99999999999999999999 is out of range",
    ),
    (
        "int 18446744073709551615",
        "line 2: \"store.shards\": 18446744073709551615 is out of range",
    ),
    (
        "int \"8\"",
        "line 2: \"store.shards\" must be an integer, got \"8\"",
    ),
    (
        "int abc",
        "line 2: \"store.shards\" must be an integer, got \"abc\"",
    ),
    (
        "int []",
        "line 2: \"store.shards\" must be an integer, got a sequence",
    ),
    (
        "int {}",
        "line 2: \"store.shards\" must be an integer, got a mapping",
    ),
    (
        "int .inf",
        "line 2: \"store.shards\" must be an integer, got \".inf\"",
    ),
    (
        "int .nan",
        "line 2: \"store.shards\" must be an integer, got \".nan\"",
    ),
    (
        "int64 \"7\"",
        "line 3: \"store.memory.limitMB\" must be an integer, got \"7\"",
    ),
    (
        "string [a, b]",
        "line 2: \"store.folder\" must be a string, got a sequence",
    ),
    (
        "list a",
        "line 3: \"server.http.trustedProxies\" must be an array of strings, got \"a\"",
    ),
    (
        "list [[a]]",
        "line 3: \"server.http.trustedProxies[0]\" must be a string, got a sequence",
    ),
    (
        "list {a: b}",
        "line 3: \"server.http.trustedProxies\" must be an array of strings, got a mapping",
    ),
    (
        "pointer bool maybe",
        "line 3: \"api.query.cancelOnDisconnect\" must be a boolean, got \"maybe\"",
    ),
    (
        "server scalar",
        "line 1: \"server\" must be a mapping, got \"3\"",
    ),
    (
        "server sequence",
        "line 2: \"server\" must be a mapping, got a sequence",
    ),
    (
        "port mapping",
        "line 4: \"server.http.port\" must be an integer, got a mapping",
    ),
    (
        "top sequence",
        "line 1: the file must be a mapping, got a sequence",
    ),
    (
        "top scalar",
        "line 1: the file must be a mapping, got \"hello\"",
    ),
];

const SYNTAX_MESSAGES: [(&str, &str); 8] = [
    ("unclosed flow", "did not find expected ',' or ']'"),
    ("unclosed quote", "line 3: found unexpected end of stream"),
    ("bad indentation", "line 2: did not find expected key"),
    (
        "tab indentation",
        "line 2: found character that cannot start any token",
    ),
    ("invalid escape", "line 2: found unknown escape character"),
    ("unknown anchor", "unknown anchor 'nope' referenced"),
    ("block in flow", "did not find expected node content"),
    ("trailing garbage", "line 1: did not find expected key"),
];

const UNLISTED_KEYS: [(&str, &str); 4] = [
    ("anchors and merge", "base"),
    ("merge then override", "base"),
    ("recursive anchor", "a"),
    ("colon in key", "store.fold:er"),
];

const D17_KEYS: [(&str, &str); 1] = [("unknown pprof", "monitoring.pprof")];

const PPROF: [&str; 2] = [
    ",\"Pprof\":{\"Enabled\":false}",
    ",\"Pprof\":{\"Enabled\":true}",
];

pub(super) fn without_pprof(go: &str) -> String {
    let found: Vec<&str> = PPROF.iter().copied().filter(|p| go.contains(p)).collect();
    assert_eq!(found.len(), 1, "one Pprof block in {go}");
    assert_eq!(go.matches(found[0]).count(), 1, "one Pprof block in {go}");
    go.replace(found[0], "")
}

fn lookup<'a>(table: &[(&str, &'a str)], used: &mut [bool], name: &str) -> Option<&'a str> {
    let i = table.iter().position(|(n, _)| *n == name)?;
    assert!(!used[i], "{name}: listed twice");
    used[i] = true;
    Some(table[i].1)
}

fn strings(row: &Value, key: &str) -> Option<String> {
    let items = row.get(key)?.as_array()?;
    let names: Vec<&str> = items
        .iter()
        .map(|v| v.as_str().expect("string item"))
        .collect();
    Some(names.join(","))
}

fn ops_utf16_input() -> Vec<u8> {
    let rows = common::read_jsonl("yaml/syntax.jsonl");
    let row = rows
        .iter()
        .find(|r| str_field(r, "name") == "ops utf16")
        .expect("the ops utf16 case of yaml/syntax.jsonl");
    unhex(str_field(row, "input_hex"))
}

fn warning_tail(path: &Path, keys: &str, duplicates: &str) -> String {
    let mut tail = format!(
        " level=WARN msg=\"{WARNING}\" component=boot path={}",
        path.display()
    );
    if !keys.is_empty() {
        tail.push_str(&format!(" keys={keys}"));
    }
    if !duplicates.is_empty() {
        tail.push_str(&format!(" duplicate_keys={duplicates}"));
    }
    tail.push('\n');
    tail
}

fn check_error(name: &str, row: &Value, err: &LoadError, tables: &mut Tables) {
    let want = row.get("error").expect("error");
    let class = str_field(want, "class");
    let message = err.to_string();
    match class {
        "validation" => {
            let LoadError::Invalid(msg) = err else {
                panic!("{name}: expected a validation error, got {err:?}");
            };
            assert_eq!(msg, str_field(want, "message"), "{name}");
            assert_eq!(message, str_field(want, "message"), "{name}");
        }
        "yaml-type" => {
            let LoadError::Parse(e) = err else {
                panic!("{name}: expected a YAML type error, got {err:?}");
            };
            assert_eq!(str_field(want, "literal"), PARSE_LITERAL, "{name}");
            let count = want.get("count").and_then(Value::as_i64).expect("count");
            assert_eq!(e.issues().len() as i64, count, "{name}");
            let lines: Vec<String> = e.issues().iter().map(|i| i.line.to_string()).collect();
            let golden: Vec<String> = want
                .get("lines")
                .and_then(Value::as_array)
                .expect("lines")
                .iter()
                .map(|v| v.as_str().expect("line").to_string())
                .collect();
            assert_eq!(lines, golden, "{name}");
            let key = str_field(want, "key");
            if !key.is_empty() {
                let path = e.issues()[0].dotted_path();
                assert!(
                    path == key || path.starts_with(&format!("{key}[")),
                    "{name}: {path} is not at {key}"
                );
            }
            let detail = lookup(&TYPE_MESSAGES, &mut tables.types, name)
                .unwrap_or_else(|| panic!("{name}: no D27 text listed"));
            assert_eq!(message, format!("{PARSE_LITERAL}{detail}"), "{name}");
        }
        "yaml-syntax" => {
            let LoadError::Parse(e) = err else {
                panic!("{name}: expected a YAML syntax error, got {err:?}");
            };
            assert!(e.issues().is_empty(), "{name}: {e:?}");
            assert_eq!(str_field(want, "literal"), PARSE_LITERAL, "{name}");
            let detail = lookup(&SYNTAX_MESSAGES, &mut tables.syntax, name)
                .unwrap_or_else(|| panic!("{name}: no D27 text listed"));
            match want.get("line").and_then(Value::as_str) {
                Some(line) => assert!(detail.starts_with(&format!("line {line}: ")), "{name}"),
                None => assert!(!detail.starts_with("line "), "{name}"),
            }
            assert_eq!(message, format!("{PARSE_LITERAL}{detail}"), "{name}");
        }
        other => panic!("{name}: unexpected class {other}"),
    }
}

struct Tables {
    types: [bool; TYPE_MESSAGES.len()],
    syntax: [bool; SYNTAX_MESSAGES.len()],
    unlisted: [bool; UNLISTED_KEYS.len()],
    d17: [bool; D17_KEYS.len()],
}

#[test]
fn configuration_cases_match_go() {
    let _state = log_state();
    let _restore = Restore;

    log::configure(&Options::default()).expect("configure");
    let capture = Capture::default();
    log::set_output(Some(Box::new(capture.clone())), false);

    let rows = common::read_jsonl("ops/yaml/cases.jsonl");
    assert_eq!(rows.len(), 134);

    let tmp = TempDir::new();
    let path = tmp.path().join("config.yaml");
    let mut tables = Tables {
        types: [false; TYPE_MESSAGES.len()],
        syntax: [false; SYNTAX_MESSAGES.len()],
        unlisted: [false; UNLISTED_KEYS.len()],
        d17: [false; D17_KEYS.len()],
    };
    let (mut ok_rows, mut error_rows, mut warned) = (0, 0, 0);
    let mut checked = 0;

    for row in &rows {
        let name = str_field(row, "name");

        if name == "missing file" {
            let absent = tmp.path().join("absent.yaml");
            let err = load_config(&absent).expect_err("missing file");
            let want = row.get("error").expect("error");
            assert_eq!(str_field(want, "class"), "os");
            assert_eq!(str_field(want, "literal"), READ_LITERAL);
            assert!(matches!(err, LoadError::Read(_)), "{err:?}");
            assert_eq!(
                err.to_string(),
                format!(
                    "{READ_LITERAL}open {}: No such file or directory (os error 2)",
                    absent.display()
                )
            );
            assert_eq!(capture.take(), "");
            error_rows += 1;
            checked += 1;
            continue;
        }

        let golden_input = str_field(row, "input");
        let input = if name == "utf16" {
            let bytes = ops_utf16_input();
            assert_eq!(String::from_utf8_lossy(&bytes), golden_input);
            bytes
        } else {
            golden_input.as_bytes().to_vec()
        };
        std::fs::write(&path, &input).expect("write the case");

        let result = load_config(&path);
        let out = capture.take();

        let ok = row.get("ok").and_then(Value::as_bool).expect("ok");
        match &result {
            Ok(cfg) => {
                assert!(ok, "{name}: Go rejected this file, Rust loaded {cfg:?}");
                let want = without_pprof(str_field(row, "config"));
                let got = json::marshal(cfg).expect("encode the configuration");
                assert_eq!(String::from_utf8(got).expect("UTF-8"), want, "{name}");
                ok_rows += 1;
            }
            Err(err) => {
                assert!(!ok, "{name}: Go loaded this file, Rust failed with {err}");
                check_error(name, row, err, &mut tables);
                error_rows += 1;
            }
        }

        let go_warned = row
            .get("warned_unknown_keys")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let d17 = lookup(&D17_KEYS, &mut tables.d17, name);
        if go_warned || d17.is_some() {
            let keys = match (strings(row, "unknown_keys"), d17) {
                (Some(keys), None) => keys,
                (None, Some(keys)) => keys.to_string(),
                (None, None) if row.get("duplicate_keys").is_some() => String::new(),
                (None, None) => lookup(&UNLISTED_KEYS, &mut tables.unlisted, name)
                    .unwrap_or_else(|| panic!("{name}: warned keys are not listed"))
                    .to_string(),
                (Some(_), Some(_)) => panic!("{name}: Go already listed keys"),
            };
            let duplicates = strings(row, "duplicate_keys").unwrap_or_default();
            let tail = warning_tail(&path, &keys, &duplicates);
            assert_eq!(out.matches('\n').count(), 1, "{name}: {out:?}");
            let head = out
                .strip_suffix(&tail)
                .unwrap_or_else(|| panic!("{name}: {out:?} does not end with {tail:?}"));
            assert!(
                head.starts_with("time=") && !head.contains(' '),
                "{name}: {out:?}"
            );
            warned += 1;
        } else {
            assert_eq!(out, "", "{name}: no warning expected");
        }

        checked += 1;
    }

    assert!(
        tables.types.iter().all(|u| *u),
        "every type message is used"
    );
    assert!(
        tables.syntax.iter().all(|u| *u),
        "every syntax message is used"
    );
    assert!(
        tables.unlisted.iter().all(|u| *u),
        "every listed key is used"
    );
    assert!(tables.d17.iter().all(|u| *u), "every D17 row is used");
    assert_eq!((ok_rows, error_rows, warned), (88, 46, 10));
    assert_eq!(checked, rows.len());
}

#[test]
fn a_lone_null_document_zeroes_the_configuration_like_go() {
    let _state = log_state();
    let _restore = Restore;

    log::configure(&Options::default()).expect("configure");
    let capture = Capture::default();
    log::set_output(Some(Box::new(capture.clone())), false);

    let tmp = TempDir::new();
    let path = tmp.path().join("config.yaml");
    std::fs::write(&path, "~\n").expect("write");
    let cfg = load_config(&path).expect("a lone ~ loads");
    assert_eq!(cfg, Config::default());
    assert_eq!(capture.take(), "");
}
