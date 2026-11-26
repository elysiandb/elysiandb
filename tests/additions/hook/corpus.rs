use std::path::PathBuf;
use std::process::Command;

use elysiandb::gocompat::json::{self, Value};
use elysiandb::hook;
use talos::host::{DocValue, json as host_json};

use super::{obj, s, setup_with, write};

const NEEDS_HOOK: [&str; 5] = [
    "ctx.query",
    "ctx.count",
    "ctx.sum",
    "ctx.group",
    "__elysian",
];

const HOOK_LABELS: [&str; 4] = [
    "timeout",
    "timeout-catch",
    "timeout-recursion",
    "fold-planned",
];

const BATCH_TWO: [&str; 6] = [
    "fn-names",
    "ctx-mutation",
    "filter-values",
    "fold-planned",
    "count-sum-types",
    "group-ops",
];

pub fn golden_path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/golden")
        .join(rel)
}

pub fn read_jsonl(rel: &str) -> Vec<Value> {
    let text = std::fs::read_to_string(golden_path(rel)).expect("golden file");
    text.lines()
        .filter(|l| !l.is_empty())
        .map(|l| json::unmarshal_value(l.as_bytes()).expect("golden line"))
        .collect()
}

pub fn field<'a>(row: &'a Value, key: &str) -> &'a str {
    match row.get(key) {
        Some(Value::String(s)) => s,
        _ => "",
    }
}

fn reserved(row: &Value) -> bool {
    let script = field(row, "script");
    let label = field(row, "label");
    NEEDS_HOOK.iter().any(|n| script.contains(n)) || HOOK_LABELS.contains(&label)
}

fn seed(label: &str) {
    if label.starts_with("reflect-") {
        for i in 0..2 {
            write(
                "order",
                obj(vec![("id", s(&format!("o{i}"))), ("group", s("g"))]),
            );
        }
        return;
    }

    if BATCH_TWO.contains(&label) || (label.starts_with("row-") && label != "row-keys") {
        for i in 0..3 {
            write(
                "order",
                obj(vec![
                    ("id", s(&format!("o{i}"))),
                    ("group", s("g")),
                    ("amount", Value::Number(i as f64 + 0.5)),
                ]),
            );
        }
        return;
    }

    for i in 0..3 {
        write(
            "order",
            obj(vec![
                ("id", s(&format!("o{i}"))),
                ("group", s("g")),
                ("amount", Value::Number(i as f64 + 0.5)),
                ("tags", Value::Array(vec![s("t")])),
                ("owner", Value::Object(obj(vec![("n", s("x"))]))),
            ]),
        );
    }
}

fn mask_interrupt(err: &str) -> String {
    let Some(start) = err.find("stopped after ") else {
        return err.to_string();
    };
    let rest = &err[start + "stopped after ".len()..];
    let digits = rest.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits == 0 || !rest[digits..].starts_with("ms at ") {
        return err.to_string();
    }
    let after_at = &rest[digits + "ms at ".len()..];
    let Some(space) = after_at.find(' ') else {
        return err.to_string();
    };
    if space == 0 {
        return err.to_string();
    }
    let pos = &after_at[space + 1..];
    if !pos.starts_with("(<eval>:") || !pos.ends_with("))") {
        return err.to_string();
    }
    let head_len = start + "stopped after ".len() + digits + "ms at ".len() + space + 1;
    format!("{}(<eval>:<position>)", &err[..head_len])
}

fn run_case(row: &Value) -> (String, String) {
    let label = field(row, "label").to_string();
    let script = field(row, "script").to_string();
    let input = field(row, "input").to_string();

    let _s = setup_with(|cfg| {
        cfg.store.shards = 4;
        if label.starts_with("timeout") {
            cfg.api.hooks.timeout_ms = 100;
        }
    });
    hook::init_hooks();
    seed(&label);

    let v = json::unmarshal_value(input.as_bytes()).expect("input");
    let entity = match &host_json::from_json(&v) {
        DocValue::Map(m) => m.clone(),
        _ => panic!("input is not an object"),
    };

    let error = match hook::apply_post_read_script("", &script, &entity, true) {
        Ok(()) => String::new(),
        Err(e) => mask_interrupt(e.message()),
    };
    let doc = DocValue::Map(entity);
    let output = match host_json::marshal(&doc) {
        Ok(b) => String::from_utf8(b).expect("utf-8 output"),
        Err(e) => format!("MARSHAL_ERROR: {e}"),
    };
    doc.release_graph();

    (error, output)
}

fn expected(row: &Value, arch: &[Value]) -> (String, String) {
    let label = field(row, "label");
    let row = arch
        .iter()
        .find(|r| field(r, "label") == label)
        .unwrap_or(row);
    if let Some(r) = row.get("expected_rust") {
        return (
            field(r, "error").to_string(),
            field(r, "output").to_string(),
        );
    }

    (
        field(row, "error").to_string(),
        field(row, "output").to_string(),
    )
}

