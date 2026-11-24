use std::collections::BTreeMap;

use elysiandb::globals;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::stat::{self, new_stats_container};

fn fields(text: &str) -> BTreeMap<String, Value> {
    match &json::unmarshal_value(text.as_bytes()) {
        Ok(Value::Object(m)) => m.clone(),
        Ok(other) => panic!("json.Unmarshal failed: {other:?}"),
        Err(err) => panic!("json.Unmarshal failed: {err}"),
    }
}

fn strings(text: &str) -> BTreeMap<String, String> {
    fields(text)
        .into_iter()
        .map(|(k, v)| match v.as_str() {
            Some(s) => (k, s.to_string()),
            None => panic!("field {k:?} is not a string"),
        })
        .collect()
}

fn check(m: &BTreeMap<String, String>, want: &[(&str, &str)]) {
    for (k, w) in want {
        let got = m.get(*k).map(String::as_str).unwrap_or("");
        if got != *w {
            panic!("field {k:?} = {got:?}, want {w:?}");
        }
    }
}

#[test]
fn test_new_stats_container_zeroed() {
    let s = new_stats_container();
    let m = strings(&s.to_json());
    check(
        &m,
        &[
            ("keys_count", "0"),
            ("expiration_keys_count", "0"),
            ("uptime_seconds", "0"),
            ("total_requests", "0"),
            ("hits", "0"),
            ("misses", "0"),
            ("version", globals::VERSION),
        ],
    );
}

#[test]
fn test_increments_and_sets() {
    let s = new_stats_container();
    s.increment_keys_count();
    s.increment_expiration_keys_count();
    s.increment_uptime_seconds();
    s.increment_total_requests();
    s.increment_hits();
    s.increment_misses();
    s.set_keys_count(42);
    s.set_expiration_keys_count(7);
    let m = strings(&s.to_json());
    check(
        &m,
        &[
            ("keys_count", "42"),
            ("expiration_keys_count", "7"),
            ("uptime_seconds", "1"),
            ("total_requests", "1"),
            ("hits", "1"),
            ("misses", "1"),
            ("version", globals::VERSION),
        ],
    );
}

#[test]
fn test_decrement_does_not_go_below_zero() {
    let s = new_stats_container();
    s.decrement_keys_count();
    s.decrement_expiration_keys_count();
    s.increment_keys_count();
    s.increment_expiration_keys_count();
    s.decrement_keys_count();
    s.decrement_expiration_keys_count();
    let m = strings(&s.to_json());
    check(&m, &[("keys_count", "0"), ("expiration_keys_count", "0")]);
}

#[test]
fn test_reset() {
    let s = new_stats_container();
    s.set_keys_count(10);
    s.set_expiration_keys_count(5);
    s.increment_uptime_seconds();
    s.increment_total_requests();
    s.increment_hits();
    s.increment_misses();
    s.reset();
    for (k, v) in strings(&s.to_json()) {
        if k == "version" {
            continue;
        }
        if v != "0" {
            panic!("after Reset, {k} = {v}, want 0");
        }
    }
}

#[test]
fn test_to_json_values_are_strings() {
    let s = new_stats_container();
    s.set_keys_count(3);
    s.set_expiration_keys_count(4);
    s.increment_uptime_seconds();
    s.increment_total_requests();
    s.increment_hits();
    s.increment_misses();
    for (k, v) in fields(&s.to_json()) {
        if v.as_str().is_none() {
            panic!("field {k:?} is not a string in JSON (got {v:?})");
        }
    }
}

#[test]
fn test_concurrent_increments_no_race_and_counts_match() {
    let s = new_stats_container();
    const GOROUTINES: usize = 64;
    const ITERS: usize = 10_000;
    std::thread::scope(|sc| {
        for _ in 0..GOROUTINES {
            sc.spawn(|| {
                for _ in 0..ITERS {
                    s.increment_keys_count();
                    s.increment_expiration_keys_count();
                    s.increment_total_requests();
                    s.increment_hits();
                }
            });
        }
    });
    let m = strings(&s.to_json());
    let want = (GOROUTINES * ITERS).to_string();
    for field in [
        "keys_count",
        "expiration_keys_count",
        "total_requests",
        "hits",
    ] {
        if m.get(field) != Some(&want) {
            panic!("{field} = {:?}, want {want}", m.get(field));
        }
    }
}

#[test]
fn test_init_global_stats_is_zeroed() {
    stat::stats().increment_hits();
    stat::stats().increment_misses();
    stat::stats().set_keys_count(9);
    stat::stats().set_expiration_keys_count(2);
    stat::init();
    for (k, v) in strings(&stat::stats().to_json()) {
        if k == "version" {
            continue;
        }
        if v != "0" {
            panic!("after Init, {k} = {v}, want 0");
        }
    }
}
