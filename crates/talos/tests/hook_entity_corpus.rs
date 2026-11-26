mod common;
mod jsrun;
mod tzchild;

use gocompat::json;
use jsrun::{new_runtime, on_big_stack};
use talos::host::{DocMap, DocValue, json as host_json};
use talos::{Runtime, Value};
use tzchild::{Zone, run_in_zone};

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

pub fn eligible(row: &json::Value) -> bool {
    let script = common::s(row, "script");
    let label = common::s(row, "label");
    !NEEDS_HOOK.iter().any(|n| script.contains(n)) && !HOOK_LABELS.contains(&label)
}

pub fn run_case(rt: &mut Runtime, script: &str, input: &str) -> (String, String) {
    let v = json::unmarshal_value(input.as_bytes()).expect("input");
    let entity = host_json::from_json(&v);
    let mut ctx = DocMap::new();
    ctx.insert("entity".to_string(), entity.clone());
    let ctx = DocValue::new_map(ctx);
    let error = (|| -> String {
        let p = match talos::compile("", script.as_bytes(), false) {
            Ok(p) => p,
            Err(e) => return e.to_string(),
        };
        if let Err(e) = rt.set_value("ctx", &ctx) {
            let e = rt.throw_to_run_error(e);
            return e.error();
        }
        if let Err(e) = rt.run_program(&p) {
            return e.error();
        }
        let f = match rt.get_var("postRead") {
            Ok(Some(f)) => f,
            _ => return String::new(),
        };
        if talos::runtime::assert_callable(rt, &f).is_none() {
            return String::new();
        }
        let c = match rt.get_var("ctx") {
            Ok(Some(c)) => c,
            _ => Value::Undefined,
        };
        match rt.call_function(&f, Value::Undefined, &[c]) {
            Ok(_) => String::new(),
            Err(e) => e.error(),
        }
    })();
    let output = match host_json::marshal(&entity) {
        Ok(b) => String::from_utf8(b).expect("utf8"),
        Err(e) => format!("MARSHAL_ERROR: {e}"),
    };
    (error, output)
}

fn expected(row: &json::Value, arch: &[json::Value]) -> (String, String) {
    let label = common::s(row, "label");
    let row = arch
        .iter()
        .find(|r| common::s(r, "label") == label)
        .unwrap_or(row);
    if let Some(r) = row.get("expected_rust") {
        return (
            common::s(r, "error").to_string(),
            common::s(r, "output").to_string(),
        );
    }
    (
        common::s(row, "error").to_string(),
        common::s(row, "output").to_string(),
    )
}

pub fn outcomes(file: &str, arch_file: &str) -> Vec<(String, bool, String)> {
    let rows = common::read_jsonl(file);
    let arch = if cfg!(target_arch = "x86_64") {
        common::read_jsonl(arch_file)
    } else {
        Vec::new()
    };
    let mut out = Vec::new();
    for row in rows.iter().filter(|r| eligible(r)) {
        let label = common::s(row, "label").to_string();
        let script = common::s(row, "script").to_string();
        let input = common::s(row, "input").to_string();
        let want = expected(row, &arch);
        let got = std::thread::Builder::new()
            .stack_size(256 << 20)
            .spawn(move || {
                let mut rt = new_runtime();
                run_case(&mut rt, &script, &input)
            })
            .expect("spawn")
            .join();
        match got {
            Ok(got) => {
                let ok = got == want;
                let detail = format!(
                    "want error {:?} output {}\n got error {:?} output {}",
                    want.0, want.1, got.0, got.1
                );
                out.push((label, ok, detail));
            }
            Err(_) => out.push((label, false, "panicked".to_string())),
        }
    }
    out
}

fn replay(file: &str, arch_file: &str) {
    let results = outcomes(file, arch_file);
    assert_eq!(results.len(), 198, "{file}: eligible cases");
    let failures: Vec<String> = results
        .iter()
        .filter(|(_, ok, _)| !*ok)
        .map(|(label, _, detail)| format!("{label}:\n{detail}"))
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn entity_corpus_utc() {
    run_in_zone("entity_corpus_utc", Zone::Env("UTC"), || {
        on_big_stack(|| replay("hooks/corpus-utc.jsonl", "hooks/arch-amd64-utc.jsonl"))
    });
}

#[test]
fn entity_corpus_europe_paris() {
    run_in_zone(
        "entity_corpus_europe_paris",
        Zone::Env("Europe/Paris"),
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
