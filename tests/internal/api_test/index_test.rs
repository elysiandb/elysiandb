use std::collections::BTreeMap;

use elysiandb::api::{self, ListQuery};
use elysiandb::datastore::Doc;
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::query::{FilterNode, Filters, Query, Sort};
use elysiandb::storage;

use super::{doc, filters, ids, init_test_store, setup, write};

pub fn decode_ids(b: &[u8]) -> Vec<String> {
    if b.is_empty() {
        return Vec::new();
    }
    b.split(|&c| c == b'\n')
        .filter(|p| !p.is_empty())
        .map(|p| String::from_utf8_lossy(p).into_owned())
        .collect()
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

pub fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn intn(r: &mut Rand, n: usize) -> usize {
    r.intn(n as i64) as usize
}

pub fn res_ids(res: &api::ListResult<'_>) -> Vec<String> {
    res.ids()
        .into_iter()
        .map(|id| String::from_utf8_lossy(&id).into_owned())
        .collect()
}

#[test]
fn concurrent_writes_to_one_entity_lose_no_id() {
    let _s = setup();
    let _db = init_test_store();

    write("items", r#"{"id":"seed","n":-1.0}"#);
    api::ensure_sort_index(b"items", b"n");
    std::thread::scope(|sc| {
        for g in 0..16 {
            sc.spawn(move || {
                for i in 0..200 {
                    let mut d = obj(vec![
                        ("id", s(&format!("g{g:02}-{i:03}"))),
                        ("n", Value::Number((g * 1000 + i) as f64)),
                    ]);
                    api::write_entity(b"items", &mut d);
                }
            });
        }
    });

    let want = 3201;
    let got = api::count_entities(b"items");
    if got != want {
        panic!("count {got}, want {want}");
    }
    let got = api::list_entities(b"items", 0, 0, b"", true, None, b"", b"").len();
    if got != want {
        panic!("list {got}, want {want}");
    }
    let got = api::list_entities(b"items", 0, 0, b"n", true, None, b"", b"").len();
    if got != want {
        panic!("sorted list {got}, want {want}");
    }
    match api::get_list_of_ids(b"items", b"", true) {
        Ok(raw) if decode_ids(&raw).len() == want => {}
        other => panic!(
            "id index, want {want} ({:?})",
            other.map(|r| decode_ids(&r).len())
        ),
    }
}

#[test]
fn write_during_an_index_build_is_in_the_index() {
    let _s = setup();
    let _db = init_test_store();

    let mut docs: Vec<Map> = (0..20000)
        .map(|i| {
            obj(vec![
                ("id", s(&format!("d{i:05}"))),
                ("rank", Value::Number(i as f64)),
            ])
        })
        .collect();
    api::write_list_of_entities(b"big", &mut docs);
    std::thread::scope(|sc| {
        let late = sc.spawn(|| {
            api::write_entity(b"big", &mut doc(r#"{"id":"late","rank":-1.0}"#));
        });
        let _ = api::list_entities(b"big", 0, 0, b"rank", true, None, b"", b"");
        let _ = late.join();
    });
    let sorted = api::list_entities(b"big", 0, 0, b"rank", true, None, b"", b"");
    if sorted.len() != 20001 || ids(&sorted)[0] != "late" {
        panic!(
            "expected 20001 sorted documents starting with the late write, got {}, first {:?}",
            sorted.len(),
            sorted.first().and_then(|d| d.get("id"))
        );
    }
    let got = api::count_entities(b"big");
    if got != 20001 {
        panic!("count {got}");
    }
}

#[test]
fn sort_ties_come_in_insertion_order_and_missing_values_last() {
    let _s = setup();
    let _db = init_test_store();

    for d in [
        r#"{"id":"s1","v":"b"}"#,
        r#"{"id":"n2","v":2.0}"#,
        r#"{"id":"m1"}"#,
        r#"{"id":"s2","v":"a"}"#,
        r#"{"id":"n1","v":1.0}"#,
        r#"{"id":"d1","v":"2024-01-02"}"#,
        r#"{"id":"b1","v":true}"#,
        r#"{"id":"s3","v":"a"}"#,
        r#"{"id":"m2","v":null}"#,
        r#"{"id":"o1","v":{"x":1.0}}"#,
        r#"{"id":"d2","v":"2023-05-01T10:00:00+02:00"}"#,
        r#"{"id":"b2","v":false}"#,
        r#"{"id":"n3","v":1.0}"#,
    ] {
        write("mixed", d);
    }

    let asc = ids(&api::list_entities(
        b"mixed", 0, 0, b"v", true, None, b"", b"",
    ));
    let want_asc = [
        "n1", "n3", "n2", "d2", "d1", "s2", "s3", "s1", "b2", "b1", "o1", "m1", "m2",
    ];
    if asc != want_asc {
        panic!("asc {asc:?}, want {want_asc:?}");
    }
    let desc = ids(&api::list_entities(
        b"mixed", 0, 0, b"v", false, None, b"", b"",
    ));
    let want_desc = [
        "o1", "b1", "b2", "s1", "s2", "s3", "d1", "d2", "n2", "n1", "n3", "m1", "m2",
    ];
    if desc != want_desc {
        panic!("desc {desc:?}, want {want_desc:?}");
    }
    for offset in 0..want_desc.len() {
        let page = ids(&api::list_entities(
            b"mixed",
            3,
            offset as i64,
            b"v",
            false,
            None,
            b"",
            b"",
        ));
        let end = (offset + 3).min(want_desc.len());
        if page != want_desc[offset..end] {
            panic!(
                "desc page at {offset}: {page:?}, want {:?}",
                &want_desc[offset..end]
            );
        }
    }
    let mut data = api::list_entities(b"mixed", 0, 0, b"", true, None, b"", b"");
    api::sort_entities(&mut data, &[Sort::new("v", false)]);
    if ids(&data) != want_desc {
        panic!(
            "the Query API comparator gives {:?}, the index {want_desc:?}",
            ids(&data)
        );
    }
}

#[test]
fn indexes_follow_updates_deletes_and_resets() {
    let _s = setup();
    let _db = init_test_store();

    for i in 0..50 {
        let mut d = obj(vec![
            ("id", s(&format!("u{i:02}"))),
            ("age", Value::Number((50 - i) as f64)),
            ("team", s(&format!("t{}", i % 5))),
        ]);
        api::write_entity(b"users", &mut d);
    }
    let f = filters(&[("team", &[("eq", "t1")])]);
    let got = api::list_entities(b"users", 0, 0, b"age", true, Some(&f), b"", b"").len();
    if got != 10 {
        panic!("expected 10 members of t1, got {got}");
    }
    api::update_entity_by_id(b"users", b"u01", &doc(r#"{"team":"t2","age":1000.0}"#));
    api::delete_entity_by_id(b"users", b"u06");
    let got = ids(&api::list_entities(
        b"users",
        0,
        0,
        b"age",
        true,
        Some(&f),
        b"",
        b"",
    ));
    if got.len() != 8 || got[0] != "u46" {
        panic!("unexpected t1 members after an update and a delete: {got:?}");
    }
    let sorted = ids(&api::list_entities(
        b"users", 1, 0, b"age", false, None, b"", b"",
    ));
    if sorted != ["u01"] {
        panic!("the updated age leads the descending sort, got {sorted:?}");
    }
    api::delete_all_entities(b"users");
    let got = api::list_entities(b"users", 0, 0, b"age", true, Some(&f), b"", b"");
    if !got.is_empty() {
        panic!("a reset empties every index, got {got:?}");
    }
    write("users", r#"{"id":"new","age":3.0,"team":"t1"}"#);
    let got = ids(&api::list_entities(
        b"users",
        0,
        0,
        b"age",
        true,
        Some(&f),
        b"",
        b"",
    ));
    if got != ["new"] {
        panic!("indexes work after a reset, got {got:?}");
    }
}

#[test]
fn index_keys_for_the_documented_entity_keys() {
    let _s = setup();
    let _db = init_test_store();

    write("books", r#"{"id":"b2","title":"Zed"}"#);
    write("books", r#"{"id":"b1","title":"Ada"}"#);
    api::list_entities(b"books", 0, 0, b"title", true, None, b"", b"");
    match storage::get_by_key(globals::api_entity_index_id_key("books").as_bytes()) {
        Ok(raw) if decode_ids(&raw).join(",") == "b2,b1" => {}
        other => panic!("id index {other:?}"),
    }
    match storage::get_by_key(
        globals::api_entity_index_field_sort_asc_key("books", "title").as_bytes(),
    ) {
        Ok(raw) if decode_ids(&raw).join(",") == "b1,b2" => {}
        other => panic!("sort index {other:?}"),
    }
    match storage::get_by_key(globals::api_all_entity_types_list_key().as_bytes()) {
        Ok(raw) if raw == b"books" => {}
        other => panic!("types list {other:?}"),
    }
    if !api::index_exists_for_field(b"books", b"title")
        || api::get_list_for_indexed_fields(b"books") != [b"title".to_vec()]
    {
        panic!("the sort index of title should be listed");
    }
    let got = api::get_sorted_entity_ids_by_field(b"books", b"title", false);
    if got != [b"b2".to_vec(), b"b1".to_vec()] {
        panic!("descending ids {got:?}");
    }
}

const DIFF_STRINGS: [&str; 14] = [
    "alpha",
    "Alpha",
    "beta",
    "BETA gamma",
    "2024-01-15",
    "2024-01-15T08:30:00Z",
    "2024-01-15T23:30:00-05:00",
    "2023-12-31",
    "",
    "a*b",
    "x\\y",
    "42",
    "true",
    "Ünïcode",
];
const DIFF_OPS: [&str; 12] = [
    "eq",
    "neq",
    "lt",
    "lte",
    "gt",
    "gte",
    "contains",
    "not_contains",
    "all",
    "any",
    "none",
    "weird",
];
const DIFF_CMPS: [&str; 20] = [
    "alpha",
    "ALPHA",
    "al*",
    "*ta",
    "*",
    "b*a",
    "2024-01-15",
    "2024-01-15T10:00:00Z",
    "2023-06-01",
    "42",
    "41.5",
    "true",
    "1",
    "0",
    "false",
    "beta,alpha",
    "1,2",
    "",
    "a\\*b",
    "ü*",
];

pub fn random_value(r: &mut Rand, depth: i64) -> Value {
    match r.intn(9) {
        0 | 1 => s(DIFF_STRINGS[intn(r, DIFF_STRINGS.len())]),
        2 => {
            let base = r.intn(100) as f64 - 20.0;
            Value::Number(base + [0.0, 0.5][intn(r, 2)])
        }
        3 => Value::Bool(r.intn(2) == 0),
        4 => {
            let n = r.intn(4);
            let mut arr = Vec::new();
            for _ in 0..n {
                if r.intn(3) == 0 {
                    arr.push(Value::Number(r.intn(3) as f64));
                } else {
                    arr.push(s(DIFF_STRINGS[intn(r, DIFF_STRINGS.len())]));
                }
            }
            Value::Array(arr)
        }
        5 => {
            if depth > 1 {
                return Value::Null;
            }
            let x = random_value(r, depth + 1);
            let y = random_value(r, depth + 1);
            Value::Object(obj(vec![("x", x), ("y", y)]))
        }
        6 => {
            if depth > 1 {
                return s("leaf");
            }
            let a = random_value(r, depth + 1);
            let b = random_value(r, depth + 1);
            Value::Array(vec![
                Value::Object(obj(vec![("x", a)])),
                Value::Object(obj(vec![("x", b)])),
            ])
        }
        7 => Value::Null,
        _ => Value::Number(r.intn(5) as f64),
    }
}

pub fn random_leaf(r: &mut Rand) -> Filters {
    let field = ["a", "b", "m.x", "m.y", "list.x"][intn(r, 5)];
    let op = DIFF_OPS[intn(r, DIFF_OPS.len())];
    let cmp = DIFF_CMPS[intn(r, DIFF_CMPS.len())];
    let mut ops: BTreeMap<Vec<u8>, Vec<u8>> = BTreeMap::new();
    ops.insert(op.as_bytes().to_vec(), cmp.as_bytes().to_vec());
    if r.intn(4) == 0 {
        let op = DIFF_OPS[intn(r, DIFF_OPS.len())];
        let cmp = DIFF_CMPS[intn(r, DIFF_CMPS.len())];
        ops.insert(op.as_bytes().to_vec(), cmp.as_bytes().to_vec());
    }
    let mut f = Filters::new();
    f.insert(field.as_bytes().to_vec(), ops);
    if r.intn(3) == 0 {
        let other = ["a", "b", "m.x"][intn(r, 3)];
        if !f.contains_key(other.as_bytes()) {
            let op = DIFF_OPS[intn(r, 4)];
            let cmp = DIFF_CMPS[intn(r, DIFF_CMPS.len())];
            let mut o = BTreeMap::new();
            o.insert(op.as_bytes().to_vec(), cmp.as_bytes().to_vec());
            f.insert(other.as_bytes().to_vec(), o);
        }
    }
    f
}

#[test]
fn planner_matches_the_scan_on_random_documents_and_filters() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(42));
    let mut all: Vec<Map> = Vec::new();
    for i in 0..600 {
        let mut d = obj(vec![("id", s(&format!("r{i:03}")))]);
        for f in ["a", "b", "m", "list"] {
            if r.intn(6) == 0 {
                continue;
            }
            let v = match f {
                "m" => {
                    let x = random_value(&mut r, 1);
                    let y = random_value(&mut r, 1);
                    Value::Object(obj(vec![("x", x), ("y", y)]))
                }
                "list" => {
                    let a = random_value(&mut r, 1);
                    let b = random_value(&mut r, 1);
                    Value::Array(vec![
                        Value::Object(obj(vec![("x", a)])),
                        Value::Object(obj(vec![("x", b)])),
                    ])
                }
                _ => random_value(&mut r, 0),
            };
            d.insert(f.to_string(), v);
        }
        all.push(d);
    }
    api::write_list_of_entities(b"diff", &mut all);
    let reference = api::list_entities(b"diff", 0, 0, b"", true, None, b"", b"");
    let scan = |f: &Filters| -> Vec<String> {
        ids(&reference
            .iter()
            .filter(|d| api::filters_match_entity(d, f))
            .cloned()
            .collect::<Vec<_>>())
    };

    for _ in 0..1500 {
        let f = random_leaf(&mut r);
        let want = scan(&f);
        let got = ids(&api::list_entities(
            b"diff",
            0,
            0,
            b"",
            true,
            Some(&f),
            b"",
            b"",
        ));
        if got != want {
            panic!("filters {f:?}: planner {got:?}, scan {want:?}");
        }
        let res = api::plan(ListQuery {
            entity: b"diff",
            filters: Some(&f),
            count_only: true,
            ..ListQuery::default()
        });
        if res.total != want.len() {
            panic!("filters {f:?}: count {}, scan {}", res.total, want.len());
        }
    }

    for _ in 0..300 {
        let first = random_leaf(&mut r);
        let second = random_leaf(&mut r);
        let third = random_leaf(&mut r);
        let node = FilterNode::with_or(vec![
            FilterNode::with_leaf(first),
            FilterNode::with_and(vec![
                FilterNode::with_leaf(second),
                FilterNode::with_leaf(third),
            ]),
        ]);
        let want: Vec<String> = ids(&reference
            .iter()
            .filter(|d| api::query_filter_matches_entity(&node, d))
            .cloned()
            .collect::<Vec<_>>());
        let q = Query {
            entity: b"diff".to_vec(),
            filter: node,
            ..Query::default()
        };
        let got = ids(&api::plan_query(&q, false).decode());
        if got != want {
            panic!("node: planner {got:?}, scan {want:?}");
        }
    }
}

#[test]
fn search_index_matches_the_scan() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(3));
    let words = [
        "dragon",
        "Dune",
        "foundation",
        "kindred",
        "the",
        "abc",
        "Ünïcode",
        "a*b",
        "beginning",
    ];
    let mut docs: Vec<Map> = Vec::new();
    for i in 0..300 {
        let a = words[intn(&mut r, words.len())];
        let b = words[intn(&mut r, words.len())];
        let mut d = obj(vec![
            ("id", s(&format!("s{i:03}"))),
            ("title", s(&format!("{a} {b}"))),
        ]);
        if r.intn(2) == 0 {
            d.insert(
                "tags".to_string(),
                Value::Array(vec![s(words[intn(&mut r, words.len())])]),
            );
        }
        if r.intn(3) == 0 {
            let note = s(words[intn(&mut r, words.len())]);
            d.insert(
                "meta".to_string(),
                Value::Object(obj(vec![
                    ("note", note),
                    ("_elysiandb_core_x", s("dragon")),
                ])),
            );
        }
        docs.push(d);
    }
    api::write_list_of_entities(b"texts", &mut docs);
    api::list_entities(b"texts", 0, 0, b"", true, None, b"warm", b"");
    if let Some(e) = storage::engine() {
        e.search_candidates(b"texts", b"*warm*");
    }
    let reference = api::list_entities(b"texts", 0, 0, b"", true, None, b"", b"");
    for search in [
        "dragon",
        "DUNE",
        "the",
        "ab",
        "*ing",
        "beg*ing",
        "zzz",
        "nïc",
        "a\\*b",
        "dune dragon",
        "on",
    ] {
        let want = ids(&reference
            .iter()
            .filter(|d| api::search_matches_entity(d, search.as_bytes()))
            .cloned()
            .collect::<Vec<_>>());
        let got = ids(&api::list_entities(
            b"texts",
            0,
            0,
            b"",
            true,
            None,
            search.as_bytes(),
            b"",
        ));
        if got != want {
            panic!("search {search:?}: index {got:?}, scan {want:?}");
        }
    }
    api::update_entity_by_id(b"texts", b"s000", &doc(r#"{"title":"zebra crossing"}"#));
    let got = ids(&api::list_entities(
        b"texts", 0, 0, b"", true, None, b"zebra", b"",
    ));
    if got != ["s000"] {
        panic!("an update is searchable at once, got {got:?}");
    }
}

#[test]
fn unfiltered_pages_and_counts_use_the_live_set() {
    let _s = setup();
    let _db = init_test_store();

    let mut list: Vec<Map> = (0..1000)
        .map(|i| obj(vec![("id", s(&format!("p{i:04}")))]))
        .collect();
    api::write_list_of_entities(b"pages", &mut list);
    for i in (0..1000).step_by(3) {
        api::delete_entity_by_id(b"pages", format!("p{i:04}").as_bytes());
    }
    let remaining: Vec<String> = (0..1000)
        .filter(|i| i % 3 != 0)
        .map(|i| format!("p{i:04}"))
        .collect();
    let got = api::count_entities(b"pages");
    if got != remaining.len() {
        panic!("count {got}, want {}", remaining.len());
    }
    for offset in [0usize, 1, 333, 665, 666, 700] {
        let page = ids(&api::list_entities(
            b"pages",
            10,
            offset as i64,
            b"",
            true,
            None,
            b"",
            b"",
        ));
        let end = (offset + 10).min(remaining.len());
        let start = offset.min(remaining.len());
        if page != remaining[start..end] {
            panic!(
                "page at {offset}: {page:?}, want {:?}",
                &remaining[start..end]
            );
        }
    }
    let Some(e) = storage::engine() else {
        panic!("no engine");
    };
    let snap = e.snapshot(b"pages");
    let mut seqs: Vec<u32> = Vec::new();
    snap.ascend(0, |d: Doc| {
        seqs.push(d.seq());
        true
    });
    if !seqs.windows(2).all(|w| w[0] <= w[1]) {
        panic!("the live set iterates in insertion order");
    }
}

#[test]
fn filters_through_references_match_the_scan() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(5));
    let names = ["Ada", "ada lovelace", "Bob", "Chiang", "Dune", "Émile"];
    for i in 0..40 {
        let name = s(names[intn(&mut r, names.len())]);
        let age = Value::Number((20 + r.intn(50)) as f64);
        let city = Value::Object(obj(vec![
            ("@entity", s("city")),
            ("id", s(&format!("c{}", r.intn(5)))),
        ]));
        let mut d = obj(vec![
            ("id", s(&format!("p{i:02}"))),
            ("name", name),
            ("age", age),
            ("city", city),
        ]);
        api::write_entity(b"person", &mut d);
    }
    for (i, label) in ["Paris", "Lyon", "paris", "Oslo", "Rome"]
        .iter()
        .enumerate()
    {
        let mut d = obj(vec![("id", s(&format!("c{i}"))), ("label", s(label))]);
        api::write_entity(b"city", &mut d);
    }
    let mut docs: Vec<Map> = Vec::new();
    for i in 0..300 {
        let title = s(names[intn(&mut r, names.len())]);
        let mut d = obj(vec![("id", s(&format!("b{i:03}"))), ("title", title)]);
        match r.intn(4) {
            0 => {
                let id = s(&format!("p{:02}", r.intn(45)));
                d.insert(
                    "author".to_string(),
                    Value::Object(obj(vec![("@entity", s("person")), ("id", id)])),
                );
            }
            1 => {
                let name = s(names[intn(&mut r, names.len())]);
                let age = Value::Number(r.intn(90) as f64);
                d.insert(
                    "author".to_string(),
                    Value::Object(obj(vec![("name", name), ("age", age)])),
                );
            }
            2 => {
                let a = s(&format!("p{:02}", r.intn(40)));
                let b = s(&format!("p{:02}", r.intn(40)));
                d.insert(
                    "authors".to_string(),
                    Value::Array(vec![
                        Value::Object(obj(vec![("@entity", s("person")), ("id", a)])),
                        Value::Object(obj(vec![("@entity", s("person")), ("id", b)])),
                    ]),
                );
            }
            _ => {}
        }
        docs.push(d);
    }
    api::write_list_of_entities(b"bk", &mut docs);

    let cases: Vec<Filters> = vec![
        filters(&[("author.name", &[("eq", "ada*")])]),
        filters(&[("author.name", &[("contains", "chi")])]),
        filters(&[("author.age", &[("gt", "40")])]),
        filters(&[("authors.name", &[("eq", "Bob")])]),
        filters(&[
            ("authors.age", &[("lte", "30")]),
            ("title", &[("eq", "dune")]),
        ]),
        filters(&[("author.city.label", &[("eq", "paris")])]),
        filters(&[("authors.city.id", &[("eq", "c2")])]),
        filters(&[("author.name", &[("neq", "Bob")])]),
        filters(&[("author.id", &[("eq", "p03")])]),
    ];
    for f in &cases {
        let includes = api::extract_auto_includes(f);
        let all = api::list_entities(b"bk", 0, 0, b"", true, None, b"", &includes);
        let want = ids(&all
            .iter()
            .filter(|d| api::filters_match_entity(d, f))
            .cloned()
            .collect::<Vec<_>>());
        let got = ids(&api::list_entities(
            b"bk",
            0,
            0,
            b"",
            true,
            Some(f),
            b"",
            b"",
        ));
        if got != want {
            panic!("filters {f:?}: planner {got:?}, scan {want:?}");
        }
        let res = api::plan(ListQuery {
            entity: b"bk",
            filters: Some(f),
            includes: &includes,
            count_only: true,
            ..ListQuery::default()
        });
        if res.total != want.len() {
            panic!("filters {f:?}: count {}, scan {}", res.total, want.len());
        }
    }
    api::update_entity_by_id(b"person", b"p03", &doc(r#"{"name":"Zed"}"#));
    let f = filters(&[("author.name", &[("eq", "zed")])]);
    for d in api::list_entities(b"bk", 0, 0, b"", true, Some(&f), b"", b"") {
        let id = match d.get("author") {
            Some(Value::Object(a)) => a.get("id").and_then(Value::as_str).map(str::to_string),
            _ => None,
        };
        if id.as_deref() != Some("p03") {
            panic!("an update of the target is seen through the reference, got {d:?}");
        }
    }
}

#[test]
fn sorted_pages_match_the_query_api_comparator() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(9));
    let values: Vec<Value> = vec![
        Value::Number(1.0),
        Value::Number(2.0),
        Value::Number(2.0),
        s("a"),
        s("b"),
        s("B"),
        Value::Bool(true),
        Value::Bool(false),
        s("2024-01-01"),
        s("2023-06-01T10:00:00+02:00"),
        Value::Object(obj(vec![("x", Value::Number(1.0))])),
        Value::Null,
    ];
    let mut docs: Vec<Map> = Vec::new();
    for i in 0..400 {
        let g = Value::Number(r.intn(4) as f64);
        let mut d = obj(vec![("id", s(&format!("s{i:03}"))), ("g", g)]);
        let v = values[intn(&mut r, values.len())].clone();
        if !v.is_null() || r.intn(2) == 0 {
            d.insert("v".to_string(), v);
        }
        if r.intn(5) > 0 {
            d.insert("w".to_string(), Value::Number(r.intn(10) as f64));
        }
        docs.push(d);
    }
    api::write_list_of_entities(b"sorts", &mut docs);
    let all = api::list_entities(b"sorts", 0, 0, b"", true, None, b"", b"");
    let specs: Vec<Vec<Sort>> = vec![
        vec![Sort::new("v", true)],
        vec![Sort::new("v", false)],
        vec![Sort::new("g", true), Sort::new("w", false)],
        vec![
            Sort::new("g", false),
            Sort::new("v", true),
            Sort::new("w", true),
        ],
        vec![Sort::new("w", true), Sort::new("g", true)],
    ];
    for sorts in &specs {
        let mut want = all.clone();
        api::sort_entities(&mut want, sorts);
        let want_ids = ids(&want);
        for page in [(0i64, 0i64), (0, 10), (37, 25), (390, 50), (500, 10)] {
            let res = api::plan(ListQuery {
                entity: b"sorts",
                sorts: sorts.clone(),
                offset: page.0,
                limit: page.1,
                ..ListQuery::default()
            });
            let start = (page.0 as usize).min(want_ids.len());
            let mut end = want_ids.len();
            if page.1 > 0 {
                end = (start + page.1 as usize).min(want_ids.len());
            }
            let got = res_ids(&res);
            if got != want_ids[start..end] {
                panic!(
                    "sorts {sorts:?} page {page:?}: got {got:?}, want {:?}",
                    &want_ids[start..end]
                );
            }
        }
        let f = filters(&[("g", &[("lt", "3")])]);
        let filtered: Vec<String> = ids(&want
            .iter()
            .filter(|d| api::filters_match_entity(d, &f))
            .cloned()
            .collect::<Vec<_>>());
        let node = FilterNode::with_leaf(f);
        let res = api::plan(ListQuery {
            entity: b"sorts",
            sorts: sorts.clone(),
            node: Some(&node),
            offset: 5,
            limit: 30,
            ..ListQuery::default()
        });
        let end = 35.min(filtered.len());
        let got = res_ids(&res);
        if got != filtered[5..end] {
            panic!(
                "filtered sorts {sorts:?}: got {got:?}, want {:?}",
                &filtered[5..end]
            );
        }
    }
}
