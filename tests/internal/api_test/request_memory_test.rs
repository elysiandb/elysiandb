use elysiandb::api::{self, Guard, GuardError, ListQuery};
use elysiandb::configuration::ApiQueryConfig;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::memory;

use super::overload_test::{OVERLOAD_DOCS, init_overload_store};
use super::setup;

struct ResetBudget;

impl Drop for ResetBudget {
    fn drop(&mut self) {
        memory::configure(0);
    }
}

#[test]
fn requests_beyond_the_memory_budget_are_rejected() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig {
        queue_timeout_ms: 20,
        ..ApiQueryConfig::default()
    });
    let mut wide: Vec<Map> = (0..2000)
        .map(|i| {
            let mut m = Map::new();
            m.insert("id".to_string(), Value::String(format!("w{i:05}")));
            m.insert("body".to_string(), Value::String("w".repeat(1024)));
            m
        })
        .collect();
    api::write_list_of_entities(b"wide", &mut wide);

    memory::configure(8 << 20);
    let _reset = ResetBudget;

    let guard = Guard::default();
    let res = api::plan(ListQuery {
        entity: b"wide",
        guard: Some(&guard),
        ..ListQuery::default()
    });
    let docs = res.decode();

    if guard.err() != Some(GuardError::Busy) || !docs.is_empty() {
        panic!(
            "a request needing more than the request share must be refused, got {} documents and {:?}",
            docs.len(),
            guard.err()
        );
    }

    guard.release_memory();
    let used = memory::REQUESTS.used();
    if used != 0 {
        panic!("a finished request gives its memory back, {used} bytes still reserved");
    }

    let small = Guard::default();
    let page = api::plan(ListQuery {
        entity: b"wide",
        limit: 20,
        guard: Some(&small),
        ..ListQuery::default()
    });
    let got = page.decode();
    if got.len() != 20 || small.err().is_some() {
        panic!(
            "a small page fits in the budget, got {} documents and {:?}",
            got.len(),
            small.err()
        );
    }
    small.release_memory();
}

#[test]
fn request_memory_is_free_without_a_budget() {
    let _s = setup();
    let _db = init_overload_store(ApiQueryConfig::default());
    memory::configure(0);

    let guard = Guard::default();
    let docs = api::plan(ListQuery {
        entity: b"big",
        guard: Some(&guard),
        ..ListQuery::default()
    })
    .decode();
    if docs.len() != OVERLOAD_DOCS || guard.err().is_some() {
        panic!(
            "without an explicit budget nothing is refused, got {} documents and {:?}",
            docs.len(),
            guard.err()
        );
    }
}
