use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::gocompat::time::{self, Duration, Location, Month, Time};
use elysiandb::query::FilterNode;

use super::{Spec, doc, doc_list, docs, filters, setup};

fn matches(entity: &Map, spec: &[(&str, &[(&str, &str)])]) -> bool {
    api::filters_match_entity(entity, &filters(spec))
}

#[test]
fn test_search_matches_entity_simple() {
    let _s = setup();
    let entity = doc(r#"{"name":"Alice"}"#);
    if !api::search_matches_entity(&entity, b"Ali*") {
        panic!("expected simple glob match");
    }
    if api::search_matches_entity(&entity, b"Bob*") {
        panic!("expected simple glob fail");
    }
}

#[test]
fn test_search_matches_entity_nested_map() {
    let _s = setup();
    let entity = doc(r#"{"user":{"profile":{"city":"Paris"}}}"#);
    if !api::search_matches_entity(&entity, b"Par*") {
        panic!("expected nested match");
    }
    if api::search_matches_entity(&entity, b"Lon*") {
        panic!("expected nested fail");
    }
}

#[test]
fn test_search_matches_entity_nested_array() {
    let _s = setup();
    let entity = doc(r#"{"tags":["blue","green","red"]}"#);
    if !api::search_matches_entity(&entity, b"gr*") {
        panic!("expected array match");
    }
    if api::search_matches_entity(&entity, b"ye*") {
        panic!("expected no match");
    }
}

#[test]
fn test_search_matches_entity_mixed_structures() {
    let _s = setup();
    let entity = doc(r#"{"data":[{"info":["hello",{"deep":"world"}]}]}"#);
    if !api::search_matches_entity(&entity, b"wor*") {
        panic!("expected deep match");
    }
    if api::search_matches_entity(&entity, b"xxx") {
        panic!("expected deep fail");
    }
}

#[test]
fn test_search_matches_entity_no_string_values() {
    let _s = setup();
    let entity = doc(r#"{"nums":[1,2,3],"obj":{"flag":false}}"#);
    if api::search_matches_entity(&entity, b"*") {
        panic!("expected no match with no string values");
    }
}

#[test]
fn test_search_matches_entity_skips_ids_markers_and_core_fields() {
    let _s = setup();
    let entity = doc(
        r#"{"id":"m1","_elysiandb_core_username":"admin","title":"Blade Runner","directors":[{"@entity":"director","id":"ridley-scott"}]}"#,
    );

    for search in ["m1", "admin", "director", "scott", "*admin*"] {
        if api::search_matches_entity(&entity, search.as_bytes()) {
            panic!("expected {search:?} not to match ids, markers or core fields");
        }
    }

    if !api::search_matches_entity(&entity, b"runner") {
        panic!("expected content fields to stay searchable");
    }
}

#[test]
fn test_search_matches_entity_included_document_content() {
    let _s = setup();
    let entity = doc(
        r#"{"id":"f1","title":"Blade Runner","directors":[{"@entity":"director","id":"ridley-scott","_elysiandb_core_username":"admin","name":"Ridley Scott"}]}"#,
    );

    if !api::search_matches_entity(&entity, b"scott") {
        panic!("expected the content of an included document to be searchable");
    }

    for search in ["ridley-scott", "admin", "director"] {
        if api::search_matches_entity(&entity, search.as_bytes()) {
            panic!(
                "expected {search:?} not to match the id, owner or marker of an included document"
            );
        }
    }
}

#[test]
fn test_search_matches_entity_included_list() {
    let _s = setup();
    let mut entity = doc(r#"{"title":"Blade Runner"}"#);
    entity.insert(
        "directors".to_string(),
        doc_list(r#"[{"@entity":"director","id":"ridley-scott","name":"Ridley Scott"}]"#),
    );

    if !api::search_matches_entity(&entity, b"scott") {
        panic!("expected the content of an included list to be searchable");
    }

    if api::search_matches_entity(&entity, b"ridley-scott") {
        panic!("expected the ids of an included list to be skipped");
    }
}

#[test]
fn test_get_nested_value() {
    let _s = setup();
    let data =
        doc(r#"{"user":{"name":"Alice","info":{"age":30.0,"date":"2025-09-27T10:00:00Z"}}}"#);
    match api::get_nested_value(&data, b"user.name") {
        Some(Value::String(s)) if s == "Alice" => {}
        other => panic!("expected Alice, got {other:?}"),
    }
    match api::get_nested_value(&data, b"user.info.age") {
        Some(Value::Number(n)) if *n == 30.0 => {}
        other => panic!("expected 30, got {other:?}"),
    }
    match api::get_nested_value(&data, b"user.info.date") {
        Some(Value::String(s)) if s == "2025-09-27T10:00:00Z" => {}
        other => panic!("expected date string, got {other:?}"),
    }
    if api::get_nested_value(&data, b"user.unknown").is_some() {
        panic!("expected false for missing field");
    }
    if api::get_nested_value(&data, b"user.info.age.year").is_some() {
        panic!("expected false for invalid deep path");
    }
}

#[test]
fn test_filters_match_entity_string_glob() {
    let _s = setup();
    let entity = doc(r#"{"name":"Alice"}"#);
    if !matches(&entity, &[("name", &[("eq", "Ali*")])]) {
        panic!("expected match for glob");
    }
    if matches(&entity, &[("name", &[("neq", "Alice")])]) {
        panic!("expected not match for neq");
    }
}

#[test]
fn test_filters_match_entity_float_ops() {
    let _s = setup();
    let entity = doc(r#"{"age":30.0}"#);
    if !matches(&entity, &[("age", &[("eq", "30")])]) {
        panic!("expected eq match");
    }
    if matches(&entity, &[("age", &[("neq", "30")])]) {
        panic!("expected neq fail");
    }
    if !matches(&entity, &[("age", &[("lt", "40")])]) {
        panic!("expected lt match");
    }
    if !matches(&entity, &[("age", &[("lte", "30")])]) {
        panic!("expected lte match");
    }
    if !matches(&entity, &[("age", &[("gt", "20")])]) {
        panic!("expected gt match");
    }
    if !matches(&entity, &[("age", &[("gte", "30")])]) {
        panic!("expected gte match");
    }
}

#[test]
fn test_filters_match_entity_date_ops() {
    let _s = setup();
    let now = Time::now().utc();
    let now_text = now.format(time::RFC3339);
    let mut entity = Map::new();
    entity.insert("createdAt".to_string(), Value::String(now_text.clone()));
    let before = now.add(Duration(-Duration::HOUR.0)).format(time::RFC3339);
    let after = now.add(Duration::HOUR).format(time::RFC3339);

    if !matches(&entity, &[("createdAt", &[("eq", &now_text)])]) {
        panic!("expected eq date match");
    }
    if matches(&entity, &[("createdAt", &[("neq", &now_text)])]) {
        panic!("expected neq fail on same date");
    }
    if !matches(&entity, &[("createdAt", &[("lt", &after)])]) {
        panic!("expected lt match");
    }
    if !matches(&entity, &[("createdAt", &[("lte", &now_text)])]) {
        panic!("expected lte match");
    }
    if !matches(&entity, &[("createdAt", &[("gt", &before)])]) {
        panic!("expected gt match");
    }
    if !matches(&entity, &[("createdAt", &[("gte", &now_text)])]) {
        panic!("expected gte match");
    }
}

#[test]
fn test_filters_match_entity_date_only_against_date_time() {
    let _s = setup();
    let entity = doc(r#"{"createdAt":"2023-05-10T15:04:05Z"}"#);
    if !matches(&entity, &[("createdAt", &[("eq", "2023-05-10")])]) {
        panic!("expected eq match with date-only vs datetime");
    }
    if matches(&entity, &[("createdAt", &[("neq", "2023-05-10")])]) {
        panic!("expected neq fail with same date-only");
    }
    if !matches(&entity, &[("createdAt", &[("lt", "2023-05-11")])]) {
        panic!("expected lt match with date-only > datetime date");
    }
    if !matches(&entity, &[("createdAt", &[("gt", "2023-05-09")])]) {
        panic!("expected gt match with date-only < datetime date");
    }
    if !matches(&entity, &[("createdAt", &[("lte", "2023-05-10")])]) {
        panic!("expected lte match on same date-only");
    }
    if !matches(&entity, &[("createdAt", &[("gte", "2023-05-10")])]) {
        panic!("expected gte match on same date-only");
    }
}

#[test]
fn test_filters_match_entity_date_only_ops() {
    let _s = setup();
    let today = Time::from_date(2023, Month::MAY, 10, 0, 0, 0, 0, &Location::utc());
    let today_text = today.format("2006-01-02");
    let mut entity = Map::new();
    entity.insert("createdAt".to_string(), Value::String(today_text.clone()));
    let before = today.add_date(0, 0, -1).format("2006-01-02");
    let after = today.add_date(0, 0, 1).format("2006-01-02");

    if !matches(&entity, &[("createdAt", &[("eq", &today_text)])]) {
        panic!("expected eq date-only match");
    }
    if matches(&entity, &[("createdAt", &[("neq", &today_text)])]) {
        panic!("expected neq fail on same date-only");
    }
    if !matches(&entity, &[("createdAt", &[("lt", &after)])]) {
        panic!("expected lt date-only match");
    }
    if !matches(&entity, &[("createdAt", &[("lte", &today_text)])]) {
        panic!("expected lte date-only match");
    }
    if !matches(&entity, &[("createdAt", &[("gt", &before)])]) {
        panic!("expected gt date-only match");
    }
    if !matches(&entity, &[("createdAt", &[("gte", &today_text)])]) {
        panic!("expected gte date-only match");
    }
}

#[test]
fn test_filters_match_entity_array_contains() {
    let _s = setup();
    let entity = doc(r#"{"tags":["toto","tata","titi"]}"#);
    if !matches(&entity, &[("tags", &[("contains", "toto")])]) {
        panic!("expected contains match");
    }
    if matches(&entity, &[("tags", &[("contains", "tutu")])]) {
        panic!("expected contains fail");
    }
}

#[test]
fn test_filters_match_entity_array_not_contains() {
    let _s = setup();
    let entity = doc(r#"{"tags":["toto","tata","titi"]}"#);
    if !matches(&entity, &[("tags", &[("not_contains", "tutu")])]) {
        panic!("expected not_contains match");
    }
    if matches(&entity, &[("tags", &[("not_contains", "toto")])]) {
        panic!("expected not_contains fail");
    }
}

#[test]
fn test_filters_match_entity_array_all() {
    let _s = setup();
    let entity = doc(r#"{"tags":["toto","tata","titi"]}"#);
    if !matches(&entity, &[("tags", &[("all", "toto,tata")])]) {
        panic!("expected all match");
    }
    if matches(&entity, &[("tags", &[("all", "toto,tutu")])]) {
        panic!("expected all fail");
    }
}

#[test]
fn test_filters_match_entity_array_any() {
    let _s = setup();
    let entity = doc(r#"{"tags":["toto","tata","titi"]}"#);
    if !matches(&entity, &[("tags", &[("any", "tutu,tata")])]) {
        panic!("expected any match");
    }
    if matches(&entity, &[("tags", &[("any", "tutu,tete")])]) {
        panic!("expected any fail");
    }
}

#[test]
fn test_filters_match_entity_array_eq() {
    let _s = setup();
    let entity = doc(r#"{"tags":["toto","tata","titi"]}"#);
    if !matches(&entity, &[("tags", &[("eq", "toto,tata,titi")])]) {
        panic!("expected eq match");
    }
    if matches(&entity, &[("tags", &[("eq", "toto,tata")])]) {
        panic!("expected eq fail length mismatch");
    }
    if !matches(&entity, &[("tags", &[("eq", "toto,titi,tata")])]) {
        panic!("expected eq match ignoring order");
    }
}

#[test]
fn test_filters_match_entity_array_none() {
    let _s = setup();
    let entity = doc(r#"{"tags":["toto","tata","titi"]}"#);
    if !matches(&entity, &[("tags", &[("none", "tutu,tete")])]) {
        panic!("expected none match");
    }
    if matches(&entity, &[("tags", &[("none", "tata,tete")])]) {
        panic!("expected none fail");
    }
}

#[test]
fn test_filters_match_entity_array_mixed_types() {
    let _s = setup();
    let entity = doc(r#"{"values":["1",2.0,"three"]}"#);
    if !matches(&entity, &[("values", &[("contains", "1")])]) {
        panic!("expected contains match for stringified int");
    }
    if !matches(&entity, &[("values", &[("contains", "2")])]) {
        panic!("expected contains match for float");
    }
    if !matches(&entity, &[("values", &[("any", "two,three")])]) {
        panic!("expected any match for string value");
    }
    if !matches(&entity, &[("values", &[("none", "four,five")])]) {
        panic!("expected none match for absent values");
    }
}

#[test]
fn test_filters_match_entity_missing_field_does_not_match() {
    let _s = setup();
    let entity = doc(r#"{"id":"nf","title":"No year"}"#);
    for op in [
        "eq",
        "neq",
        "lt",
        "lte",
        "gt",
        "gte",
        "contains",
        "not_contains",
        "any",
        "all",
        "none",
    ] {
        if matches(&entity, &[("year", &[(op, "1982")])]) {
            panic!("expected {op} on missing field to fail");
        }
    }

    if matches(&entity, &[("author.name", &[("eq", "Alice")])]) {
        panic!("expected filter on missing nested field to fail");
    }

    if matches(
        &entity,
        &[
            ("title", &[("eq", "No year")]),
            ("year", &[("gte", "1982")]),
        ],
    ) {
        panic!("expected missing field to fail even when other filters match");
    }
}

#[test]
fn test_filters_match_entity_null_value_does_not_match() {
    let _s = setup();
    let entity = doc(r#"{"year":null}"#);
    if matches(&entity, &[("year", &[("gte", "1982")])]) {
        panic!("expected filter on null field to fail");
    }
}

#[test]
fn test_filters_match_entity_array_traversal() {
    let _s = setup();
    let entity =
        doc(r#"{"cats":[{"name":"sf","meta":{"rank":1.0}},{"name":"noir","meta":{"rank":4.0}}]}"#);

    if !matches(&entity, &[("cats.name", &[("eq", "sf")])]) {
        panic!("expected match when an array element satisfies the filter");
    }
    if matches(&entity, &[("cats.name", &[("eq", "comedy")])]) {
        panic!("expected no match when no array element satisfies the filter");
    }
    if !matches(&entity, &[("cats.meta.rank", &[("gte", "3")])]) {
        panic!("expected match on nested path through array");
    }
    if matches(&entity, &[("cats.meta.rank", &[("gt", "4")])]) {
        panic!("expected no match on nested path through array");
    }
}

#[test]
fn test_filters_match_entity_traverses_included_relation_list() {
    let _s = setup();
    let mut entity = Map::new();
    entity.insert(
        "authors".to_string(),
        doc_list(r#"[{"name":"Alice","job":{"title":"Writer"}},{"name":"Bob"}]"#),
    );

    if !matches(&entity, &[("authors.name", &[("eq", "Bob")])]) {
        panic!("expected match on included relation list");
    }
    if !matches(&entity, &[("authors.job.title", &[("eq", "Writer")])]) {
        panic!("expected match on nested field of included relation list");
    }
    if matches(&entity, &[("authors.name", &[("eq", "Carol")])]) {
        panic!("expected no match on included relation list");
    }
}

#[test]
fn test_filters_match_entity_integer_values() {
    let _s = setup();
    let mut entity = Map::new();
    entity.insert("score".to_string(), Value::Int(5));
    entity.insert("rank".to_string(), Value::Int(3));
    entity.insert("level".to_string(), Value::Int(2));

    if !matches(
        &entity,
        &[
            ("score", &[("gte", "5")]),
            ("rank", &[("eq", "3")]),
            ("level", &[("lt", "3")]),
        ],
    ) {
        panic!("expected integer values to match");
    }
    if matches(&entity, &[("score", &[("gt", "5")])]) {
        panic!("expected integer value comparison to fail");
    }
}

#[test]
fn test_filters_match_entity_same_semantics_as_query_filter() {
    let _s = setup();
    let data = docs(
        r#"[{"id":"f1","year":1981.0},{"id":"f2","year":1982.0},{"id":"nf","title":"No year"},{"id":"t1","cats":[{"name":"sf"},{"name":"noir"}]},{"id":"t2","cats":[{"name":"comedy"}]},{"id":"n1","year":null,"cats":"sf"}]"#,
    );

    let specs: [Spec<'_>; 6] = [
        &[("year", &[("gte", "1982")])],
        &[("year", &[("neq", "1982")])],
        &[("cats.name", &[("eq", "sf")])],
        &[("cats.name", &[("neq", "sf")])],
        &[("cats", &[("eq", "sf")])],
        &[("title", &[("eq", "*")])],
    ];

    for spec in specs {
        let f = filters(spec);
        let want = api::apply_query_filter(data.clone(), &FilterNode::with_leaf(f.clone()));
        let got = api::apply_filters_to_list(data.clone(), Some(&f));
        if want != got {
            panic!("filter {spec:?}: REST {got:?}, query {want:?}");
        }
    }
}

#[test]
fn test_stored_filters_match_entity_ignores_unresolved_fields() {
    let _s = setup();
    let entity = doc(r#"{"tag":"a","cats":[{"name":"sf"}]}"#);

    let f = filters(&[
        ("allowed", &[("eq", "true")]),
        ("tag", &[("eq", "a")]),
        ("cats.name", &[("eq", "sf")]),
    ]);
    if !api::stored_filters_match_entity(&entity, &f) {
        panic!("expected unresolved field to be ignored");
    }

    let f = filters(&[("allowed", &[("eq", "true")]), ("tag", &[("eq", "b")])]);
    if api::stored_filters_match_entity(&entity, &f) {
        panic!("expected stored field mismatch to fail");
    }

    let f = filters(&[("cats.name", &[("eq", "noir")])]);
    if api::stored_filters_match_entity(&entity, &f) {
        panic!("expected array traversal mismatch to fail");
    }
}
