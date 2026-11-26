use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::{Duration, Instant};

use elysiandb::api::{self, Guard, ListQuery};
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::gocompat::time::{Duration as GoDuration, Location, Month, Time};
use elysiandb::query::{FilterNode, Filters, Query};

use super::index_test::{obj, s};
use super::{filters, ids, init_test_store, setup};

const EVAL_DATES: [&str; 13] = [
    "2015-01-01",
    "2014-12-31",
    "2015-01-02",
    "2022-01-01",
    "2021-12-31",
    "2015-01-01T00:30:00+02:00",
    "2014-12-31T23:59:59Z",
    "2015-01-01T00:00:00Z",
    "2021-12-31T22:00:00-03:00",
    "2022-01-01T00:00:00.5Z",
    "2015-01-01T23:59:59.999999999Z",
    "0000-01-01",
    "9999-12-31",
];
const EVAL_STRINGS: [&str; 9] = [
    "alpha",
    "2015",
    "2015-01-0",
    "zzz",
    "",
    "a\u{0}b",
    "Été",
    "2015-01-01x",
    "2015-13-01",
];
const EVAL_CMPS: [&str; 12] = [
    "2015-01-01",
    "2022-01-01",
    "2014-12-31",
    "2015-01-01T12:00:00Z",
    "alpha",
    "2015",
    "42",
    "true",
    "zzz",
    "a*",
    "*0*",
    "0000-01-01",
];
const EVAL_OPS: [&str; 11] = [
    "gte",
    "gt",
    "lt",
    "lte",
    "eq",
    "neq",
    "contains",
    "not_contains",
    "any",
    "all",
    "none",
];

fn intn(r: &mut Rand, n: usize) -> usize {
    r.intn(n as i64) as usize
}

fn eval_value(r: &mut Rand, dates_only: bool) -> Value {
    if dates_only {
        return s(EVAL_DATES[intn(r, EVAL_DATES.len() - 2)]);
    }

    match r.intn(8) {
        7 => Value::Array(Vec::new()),
        0..=2 => s(EVAL_DATES[intn(r, EVAL_DATES.len())]),
        3 => s(EVAL_STRINGS[intn(r, EVAL_STRINGS.len())]),
        4 => Value::Number(r.intn(60) as f64 - 10.0),
        5 => Value::Bool(r.intn(2) == 0),
        _ => {
            let a = s(EVAL_DATES[intn(r, EVAL_DATES.len())]);
            let b = Value::Number(r.intn(3) as f64);
            let c = s(EVAL_STRINGS[intn(r, EVAL_STRINGS.len())]);
            Value::Array(vec![a, b, c])
        }
    }
}

fn single_ops(op: &str, cmp: &str) -> BTreeMap<Vec<u8>, Vec<u8>> {
    BTreeMap::from([(op.as_bytes().to_vec(), cmp.as_bytes().to_vec())])
}

fn eval_filter(r: &mut Rand) -> Filters {
    let field = ["day", "mixed", "rows.at"][intn(r, 3)];
    let mut ops: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    let mut n = 1 + r.intn(2);
    while n > 0 {
        let op = EVAL_OPS[intn(r, EVAL_OPS.len())];
        let cmp = EVAL_CMPS[intn(r, EVAL_CMPS.len())];
        ops.insert(op.as_bytes().to_vec(), cmp.as_bytes().to_vec());
        n -= 1;
    }
    if r.intn(2) == 0 {
        let op = ["gte", "gt", "lt", "lte"][intn(r, 4)];
        let cmp = EVAL_CMPS[intn(r, 3)];
        ops = single_ops(op, cmp);
        if r.intn(2) == 0 {
            let op = ["gte", "gt", "lt", "lte"][intn(r, 4)];
            let cmp = EVAL_CMPS[intn(r, 3)];
            ops.insert(op.as_bytes().to_vec(), cmp.as_bytes().to_vec());
        }
    }

    Filters::from([(field.as_bytes().to_vec(), ops)])
}

