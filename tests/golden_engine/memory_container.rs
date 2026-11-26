use std::path::PathBuf;

use crate::common;
use crate::tempdir::TempDir;
use elysiandb::boot;
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strconv;

const REL: &str = "engine/memory_container.jsonl";
const BUDGET_ROWS: usize = 24;
const LIMIT_ROWS: usize = 27;

fn dec(v: &Value) -> i64 {
    let s = v
        .as_str()
        .unwrap_or_else(|| panic!("not a decimal string: {v:?}"));
    strconv::parse_int(s.as_bytes(), 10, 64).unwrap_or_else(|e| panic!("bad integer {s}: {e}"))
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing field {key} in {row:?}"))
}

#[test]
fn memory_container_matches_go() {
    let rows = common::read_jsonl(REL);
    let dir = TempDir::new();
    let (mut budgets, mut limits) = (0, 0);

    for (i, row) in rows.iter().enumerate() {
        let want = dec(field(row, "out"));
        match field(row, "fn").as_str() {
            Some("BudgetWithinContainer") => {
                let input = field(row, "in").as_array().expect("in");
                let got = boot::budget_within_container(dec(&input[0]), dec(&input[1]));
                assert_eq!(got, want, "{row:?}");
                budgets += 1;
            }
            Some("containerMemoryLimit") => {
                let mut paths: Vec<PathBuf> = Vec::new();
                let first = dir.path().join(format!("first-{i}"));
                if let Some(text) = field(row, "first_file").as_str() {
                    std::fs::write(&first, text).expect("first file");
                }
                paths.push(first);
                if let Some(text) = row.get("second_file").and_then(Value::as_str) {
                    let second = dir.path().join(format!("second-{i}"));
                    std::fs::write(&second, text).expect("second file");
                    paths.push(second);
                }
                let refs: Vec<&std::path::Path> = paths.iter().map(PathBuf::as_path).collect();
                assert_eq!(boot::container_memory_limit_from(&refs), want, "{row:?}");
                limits += 1;
            }
            other => panic!("unknown function {other:?}"),
        }
    }

    assert_eq!((budgets, limits), (BUDGET_ROWS, LIMIT_ROWS));
    assert_eq!(budgets + limits, rows.len());
}
