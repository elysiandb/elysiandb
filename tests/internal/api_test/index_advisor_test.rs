use std::collections::{BTreeMap, BTreeSet};

use elysiandb::api::{self, ListQuery};
use elysiandb::datastore::composite_spec;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::query::{FilterNode, Query, Sort};
use elysiandb::storage;

use super::index_test::{obj, res_ids, s};
use super::{filters, ids, init_test_store, setup};

fn index_kinds(entity: &str) -> BTreeSet<String> {
    storage::engine()
        .and_then(|e| e.index_names(entity.as_bytes()))
        .unwrap_or_default()
        .into_iter()
        .map(|n| String::from_utf8_lossy(&n).into_owned())
        .collect()
}

#[test]
fn test_the_advisor_turns_low_cardinality_filters_into_bitmaps() {
    let _s = setup();
    let _db = init_test_store();

    let mut docs: Vec<Map> = (0..6000)
        .map(|i| {
            obj(vec![
                ("id", s(&format!("a{i:05}"))),
                ("status", s(["open", "closed", "Draft"][i % 3])),
                ("code", s(&format!("c{i:05}"))),
            ])
        })
        .collect();
    api::write_list_of_entities(b"adv", &mut docs);
    let f = filters(&[("status", &[("eq", "draft")])]);
    let before = ids(&api::list_entities(
        b"adv",
        0,
        0,
        b"",
        true,
        Some(&f),
        b"",
        b"",
    ));
    let code = filters(&[("code", &[("eq", "c00042")])]);
    api::list_entities(b"adv", 0, 0, b"", true, Some(&code), b"", b"");
    let Some(e) = storage::engine() else {
        panic!("no engine");
    };
    e.advise();
    e.advise();
    let kinds = index_kinds("adv");
    if !kinds.contains("b:status") || kinds.contains("f:status") {
        panic!("a field with three values should end up as a bitmap only, got {kinds:?}");
    }
    if !kinds.contains("f:code") || kinds.contains("b:code") {
        panic!("a field with distinct values stays ordered, got {kinds:?}");
    }
    let after = ids(&api::list_entities(
        b"adv",
        0,
        0,
        b"",
        true,
        Some(&f),
        b"",
        b"",
    ));
    if before != after || after.len() != 2000 {
        panic!(
            "the bitmap answers like the ordered index: {} before, {} after",
            before.len(),
            after.len()
        );
    }
    for (i, d) in docs.iter_mut().enumerate() {
        d.insert("status".to_string(), s(&format!("s{i:05}")));
    }
    api::write_list_of_entities(b"adv", &mut docs);
    e.advise();
    e.advise();
    let kinds = index_kinds("adv");
    if !kinds.contains("f:status") || kinds.contains("b:status") {
        panic!("a field whose values became distinct goes back to an ordered index, got {kinds:?}");
    }
    let f = filters(&[("status", &[("eq", "s00007")])]);
    let got = ids(&api::list_entities(
        b"adv",
        0,
        0,
        b"",
        true,
        Some(&f),
        b"",
        b"",
    ));
    if got != ["a00007"] {
        panic!("filter after the switch back: {got:?}");
    }
}

fn make_doc(r: &mut Rand, i: i64) -> Map {
    let kinds = ["film", "Serie", "doc", "Film"];
    let kind = s(kinds[r.intn(kinds.len() as i64) as usize]);
    let mut d = obj(vec![("id", s(&format!("w{i:05}"))), ("kind", kind)]);
    match r.intn(5) {
        0 => {}
        1 => {
            d.insert("rank".to_string(), Value::Number(r.intn(50) as f64));
        }
        2 => {
            d.insert("rank".to_string(), s(&format!("t{:02}", r.intn(30))));
        }
        _ => {
            d.insert("rank".to_string(), Value::Number(r.intn(20) as f64));
        }
    }
    d
}