fn replay(file: &str, arch_file: &str) {
    let rows = read_jsonl(file);
    let arch = if cfg!(target_arch = "x86_64") {
        read_jsonl(arch_file)
    } else {
        Vec::new()
    };

    let cases: Vec<&Value> = rows.iter().filter(|r| reserved(r)).collect();
    assert_eq!(cases.len(), 32, "{file}: cases reserved for the hook port");

    let mut failures = Vec::new();
    let mut passed = 0;
    for row in &cases {
        let label = field(row, "label");
        let want = expected(row, &arch);
        let got = run_case(row);
        if got == want {
            passed += 1;
            continue;
        }
        {
            failures.push(format!(
                "{label}:\n want error {:?} output {}\n  got error {:?} output {}",
                want.0, want.1, got.0, got.1
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(passed, cases.len(), "{file}: passing cases");
}

pub const ZONE_VAR: &str = "ELYSIANDB_HOOK_TZ_CHILD";

pub fn run_in_zone(test: &str, zone: &str, body: impl FnOnce()) {
    if let Ok(z) = std::env::var(ZONE_VAR) {
        if z != zone {
            return;
        }
        assert_eq!(std::env::var("TZ").as_deref(), Ok(zone));
        body();
        return;
    }

    let exe = std::env::current_exe().expect("current exe");
    let out = Command::new(exe)
        .args([
            "--exact",
            test,
            "--nocapture",
            "--include-ignored",
            "--test-threads=1",
        ])
        .env(ZONE_VAR, zone)
        .env("TZ", zone)
        .output()
        .expect("spawn the test binary");
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.status.success(), "{test} in {zone}:\n{text}");
    assert!(
        text.contains("1 passed"),
        "{test} did not run in {zone}:\n{text}"
    );
}

fn on_big_stack(f: impl FnOnce() + Send + 'static) {
    let h = std::thread::Builder::new()
        .stack_size(256 << 20)
        .spawn(f)
        .expect("spawn");
    if let Err(p) = h.join() {
        std::panic::resume_unwind(p);
    }
}

#[test]
fn hook_corpus_utc() {
    run_in_zone("hook::corpus::hook_corpus_utc", "UTC", || {
        on_big_stack(|| replay("hooks/corpus-utc.jsonl", "hooks/arch-amd64-utc.jsonl"))
    });
}

#[test]
fn hook_corpus_europe_paris() {
    run_in_zone(
        "hook::corpus::hook_corpus_europe_paris",
        "Europe/Paris",
        || {
            on_big_stack(|| {
                replay(
                    "hooks/corpus-europe-paris.jsonl",
                    "hooks/arch-amd64-europe-paris.jsonl",
                )
            })
        },
    );
}

fn go_lossy(b: &[u8]) -> String {
    let mut out = String::new();
    for chunk in b.utf8_chunks() {
        out.push_str(chunk.valid());
        for _ in chunk.invalid() {
            out.push('\u{fffd}');
        }
    }
    out
}

#[test]
fn hook_plan_bytecode_matches_the_baseline() {
    let scripts: std::collections::HashMap<String, String> = read_jsonl("hooks/corpus-utc.jsonl")
        .iter()
        .map(|r| {
            (
                field(r, "label").to_string(),
                field(r, "script").to_string(),
            )
        })
        .collect();

    let mut checked = 0;
    let mut planned = 0;
    let mut failures = Vec::new();
    let rows = read_jsonl("hooks/bytecode/plans.jsonl");
    for row in &rows {
        let label = field(row, "label");
        let script = scripts
            .get(label)
            .unwrap_or_else(|| panic!("no corpus script for {label}"));
        let info = hook::plan_info(script, "postRead");
        checked += 1;
        let want = matches!(row.get("planned"), Some(Value::Bool(true)));
        match (info, want) {
            (None, false) => {}
            (Some(_), false) => {
                failures.push(format!("{label}: planned, the baseline did not plan it"))
            }
            (None, true) => failures.push(format!("{label}: not planned")),
            (Some(info), true) => {
                planned += 1;
                let file = field(row, "file");
                let dump = reference_dump(
                    &std::fs::read_to_string(golden_path(&format!("hooks/{file}")))
                        .expect("plan dump"),
                );
                if info.dump != dump {
                    failures.push(format!("{label}: the plan dump differs"));
                }
                let folds = match row.get("folds") {
                    Some(Value::Number(n)) => *n as usize,
                    Some(Value::Int(n)) => *n as usize,
                    _ => 0,
                };
                if info.fold_ids.len() != folds {
                    failures.push(format!("{label}: {} folds", info.fold_ids.len()));
                }
                let ids: Vec<String> = match row.get("fold_ids") {
                    Some(Value::Array(a)) => a
                        .iter()
                        .map(|v| match v {
                            Value::String(s) => s.clone(),
                            _ => String::new(),
                        })
                        .collect(),
                    _ => Vec::new(),
                };
                let got: Vec<String> = info.fold_ids.iter().map(|b| go_lossy(b)).collect();
                if got != ids {
                    failures.push(format!("{label}: fold ids {got:?}, baseline {ids:?}"));
                }
                if !dump.contains(&format!(
                    "source:{}",
                    elysiandb::gocompat::strconv::quote(&info.source)
                )) {
                    failures.push(format!("{label}: the rewritten source differs"));
                }
            }
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(checked, rows.len());
    assert_eq!(checked, 230);
    assert_eq!(planned, 1);
}

pub fn reference_dump(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.split_inclusive('\n') {
        let Some(i) = line.find('\t') else {
            out.push_str(line);
            continue;
        };
        let (head, rest) = line.split_at(i + 1);
        let end = rest.find('\t').unwrap_or(rest.len());
        let (field, tail) = rest.split_at(end);
        let star = if field.starts_with('*') { "*" } else { "" };
        let body = &field[star.len()..];
        match body.find('.') {
            Some(d) if d > 0 && body[..d].bytes().all(|c| c.is_ascii_lowercase()) => {
                out.push_str(head);
                out.push_str(star);
                out.push_str("talos");
                out.push_str(&body[d..]);
                out.push_str(tail);
            }
            _ => out.push_str(line),
        }
    }
    out
}
