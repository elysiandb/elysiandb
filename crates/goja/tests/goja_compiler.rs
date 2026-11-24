mod common;

use common::{b, golden_path, read_golden, read_jsonl, s};
use gocompat::strconv::{quoted_prefix, unquote};
use std::collections::HashMap;

fn dump(src: &[u8]) -> Result<String, String> {
    match goja::compile("", src, false) {
        Ok(p) => Ok(goja::gojadump::program(&p)),
        Err(e) => Err(e.to_string()),
    }
}

fn first_difference(got: &str, want: &str) -> String {
    let g: Vec<&str> = got.lines().collect();
    let w: Vec<&str> = want.lines().collect();
    for i in 0..g.len().max(w.len()) {
        let gl = g.get(i).copied().unwrap_or("<eof>");
        let wl = w.get(i).copied().unwrap_or("<eof>");
        if gl != wl {
            return format!("line {}:\n    rust {gl}\n    go   {wl}", i + 1);
        }
    }
    "identical lines, different bytes".to_string()
}

fn top_level_source(dump: &str) -> Vec<u8> {
    let line = dump
        .lines()
        .find(|l| l.starts_with("0\t*goja.newFunc\t"))
        .unwrap_or_else(|| panic!("no top-level newFunc in dump"));
    let start = line.find("source:").expect("source field") + "source:".len();
    let quoted = quoted_prefix(&line.as_bytes()[start..]).expect("quoted source");
    unquote(&quoted).expect("valid quoted source")
}

fn corpus_scripts() -> HashMap<String, Vec<u8>> {
    read_jsonl("hooks/corpus-utc.jsonl")
        .iter()
        .map(|row| {
            (
                s(row, "label").to_string(),
                s(row, "script").as_bytes().to_vec(),
            )
        })
        .collect()
}

fn code_files_on_disk() -> usize {
    std::fs::read_dir(golden_path("hooks/bytecode"))
        .expect("bytecode directory")
        .filter_map(Result::ok)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".code"))
        .count()
}

#[test]
fn bytecode_dumps_match_goja() {
    let scripts = corpus_scripts();
    let mut checked = 0usize;
    let mut not_compiled = Vec::new();
    let mut failures = Vec::new();

    for row in read_jsonl("hooks/bytecode/index.jsonl") {
        let label = s(&row, "label");
        let src = scripts
            .get(label)
            .unwrap_or_else(|| panic!("no corpus script for {label}"));
        let compiled = b(&row, "compiled");
        match (dump(src), compiled) {
            (Ok(got), true) => {
                let file = s(&row, "file");
                let want =
                    String::from_utf8(read_golden(&format!("hooks/{file}"))).expect("utf-8 dump");
                checked += 1;
                if got != want {
                    failures.push(format!("{label}: {}", first_difference(&got, &want)));
                }
            }
            (Err(_), false) => not_compiled.push(label.to_string()),
            (Ok(_), false) => failures.push(format!("{label}: compiled, goja refused it")),
            (Err(e), true) => failures.push(format!("{label}: not compiled: {e}")),
        }
    }

    for row in read_jsonl("hooks/bytecode/plans.jsonl") {
        if !b(&row, "planned") {
            continue;
        }
        let label = s(&row, "label");
        let original = String::from_utf8(read_golden(&format!("hooks/bytecode/{label}.code")))
            .expect("utf-8 dump");
        assert_eq!(
            &top_level_source(&original),
            &scripts[label],
            "{label}: the script is not exactly its top-level function"
        );
        let file = s(&row, "file");
        let want = String::from_utf8(read_golden(&format!("hooks/{file}"))).expect("utf-8 dump");
        let rewritten = top_level_source(&want);
        checked += 1;
        match dump(&rewritten) {
            Ok(got) => {
                if got != want {
                    failures.push(format!(
                        "{label} (planned): {}",
                        first_difference(&got, &want)
                    ));
                }
            }
            Err(e) => failures.push(format!("{label} (planned): not compiled: {e}")),
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} dumps differ:\n{}",
        failures.len(),
        checked,
        failures.join("\n")
    );
    assert_eq!(checked, code_files_on_disk());
    assert_eq!(
        not_compiled,
        [
            "syntax",
            "syntax2",
            "import",
            "for-await",
            "async-gen",
            "using",
            "decorator",
            "html-comment",
            "regex-flags-d",
            "sourcemap-missing-file",
            "sourcemap-bad-data",
            "re-bad-literal",
        ]
    );
}