fn check_composite(label: &str) {
    let all = api::list_entities(b"cmp", 0, 0, b"", true, None, b"", b"");
    for kind in ["film", "FILM", "serie", "doc", "none"] {
        for asc in [true, false] {
            let f = filters(&[("kind", &[("eq", kind)])]);
            let mut want: Vec<Map> = all
                .iter()
                .filter(|d| api::filters_match_entity(d, &f))
                .cloned()
                .collect();
            api::sort_entities(&mut want, &[Sort::new("rank", asc)]);
            let want_ids = ids(&want);
            let n = want_ids.len() as i64;
            for page in [(0i64, 0i64), (0, 20), (37, 25), (n - 3, 10), (n + 5, 10)] {
                let res = api::plan(ListQuery {
                    entity: b"cmp",
                    filters: Some(&f),
                    sort_field: b"rank",
                    sort_asc: asc,
                    offset: page.0,
                    limit: page.1,
                    ..ListQuery::default()
                });
                let start = (page.0.max(0) as usize).min(want_ids.len());
                let mut end = want_ids.len();
                if page.1 > 0 {
                    end = (start + page.1 as usize).min(want_ids.len());
                }
                let got = res_ids(&res);
                if got != want_ids[start..end] {
                    panic!(
                        "{label}: kind {kind} asc {asc} page {page:?}: got {got:?}, want {:?}",
                        &want_ids[start..end]
                    );
                }
            }
            let count = api::plan(ListQuery {
                entity: b"cmp",
                filters: Some(&f),
                sort_field: b"rank",
                sort_asc: asc,
                count_only: true,
                ..ListQuery::default()
            });
            if count.total != want_ids.len() {
                panic!(
                    "{label}: count for {kind} = {}, want {}",
                    count.total,
                    want_ids.len()
                );
            }
            let direction: &[u8] = if asc { b"asc" } else { b"desc" };
            let q = Query {
                entity: b"cmp".to_vec(),
                filter: FilterNode::with_leaf(f.clone()),
                sorts: BTreeMap::from([(b"rank".to_vec(), direction.to_vec())]),
                offset: 11,
                limit: 7,
                ..Query::default()
            };
            let qres = api::plan_query(&q, false);
            let end = 18.min(want_ids.len());
            let start = 11.min(end);
            let got = res_ids(&qres);
            if got != want_ids[start..end] {
                panic!(
                    "{label}: query API kind {kind} asc {asc}: got {got:?}, want {:?}",
                    &want_ids[start..end]
                );
            }
        }
    }
}

#[test]
fn test_composite_indexes_page_like_the_sort_index() {
    let _s = setup();
    let _db = init_test_store();

    let mut r = Rand::new(rand::new_source(3));
    let mut docs: Vec<Map> = (0..5000).map(|i| make_doc(&mut r, i)).collect();
    api::write_list_of_entities(b"cmp", &mut docs);

    check_composite("without composite");
    let Some(e) = storage::engine() else {
        panic!("no engine");
    };
    if !e.ensure_index(b"cmp", &composite_spec(b"kind", b"rank")) {
        panic!("composite not built");
    }
    check_composite("with composite");
    let mut uses = 0;
    for st in e.index_states() {
        if st.entity == b"cmp" && st.index == b"c:kind,rank" {
            uses = st.uses;
        }
    }
    if uses < 40 {
        panic!("the pages should be read from the composite index, used {uses} times");
    }
    for _ in 0..2000 {
        let n = r.intn(5000);
        if r.intn(4) == 0 {
            api::delete_entity_by_id(b"cmp", format!("w{n:05}").as_bytes());
            continue;
        }
        let mut d = make_doc(&mut r, n);
        api::write_entity(b"cmp", &mut d);
    }
    check_composite("after writes");
    let mut odd = obj(vec![
        ("id", s("odd")),
        ("kind", Value::Bool(true)),
        ("rank", Value::Number(1.0)),
    ]);
    api::write_entity(b"cmp", &mut odd);
    check_composite("with an irregular document");
    api::delete_entity_by_id(b"cmp", b"odd");
    check_composite("after the irregular document left");
}

#[test]
fn test_query_shapes_create_composite_indexes() {
    let _s = setup();
    let _db = init_test_store();

    let mut docs: Vec<Map> = (0..5000)
        .map(|i| {
            obj(vec![
                ("id", s(&format!("v{i:05}"))),
                ("lang", s(["vo", "vf"][i % 2])),
                ("day", s(&format!("d{:05}", 5000 - i))),
            ])
        })
        .collect();
    api::write_list_of_entities(b"shape", &mut docs);
    let f = filters(&[("lang", &[("eq", "vo")])]);
    for i in 0..10 {
        api::list_entities(b"shape", 10, i * 10, b"day", true, Some(&f), b"", b"");
    }
    if !index_kinds("shape").contains("c:lang,day") {
        panic!(
            "a repeated filter and sort shape should get a composite index, got {:?}",
            index_kinds("shape")
        );
    }
    let f = filters(&[("lang", &[("eq", "VO")])]);
    let got = ids(&api::list_entities(
        b"shape",
        3,
        2,
        b"day",
        true,
        Some(&f),
        b"",
        b"",
    ));
    let want = ["v04994", "v04992", "v04990"];
    if got.join(",") != want.join(",") {
        panic!("composite page {got:?}, want {want:?}");
    }
}
