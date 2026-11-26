use elysiandb::api;
use elysiandb::gocompat::json::Map;
use elysiandb::query::FilterNode;

use super::{doc, docs, filters, ids, init_test_store, setup, write};

fn matches(entity: &Map, field: &str, op: &str, value: &str) -> bool {
    api::filters_match_entity(entity, &filters(&[(field, &[(op, value)])]))
}

#[test]
fn filters_match_entity_string_comparisons() {
    let _s = setup();
    let entity = doc(r#"{"label":"banana"}"#);
    let cases: [(&str, &str, bool); 17] = [
        ("lt", "b", false),
        ("lt", "c", true),
        ("lte", "banana", true),
        ("lte", "apple", false),
        ("gt", "apple", true),
        ("gt", "zzz", false),
        ("gte", "banana", true),
        ("contains", "nan", true),
        ("contains", "NAN", true),
        ("contains", "kiwi", false),
        ("not_contains", "nan", false),
        ("not_contains", "kiwi", true),
        ("eq", "ban*", true),
        ("neq", "ban*", false),
        ("any", "banana", false),
        ("all", "banana", false),
        ("unknown", "banana", false),
    ];
    for (op, value, want) in cases {
        let got = matches(&entity, "label", op, value);
        if got != want {
            panic!("label {op} {value:?}: expected {want}, got {got}");
        }
    }
}

#[test]
fn filters_match_entity_dates_still_compare_as_dates() {
    let _s = setup();
    let entity = doc(r#"{"day":"2024-05-01"}"#);
    if !matches(&entity, "day", "lt", "2024-06-01") || matches(&entity, "day", "gt", "2024-06-01") {
        panic!("expected date comparisons");
    }
    if matches(&entity, "day", "lte", "2024-04-30") || !matches(&entity, "day", "lte", "2024-05-01")
    {
        panic!("expected lte to compare dates");
    }
    if !matches(&entity, "day", "contains", "-05-") {
        panic!("expected contains to apply to the date string");
    }
    if !matches(&entity, "day", "lt", "notadate") {
        panic!("expected a comparison with a non-date value to be lexicographic");
    }
    let both = filters(&[("day", &[("gte", "2024-01-01"), ("lt", "notadate")])]);
    if !api::filters_match_entity(&entity, &both) {
        panic!("expected mixed date and non-date values to compare as strings");
    }
}

#[test]
fn filters_match_entity_array_contains_does_not_split_values() {
    let _s = setup();
    let entity = doc(r#"{"tags":["Smith, John","x"]}"#);
    if !matches(&entity, "tags", "contains", "Smith, John") {
        panic!("expected the whole value to be looked up");
    }
    if matches(&entity, "tags", "contains", "Smith") {
        panic!("expected no partial match on array items");
    }
    if matches(&entity, "tags", "contains", "x,zzz") {
        panic!("expected the value not to be split");
    }
    if !matches(&entity, "tags", "not_contains", "x,zzz")
        || matches(&entity, "tags", "not_contains", "x")
    {
        panic!("unexpected not_contains result");
    }
    if !matches(&entity, "tags", "any", "x,zzz")
        || matches(&entity, "tags", "all", "x,zzz")
        || !matches(&entity, "tags", "none", "a,b")
    {
        panic!("expected any, all and none to keep splitting on commas");
    }
    if matches(&entity, "tags", "lt", "z") {
        panic!("expected an unsupported array operator not to match");
    }
}

#[test]
fn filters_match_entity_unsupported_operators_never_match() {
    let _s = setup();
    let entity = doc(r#"{"n":3.0,"ok":true}"#);
    for op in ["contains", "any", "unknown"] {
        if matches(&entity, "n", op, "3") {
            panic!("number {op}: expected no match");
        }
        if matches(&entity, "ok", op, "true") {
            panic!("boolean {op}: expected no match");
        }
    }
    if !matches(&entity, "n", "gte", "3")
        || !matches(&entity, "ok", "eq", "true")
        || !matches(&entity, "ok", "neq", "false")
    {
        panic!("expected supported operators to keep working");
    }
}

#[test]
fn apply_query_filter_uses_the_same_operators() {
    let _s = setup();
    let data = docs(r#"[{"id":"t1","label":"banana"},{"id":"t2","label":"apple"}]"#);
    let out = api::apply_query_filter(
        data,
        &FilterNode::with_leaf(filters(&[("label", &[("lt", "b")])])),
    );
    if ids(&out) != ["t2"] {
        panic!("expected only apple, got {out:?}");
    }
}

#[test]
fn search_pattern() {
    let _s = setup();
    let cases = [
        ("Zebra", "*Zebra*"),
        ("*Zebra*", "*Zebra*"),
        ("Zeb*", "Zeb*"),
        ("", "**"),
    ];
    for (input, want) in cases {
        let got = api::search_pattern(input.as_bytes());
        if got != want.as_bytes() {
            panic!(
                "SearchPattern({input:?}): expected {want:?}, got {:?}",
                String::from_utf8_lossy(&got)
            );
        }
    }
    let entity = doc(r#"{"title":"Zebra crossing"}"#);
    if !api::search_matches_entity(&entity, b"zebra") {
        panic!("expected a plain search to match a substring");
    }
    if !api::search_matches_entity(&entity, b"Zeb") {
        panic!("expected a prefix to match without a glob");
    }
    if api::search_matches_entity(&entity, b"crossing*") {
        panic!("expected an explicit glob to keep its meaning");
    }
}

#[test]
fn list_entities_search_and_filters_before_pagination() {
    let _s = setup();
    let _db = init_test_store();
    for d in [
        r#"{"id":"n1","title":"Note 1","kind":"k1"}"#,
        r#"{"id":"n2","title":"Note 2","kind":"k0"}"#,
        r#"{"id":"n3","title":"Note 3","kind":"k1"}"#,
        r#"{"id":"n4","title":"Note 4","kind":"k0"}"#,
        r#"{"id":"n5","title":"Zebra crossing","kind":"k1"}"#,
    ] {
        write("note", d);
    }

    let got = ids(&api::list_entities(
        b"note", 2, 0, b"", true, None, b"Zebra", b"",
    ));
    if got != ["n5"] {
        panic!("expected the match beyond the first page, got {got:?}");
    }
    let f = filters(&[("kind", &[("eq", "k0")])]);
    let got = ids(&api::list_entities(
        b"note",
        0,
        0,
        b"id",
        true,
        Some(&f),
        b"Note",
        b"",
    ));
    if got != ["n2", "n4"] {
        panic!("expected search and filters to combine, got {got:?}");
    }
    let got = ids(&api::list_entities(
        b"note",
        1,
        1,
        b"id",
        true,
        Some(&f),
        b"Note",
        b"",
    ));
    if got != ["n4"] {
        panic!("expected pagination after matching, got {got:?}");
    }
    let got = api::list_entities(b"note", 2, 0, b"", true, None, b"", b"");
    if got.len() != 2 {
        panic!("expected plain pagination to keep working, got {got:?}");
    }
}
