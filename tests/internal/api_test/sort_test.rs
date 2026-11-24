use elysiandb::api;

use super::{init_test_store, setup, write};

fn sorted(entity: &str, field: &str, asc: bool) -> Vec<String> {
    api::get_sorted_entity_ids_by_field(entity.as_bytes(), field.as_bytes(), asc)
        .into_iter()
        .map(|id| String::from_utf8_lossy(&id).into_owned())
        .collect()
}

fn check(entity: &str, field: &str, docs: &[&str], want_asc: &[&str], want_desc: &[&str]) {
    let _s = setup();
    let _db = init_test_store();
    for d in docs {
        write(entity, d);
    }

    let asc = sorted(entity, field, true);
    let desc = sorted(entity, field, false);
    if asc != want_asc {
        panic!("{field} asc = {asc:?}, want {want_asc:?}");
    }
    if desc != want_desc {
        panic!("{field} desc = {desc:?}, want {want_desc:?}");
    }
}

fn idx(arr: &[String], v: &str) -> i64 {
    arr.iter().position(|x| x == v).map_or(-1, |i| i as i64)
}

fn contains_exactly(got: &[String], want: &[&str]) -> bool {
    got.len() == want.len() && want.iter().all(|w| got.iter().any(|g| g == w))
}

#[test]
fn test_get_sorted_entity_ids_by_field_int_asc_desc() {
    check(
        "people",
        "age",
        &[
            r#"{"id":"p1","age":30}"#,
            r#"{"id":"p2","age":22}"#,
            r#"{"id":"p3","age":40}"#,
        ],
        &["p2", "p1", "p3"],
        &["p3", "p1", "p2"],
    );
}

#[test]
fn test_get_sorted_entity_ids_by_field_float_asc_desc() {
    check(
        "scores",
        "score",
        &[
            r#"{"id":"s1","score":1.5}"#,
            r#"{"id":"s2","score":3.0}"#,
            r#"{"id":"s3","score":2.2}"#,
        ],
        &["s1", "s3", "s2"],
        &["s2", "s3", "s1"],
    );
}

#[test]
fn test_get_sorted_entity_ids_by_field_string_asc_desc() {
    check(
        "cities",
        "name",
        &[
            r#"{"id":"c1","name":"Paris"}"#,
            r#"{"id":"c2","name":"Berlin"}"#,
            r#"{"id":"c3","name":"Tokyo"}"#,
        ],
        &["c2", "c1", "c3"],
        &["c3", "c1", "c2"],
    );
}

#[test]
fn test_get_sorted_entity_ids_by_field_mixed_types_and_missing() {
    let _s = setup();
    let _db = init_test_store();
    write("mixed", r#"{"id":"m1","rank":10}"#);
    write("mixed", r#"{"id":"m2","rank":5}"#);
    write("mixed", r#"{"id":"m3","rank":"not-an-int"}"#);
    write("mixed", r#"{"id":"m4"}"#);

    let asc = sorted("mixed", "rank", true);
    let desc = sorted("mixed", "rank", false);
    let want = ["m1", "m2", "m3", "m4"];
    if !contains_exactly(&asc, &want) {
        panic!("asc should contain all ids, got {asc:?}");
    }
    if !contains_exactly(&desc, &want) {
        panic!("desc should contain all ids, got {desc:?}");
    }
    if idx(&asc, "m2") > idx(&asc, "m1") {
        panic!("expected m2 to come before m1 in asc, got {asc:?}");
    }
    if idx(&desc, "m1") > idx(&desc, "m2") {
        panic!("expected m1 to come before m2 in desc, got {desc:?}");
    }
}

#[test]
fn test_get_sorted_entity_ids_by_field_nested_fields() {
    check(
        "books",
        "author.name",
        &[
            r#"{"id":"b1","author":{"name":"Charles"}}"#,
            r#"{"id":"b2","author":{"name":"Alice"}}"#,
            r#"{"id":"b3","author":{"name":"Bob"}}"#,
        ],
        &["b2", "b3", "b1"],
        &["b1", "b3", "b2"],
    );
}

#[test]
fn test_get_sorted_entity_ids_by_field_date_asc_desc() {
    check(
        "events",
        "date",
        &[
            r#"{"id":"e1","date":"2023-01-01"}"#,
            r#"{"id":"e2","date":"2022-12-31"}"#,
            r#"{"id":"e3","date":"2023-01-02"}"#,
        ],
        &["e2", "e1", "e3"],
        &["e3", "e1", "e2"],
    );
}

#[test]
fn test_get_sorted_entity_ids_by_field_date_time_asc_desc() {
    check(
        "events",
        "date",
        &[
            r#"{"id":"e1","date":"2023-01-01T10:00:00Z"}"#,
            r#"{"id":"e2","date":"2022-12-31T23:59:59Z"}"#,
            r#"{"id":"e3","date":"2023-01-02T00:00:00Z"}"#,
        ],
        &["e2", "e1", "e3"],
        &["e3", "e1", "e2"],
    );
}

#[test]
fn test_get_sorted_entity_ids_by_field_nested_date_asc_desc() {
    check(
        "meetings",
        "schedule.start",
        &[
            r#"{"id":"m1","schedule":{"start":"2023-01-02"}}"#,
            r#"{"id":"m2","schedule":{"start":"2022-12-31"}}"#,
            r#"{"id":"m3","schedule":{"start":"2023-01-01"}}"#,
        ],
        &["m2", "m3", "m1"],
        &["m1", "m3", "m2"],
    );
}
