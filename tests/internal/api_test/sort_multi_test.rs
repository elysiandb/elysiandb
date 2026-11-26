use std::collections::BTreeMap;

use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::query::{FilterNode, Query, Sort};

use super::{docs, ids, init_test_store, setup, write};

fn sorts(spec: &[(&str, bool)]) -> Vec<Sort> {
    spec.iter().map(|(f, a)| Sort::new(*f, *a)).collect()
}

fn sort_map(spec: &[(&str, &str)]) -> BTreeMap<Vec<u8>, Vec<u8>> {
    spec.iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
        .collect()
}

#[test]
fn sort_entities_uses_every_key_in_order() {
    let _s = setup();
    let mut data = docs(
        r#"[{"id":"f1","title":"C","year":1983.0},{"id":"f2","title":"B","year":1982.0},{"id":"f3","title":"A","year":1979.0},{"id":"f4","title":"A","year":1983.0}]"#,
    );
    api::sort_entities(&mut data, &sorts(&[("year", false), ("title", true)]));
    if ids(&data) != ["f4", "f1", "f2", "f3"] {
        panic!("year desc, title asc: got {:?}", ids(&data));
    }
    api::sort_entities(&mut data, &sorts(&[("title", true), ("year", true)]));
    if ids(&data) != ["f3", "f4", "f2", "f1"] {
        panic!("title asc, year asc: got {:?}", ids(&data));
    }
}

#[test]
fn sort_entities_dates_nested_fields_and_missing_values() {
    let _s = setup();
    let mut data = docs(
        r#"[{"id":"a","meta":{"at":"2024-05-01"},"rank":1.0},{"id":"b","rank":1.0},{"id":"c","meta":{"at":"2023-01-01T10:00:00Z"},"rank":1.0},{"id":"d","meta":{"at":"2024-05-01"},"rank":0.0}]"#,
    );
    api::sort_entities(&mut data, &sorts(&[("meta.at", true), ("rank", true)]));
    if ids(&data) != ["c", "d", "a", "b"] {
        panic!("asc with missing values last: got {:?}", ids(&data));
    }
    api::sort_entities(&mut data, &sorts(&[("meta.at", false), ("rank", false)]));
    if ids(&data) != ["a", "d", "c", "b"] {
        panic!("desc with missing values last: got {:?}", ids(&data));
    }
}

#[test]
fn sort_entities_mixed_types_are_deterministic() {
    let _s = setup();
    let build = || {
        docs(
            r#"[{"id":"s","v":"x","k":1.0},{"id":"b","v":true,"k":1.0},{"id":"n","v":2.0,"k":1.0},{"id":"d","v":"2024-01-01","k":1.0}]"#,
        )
    };
    let mut first = build();
    api::sort_entities(&mut first, &sorts(&[("v", true), ("k", true)]));
    if ids(&first) != ["n", "d", "s", "b"] {
        panic!(
            "expected numbers, dates, strings then booleans, got {:?}",
            ids(&first)
        );
    }
    for _ in 0..20 {
        let mut again = build();
        api::sort_entities(&mut again, &sorts(&[("v", true), ("k", true)]));
        if ids(&again) != ids(&first) {
            panic!(
                "expected a stable order, got {:?} then {:?}",
                ids(&first),
                ids(&again)
            );
        }
    }
}

#[test]
fn execute_query_with_several_sorts() {
    let _s = setup();
    let _db = init_test_store();
    for d in [
        r#"{"id":"f1","title":"C","year":1983.0}"#,
        r#"{"id":"f2","title":"B","year":1982.0}"#,
        r#"{"id":"f3","title":"A","year":1979.0}"#,
        r#"{"id":"f4","title":"A","year":1983.0}"#,
    ] {
        write("flm", d);
    }
    for _ in 0..10 {
        let (out, err) = api::execute_query(&Query {
            entity: b"flm".to_vec(),
            filter: FilterNode::with_leaf(Default::default()),
            sorts: sort_map(&[("year", "desc"), ("title", "asc")]),
            sort_order: vec![b"year".to_vec(), b"title".to_vec()],
            offset: 1,
            limit: 2,
        });
        if let Some(err) = err {
            panic!("{err}");
        }
        if ids(&out) != ["f1", "f2"] {
            panic!("expected [f1 f2], got {:?}", ids(&out));
        }
    }
}