pub fn wait_for_indexes() {
    let deadline = Instant::now() + Duration::from_secs(20);
    while api::index_backlog() > 0 {
        if Instant::now() > deadline {
            panic!("the index backlog never drained");
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

#[test]
fn filters_evaluated_on_the_index_match_the_scan() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(7));
    let mut all: Vec<Map> = Vec::new();
    for i in 0..3000 {
        let mut d = obj(vec![("id", s(&format!("e{i:04}")))]);
        if r.intn(10) > 0 {
            d.insert("day".to_string(), eval_value(&mut r, true));
        }
        if r.intn(10) > 0 {
            d.insert("mixed".to_string(), eval_value(&mut r, false));
        }
        if r.intn(3) > 0 {
            let flag = r.intn(2) == 0;
            let a = eval_value(&mut r, flag);
            let b = eval_value(&mut r, false);
            d.insert(
                "rows".to_string(),
                Value::Array(vec![
                    Value::Object(obj(vec![("at", a)])),
                    Value::Object(obj(vec![("at", b)])),
                ]),
            );
        }
        all.push(d);
    }
    api::write_list_of_entities(b"evaluated", &mut all);
    let reference = api::list_entities(b"evaluated", 0, 0, b"", true, None, b"", b"");
    for field in ["day", "mixed", "rows.at"] {
        let f = filters(&[(field, &[("eq", "x")])]);
        api::plan(ListQuery {
            entity: b"evaluated",
            filters: Some(&f),
            ..ListQuery::default()
        });
    }
    wait_for_indexes();

    let mut single: Vec<Filters> = Vec::new();
    for field in ["day", "mixed", "rows.at"] {
        for op in EVAL_OPS {
            for cmp in EVAL_CMPS {
                single.push(Filters::from([(
                    field.as_bytes().to_vec(),
                    single_ops(op, cmp),
                )]));
            }
        }
    }

    for i in 0..single.len() + 300 {
        let f = if i < single.len() {
            single[i].clone()
        } else {
            eval_filter(&mut r)
        };
        let want = ids(&reference
            .iter()
            .filter(|d| api::filters_match_entity(d, &f))
            .cloned()
            .collect::<Vec<_>>());
        let got = ids(&api::list_entities(
            b"evaluated",
            0,
            0,
            b"",
            true,
            Some(&f),
            b"",
            b"",
        ));
        if got != want {
            panic!(
                "filters {f:?}: planner {} documents, scan {}",
                got.len(),
                want.len()
            );
        }
        let res = api::plan(ListQuery {
            entity: b"evaluated",
            filters: Some(&f),
            count_only: true,
            ..ListQuery::default()
        });
        if res.total != want.len() {
            panic!("filters {f:?}: count {}, scan {}", res.total, want.len());
        }
        let other = eval_filter(&mut r);
        let node =
            FilterNode::with_or(vec![FilterNode::with_leaf(f), FilterNode::with_leaf(other)]);
        let want_node = reference
            .iter()
            .filter(|d| api::query_filter_matches_entity(&node, d))
            .count();
        let q = Query {
            entity: b"evaluated".to_vec(),
            filter: node,
            ..Query::default()
        };
        let res = api::plan_query(&q, true);
        if res.total != want_node {
            panic!("node: count {}, scan {want_node}", res.total);
        }
    }
}

#[test]
fn date_range_count_is_served_by_the_index() {
    let _s = setup();
    let _db = init_test_store();

    let start = Time::from_date(2010, Month::JANUARY, 1, 0, 0, 0, 0, &Location::utc());
    let mut all: Vec<Map> = Vec::with_capacity(20000);
    for i in 0..20000i64 {
        let day = start.add(GoDuration(i * 6 * GoDuration::HOUR.0));
        all.push(obj(vec![
            ("id", s(&format!("v{i:05}"))),
            ("date_start", s(&day.format("2006-01-02"))),
            ("note", s("viewing")),
        ]));
    }
    api::write_list_of_entities(b"viewing", &mut all);
    let f = filters(&[("date_start", &[("gte", "2015-01-01")])]);
    api::plan(ListQuery {
        entity: b"viewing",
        filters: Some(&f),
        count_only: true,
        ..ListQuery::default()
    });
    wait_for_indexes();
    let want = all
        .iter()
        .filter(|d| d.get("date_start").and_then(Value::as_str).unwrap_or("") >= "2015-01-01")
        .count();

    let decoded = Arc::new(AtomicI64::new(0));
    let counter = decoded.clone();
    let guard = Guard::new(
        None,
        Some(Box::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
            false
        })),
    );
    let res = api::plan(ListQuery {
        entity: b"viewing",
        filters: Some(&f),
        count_only: true,
        guard: Some(&guard),
        ..ListQuery::default()
    });
    if res.total != want {
        panic!("count {}, want {want}", res.total);
    }
    let n = decoded.load(Ordering::SeqCst);
    if n != 0 {
        panic!(
            "a day range on a date field is counted on the index, {n} guard checks while verifying documents"
        );
    }
}
