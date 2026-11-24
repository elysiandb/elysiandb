use std::collections::{BTreeMap, BTreeSet};

use crate::common;
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strconv;
use elysiandb::memory::{self, Level};

const REL: &str = "engine/memory.jsonl";

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

fn fields(row: &Value) -> BTreeSet<String> {
    row.as_object().expect("object").keys().cloned().collect()
}

fn set(keys: &[&str]) -> BTreeSet<String> {
    keys.iter().map(|k| k.to_string()).collect()
}

fn by_pool(v: &Value) -> BTreeMap<String, i64> {
    v.as_object()
        .expect("pool map")
        .iter()
        .map(|(k, v)| (k.clone(), dec(v)))
        .collect()
}

fn pool_values(f: impl Fn(&memory::Pool) -> i64) -> BTreeMap<String, i64> {
    memory::pools()
        .iter()
        .map(|p| (p.name().to_string(), f(p)))
        .collect()
}

fn check_configure(row: &Value) {
    assert_eq!(
        fields(row),
        set(&["fn", "in", "budget", "limits", "effective", "request_cap"])
    );
    let b = dec(field(row, "in"));
    memory::configure(b);

    assert_eq!(
        dec(field(row, "budget")),
        memory::budget(),
        "Configure({b}) budget"
    );
    assert_eq!(
        by_pool(field(row, "limits")),
        pool_values(memory::Pool::limit),
        "Configure({b}) limits"
    );

    let effective = field(row, "effective");
    assert_eq!(fields(effective), set(&["normal", "high", "critical"]));
    for lv in [Level::Normal, Level::High, Level::Critical] {
        memory::set_level(lv);
        assert_eq!(
            by_pool(field(effective, lv.as_str())),
            pool_values(memory::Pool::effective),
            "Configure({b}) effective at {lv}"
        );
    }
    memory::set_level(Level::Normal);

    assert_eq!(
        dec(field(row, "request_cap")),
        memory::request_cap(),
        "Configure({b}) request cap"
    );
}

fn check_level_for(row: &Value) {
    assert_eq!(fields(row), set(&["fn", "in", "out"]));
    let input: Vec<i64> = field(row, "in")
        .as_array()
        .expect("in")
        .iter()
        .map(dec)
        .collect();
    assert_eq!(input.len(), 3);
    let got = memory::level_for(input[0], input[1], input[2]);
    assert_eq!(
        common::str_field(row, "out"),
        got.as_str(),
        "levelFor({input:?})"
    );
}

fn level_of(v: &Value) -> Level {
    match v.as_i64() {
        Some(0) => Level::Normal,
        Some(1) => Level::High,
        Some(2) => Level::Critical,
        other => panic!("unknown level {other:?}"),
    }
}

fn check_follow_spare(row: &Value) {
    assert_eq!(
        fields(row),
        set(&["fn", "sequence", "budget", "steps", "elastic"])
    );
    let name = common::str_field(row, "sequence");
    let b = dec(field(row, "budget"));
    let steps = field(row, "steps").as_array().expect("steps");
    let want: Vec<i64> = field(row, "elastic")
        .as_array()
        .expect("elastic")
        .iter()
        .map(dec)
        .collect();
    assert_eq!(steps.len(), want.len(), "{name}: step count");

    memory::set_elastic(0);
    let mut got = Vec::new();
    for s in steps {
        assert_eq!(fields(s), set(&["live", "level"]), "{name}: step fields");
        let live = match field(s, "live") {
            Value::Number(n) => strconv::parse_float(n.as_str().as_bytes(), 64)
                .unwrap_or_else(|e| panic!("bad float {}: {e}", n.as_str())),
            other => panic!("live is not a number: {other:?}"),
        };
        memory::follow_spare(b, (b as f64 * live) as i64, level_of(field(s, "level")));
        got.push(memory::elastic());
    }
    assert_eq!(got, want, "followSpare {name} with budget {b}");
}

#[test]
fn golden_memory() {
    let rows = common::read_jsonl(REL);
    assert_eq!(rows.len(), common::lines(&common::read_golden(REL)).len());

    let mut checked = 0;
    for row in &rows {
        match common::str_field(row, "fn") {
            "Configure" => check_configure(row),
            "levelFor" => check_level_for(row),
            "followSpare" => check_follow_spare(row),
            other => panic!("unknown function {other}"),
        }
        checked += 1;
    }

    memory::set_elastic(0);
    memory::configure(0);

    assert_eq!(checked, rows.len());
    assert_eq!(checked, 238);
}
