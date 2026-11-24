use std::collections::BTreeMap;

use elysiandb::api;
use elysiandb::gocompat::json::Value;
use elysiandb::query::{FilterNode, Query};

use super::{Spec, doc, docs, filters, ids, init_test_store, setup, write};

fn leaf(spec: Spec<'_>) -> FilterNode {
    FilterNode::with_leaf(filters(spec))
}

#[test]
fn test_apply_query_filter_empty_node_matches_nothing() {
    let _s = setup();
    let data = docs(r#"[{"id":"1","title":"abc"}]"#);
    let out = api::apply_query_filter(data, &FilterNode::default());
    if !out.is_empty() {
        panic!("expected empty result, got {out:?}");
    }
}

#[test]
fn test_apply_query_filter_primitives_and_nested_arrays() {
    let _s = setup();
    let data = docs(
        r#"[{"id":"1","title":"abcXYZdef","price":10.0,"flag":true,"tags":["Go","kv"],"author":{"name":"Taymour Negib"},"categories":[{"title":"coco"},{"title":"caca"}]},{"id":"2","title":"nope"}]"#,
    );
    let f = FilterNode::with_and(vec![
        leaf(&[("price", &[("gte", "9")])]),
        leaf(&[("flag", &[("eq", "true")])]),
        leaf(&[("tags", &[("contains", "Go")])]),
        leaf(&[("author.name", &[("eq", "Taymour*")])]),
        leaf(&[("categories.title", &[("eq", "coco")])]),
    ]);
    let out = api::apply_query_filter(data, &f);
    if ids(&out) != ["1"] {
        panic!("expected only id=1, got {out:?}");
    }
}

#[test]
fn test_apply_query_filter_glob_strict_match_branches() {
    let _s = setup();
    let data = docs(r#"[{"id":"1","title":"abcXYZdef"}]"#);
    let cases: [(&str, &str, &str, usize); 9] = [
        ("star matches all", "eq", "*", 1),
        ("exact match no star", "eq", "abcXYZdef", 1),
        ("prefix and suffix match", "eq", "abc*def", 1),
        ("contains with leading trailing star", "eq", "*XYZ*", 1),
        ("multiple stars with empty parts", "eq", "**XYZ**", 1),
        ("must end when no trailing star", "eq", "*XYZ", 0),
        ("prefix mismatch", "eq", "zzz*def", 0),
        ("neq rejects when pattern matches", "neq", "*XYZ*", 0),
        ("neq passes when pattern does not match", "neq", "*NOPE*", 1),
    ];
    for (name, op, cmp, want) in cases {
        let out = api::apply_query_filter(data.clone(), &leaf(&[("title", &[(op, cmp)])]));
        if out.len() != want {
            panic!(
                "{name}: expected {want} results, got {} ({out:?})",
                out.len()
            );
        }
    }
}

#[test]
fn test_apply_query_filter_or_and_nodes() {
    let _s = setup();
    let data = docs(
        r#"[{"id":"1","title":"Go","status":"published"},{"id":"2","title":"Python","status":"draft"}]"#,
    );
    let f = FilterNode::with_or(vec![
        leaf(&[("title", &[("eq", "Go")])]),
        leaf(&[("title", &[("eq", "Rust")])]),
    ]);
    let out = api::apply_query_filter(data, &f);
    if ids(&out) != ["1"] {
        panic!("expected only id=1, got {out:?}");
    }
}

#[test]
fn test_apply_query_filter_unsupported_value_type_fails() {
    let _s = setup();
    let data = docs(r#"[{"id":"1","meta":{"a":1}}]"#);
    let out = api::apply_query_filter(data, &leaf(&[("meta", &[("eq", "*")])]));
    if !out.is_empty() {
        panic!("expected empty result, got {out:?}");
    }
}

#[test]
fn test_execute_query_with_sort_offset_limit() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "articles",
        r#"{"id":"a1","status":"published","title":"Go"}"#,
    );
    write(
        "articles",
        r#"{"id":"a2","status":"published","title":"Rust"}"#,
    );
    write(
        "articles",
        r#"{"id":"a3","status":"published","title":"Python"}"#,
    );

    let q = Query {
        entity: b"articles".to_vec(),
        offset: 1,
        limit: 1,
        sorts: BTreeMap::from([(b"title".to_vec(), b"asc".to_vec())]),
        filter: leaf(&[("status", &[("eq", "published")])]),
        ..Query::default()
    };
    let (out, err) = api::execute_query(&q);
    if let Some(err) = err {
        panic!("unexpected error: {err}");
    }
    if out.len() != 1 {
        panic!("expected 1 result, got {out:?}");
    }
    if ids(&out) != ["a3"] {
        panic!(
            "expected id=a3 after sort+offset, got {:?}",
            out[0].get("id")
        );
    }
}

#[test]
fn test_execute_query_complex_nested_query() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "article",
        r#"{"id":"x1","status":"published","title":"Why Go is the ideal language","excerpt":"Go is well suited to distributed systems","author":{"id":"u789","name":"Taymour Negib"},"tags":["Go","distributed"],"categories":[{"title":"coco"},{"title":"caca"}]}"#,
    );

    let q = Query {
        entity: b"article".to_vec(),
        filter: FilterNode::with_and(vec![
            leaf(&[("status", &[("eq", "published")])]),
            FilterNode::with_or(vec![
                leaf(&[("title", &[("eq", "*Go*")])]),
                leaf(&[("excerpt", &[("eq", "*distribution*")])]),
            ]),
        ]),
        ..Query::default()
    };
    let (out, err) = api::execute_query(&q);
    if let Some(err) = err {
        panic!("unexpected error: {err}");
    }
    if out.len() != 1 {
        panic!("expected 1 result, got {out:?}");
    }
    if ids(&out) != ["x1"] {
        panic!("expected id x1, got {:?}", out[0].get("id"));
    }
    let want_author = Value::Object(doc(r#"{"id":"u789","name":"Taymour Negib"}"#));
    if out[0].get("author") != Some(&want_author) {
        panic!("author mismatch: got {:?}", out[0].get("author"));
    }
}