#[test]
fn ordered_sorts() {
    let _s = setup();
    let q = Query {
        sorts: sort_map(&[("year", "DESC"), ("title", "asc"), ("rank", "whatever")]),
        sort_order: ["year", "missing", "title", "year"]
            .iter()
            .map(|s| s.as_bytes().to_vec())
            .collect(),
        ..Query::default()
    };
    let want = sorts(&[("year", false), ("title", true), ("rank", true)]);
    if q.ordered_sorts() != want {
        panic!("expected {want:?}, got {:?}", q.ordered_sorts());
    }
    let unordered = Query {
        sorts: sort_map(&[("b", "asc"), ("a", "desc")]),
        ..Query::default()
    };
    if unordered.ordered_sorts() != sorts(&[("a", false), ("b", true)]) {
        panic!(
            "expected alphabetical order without an explicit order, got {:?}",
            unordered.ordered_sorts()
        );
    }
    if !Query::default().ordered_sorts().is_empty() {
        panic!(
            "expected no sort, got {:?}",
            Query::default().ordered_sorts()
        );
    }
}

#[test]
fn sort_entities_ties_missing_values_and_other_types() {
    let _s = setup();
    let mut data = docs(
        r#"[{"id":"x","k":2.0},{"id":"y","k":1.0},{"id":"z","v":1.0,"k":1.0},{"id":"same1","v":1.0,"k":1.0}]"#,
    );
    api::sort_entities(&mut data, &sorts(&[("v", true), ("k", true)]));
    if ids(&data) != ["z", "same1", "y", "x"] {
        panic!(
            "expected ties to keep their order and missing values to sort by the next key, got {:?}",
            ids(&data)
        );
    }

    let mut flags =
        docs(r#"[{"id":"t","flag":true},{"id":"f","flag":false},{"id":"t2","flag":true}]"#);
    api::sort_entities(&mut flags, &sorts(&[("flag", true), ("id", false)]));
    if ids(&flags) != ["f", "t2", "t"] {
        panic!("expected false before true, got {:?}", ids(&flags));
    }

    let mut values =
        docs(r#"[{"id":"obj","v":{"a":1.0}},{"id":"arr","v":[1.0]},{"id":"bool","v":false}]"#);
    api::sort_entities(&mut values, &sorts(&[("v", true), ("id", true)]));
    if ids(&values) != ["bool", "arr", "obj"] {
        panic!(
            "expected objects and arrays after scalar values, got {:?}",
            ids(&values)
        );
    }
}

#[test]
fn sort_entities_go_number_types() {
    let _s = setup();
    let entry = |id: &str, n: Value| {
        let mut m = Map::new();
        m.insert("id".to_string(), Value::String(id.to_string()));
        m.insert("n".to_string(), n);
        m
    };
    let mut data = vec![
        entry("i64", Value::Int(5)),
        entry("f32", Value::Number(1.5)),
        entry("i32", Value::Int(3)),
        entry("int", Value::Int(2)),
        entry("f64", Value::Number(4.0)),
    ];
    api::sort_entities(&mut data, &sorts(&[("n", true), ("id", true)]));
    if ids(&data) != ["f32", "int", "i32", "f64", "i64"] {
        panic!(
            "expected numeric order across Go number types, got {:?}",
            ids(&data)
        );
    }
}

#[test]
fn primary_sort() {
    let _s = setup();
    let (field, ascending) = Query {
        sorts: sort_map(&[("year", "desc"), ("title", "asc")]),
        sort_order: vec![b"year".to_vec(), b"title".to_vec()],
        ..Query::default()
    }
    .primary_sort();
    if field != b"year" || ascending {
        panic!(
            "expected year desc, got {} {ascending}",
            String::from_utf8_lossy(&field)
        );
    }
    let (field, ascending) = Query::default().primary_sort();
    if !field.is_empty() || !ascending {
        panic!(
            "expected no sort, got {} {ascending}",
            String::from_utf8_lossy(&field)
        );
    }
}
