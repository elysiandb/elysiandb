use std::collections::BTreeSet;

use elysiandb::api;
use elysiandb::gocompat::json::{Map, Value};

use super::{doc, filters, ids, init_test_store, set_schema, setup, write};

fn text<'a>(m: &'a Map, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str)
}

fn list(entity: &str, f: Option<&elysiandb::query::Filters>) -> Vec<Map> {
    api::list_entities(entity.as_bytes(), 0, 0, b"", true, f, b"", b"")
}

#[test]
fn write_and_read_entity_by_id() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "articles",
        r#"{"id":"a1","title":"Hello","tags":["go","kv"]}"#,
    );

    let Some(got) = api::read_entity_by_id(b"articles", b"a1") else {
        panic!("ReadEntityById returned nil");
    };
    if text(&got, "id") != Some("a1") {
        panic!("id mismatch: got {:?} want a1", got.get("id"));
    }
    if text(&got, "title") != Some("Hello") {
        panic!("title mismatch: got {:?} want Hello", got.get("title"));
    }
    match got.get("tags") {
        Some(Value::Array(tags))
            if tags.len() == 2
                && tags[0].as_str() == Some("go")
                && tags[1].as_str() == Some("kv") => {}
        other => panic!("tags mismatch: got {other:?}"),
    }
}

#[test]
fn read_all_entities() {
    let _s = setup();
    let _db = init_test_store();

    write("users", r#"{"id":"u1","name":"alice"}"#);
    write("users", r#"{"id":"u2","name":"bob"}"#);

    let all = list("users", None);
    if all.len() != 2 {
        panic!("ListEntities len={}, want 2, all={all:?}", all.len());
    }

    let seen: BTreeSet<String> = ids(&all).into_iter().collect();
    if !seen.contains("u1") || !seen.contains("u2") {
        panic!("expected to see u1 and u2, got {seen:?}");
    }
}

#[test]
fn update_entity_by_id_merges_and_persists() {
    let _s = setup();
    let _db = init_test_store();

    write("orders", r#"{"id":"o42","status":"pending","price":10}"#);

    let Some(updated) =
        api::update_entity_by_id(b"orders", b"o42", &doc(r#"{"status":"paid","note":"ok"}"#))
    else {
        panic!("UpdateEntityById returned nil");
    };
    if text(&updated, "status") != Some("paid") {
        panic!("status not updated: {:?}", updated.get("status"));
    }
    if text(&updated, "note") != Some("ok") {
        panic!("note not merged: {:?}", updated.get("note"));
    }
    let got = api::read_entity_by_id(b"orders", b"o42");
    if got.as_ref() != Some(&updated) {
        panic!("persisted mismatch:\n updated={updated:?}\n got={got:?}");
    }
}

#[test]
fn delete_entity_by_id_removes_single() {
    let _s = setup();
    let _db = init_test_store();

    write("comments", r#"{"id":"c1","body":"hello"}"#);
    write("comments", r#"{"id":"c2","body":"world"}"#);

    api::delete_entity_by_id(b"comments", b"c1");

    if let Some(v) = api::read_entity_by_id(b"comments", b"c1") {
        panic!("c1 should be deleted, got {v:?}");
    }
    if api::read_entity_by_id(b"comments", b"c2").is_none() {
        panic!("c2 should still exist");
    }
}

#[test]
fn delete_all_entities_removes_all_for_that_entity_only() {
    let _s = setup();
    let _db = init_test_store();

    write("posts", r#"{"id":"p1"}"#);
    write("posts", r#"{"id":"p2"}"#);
    write("profiles", r#"{"id":"me"}"#);

    api::delete_all_entities(b"posts");

    let posts = list("posts", None);
    if !posts.is_empty() {
        panic!("posts should be empty, got {posts:?}");
    }
    if api::read_entity_by_id(b"profiles", b"me").is_none() {
        panic!("profiles entity should be untouched");
    }
}

#[test]
fn list_entities_with_filters() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "books",
        r#"{"id":"b1","title":"Go in Action","author":"Alice"}"#,
    );
    write(
        "books",
        r#"{"id":"b2","title":"Learning Python","author":"Bob"}"#,
    );
    write(
        "books",
        r#"{"id":"b3","title":"Advanced Go","author":"Alice"}"#,
    );

    let r = list("books", Some(&filters(&[("author", &[("eq", "Alice")])])));
    if r.len() != 2 {
        panic!(
            "expected 2 results for author=Alice, got {} ({r:?})",
            r.len()
        );
    }

    let r = list(
        "books",
        Some(&filters(&[("title", &[("eq", "Learning Python")])])),
    );
    if ids(&r) != ["b2"] {
        panic!("expected only b2, got {r:?}");
    }

    let r = list("books", Some(&filters(&[("author", &[("eq", "Al*")])])));
    if r.len() != 2 {
        panic!(
            "expected 2 results for author wildcard, got {} ({r:?})",
            r.len()
        );
    }

    let r = list("books", Some(&filters(&[("title", &[("eq", "*Go*")])])));
    if r.len() != 2 {
        panic!(
            "expected 2 results for title contains Go, got {} ({r:?})",
            r.len()
        );
    }

    let r = list("books", Some(&filters(&[("author", &[("neq", "Alice")])])));
    if ids(&r) != ["b2"] {
        panic!("expected only b2 with neq filter, got {r:?}");
    }
}

#[test]
fn list_entities_with_nested_filters() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "articles",
        r#"{"id":"a1","title":"Go & Dist","author":{"name":"Mister Good","id":"u1","category":{"title":"yep"}}}"#,
    );
    write(
        "articles",
        r#"{"id":"a2","title":"Python Tips","author":{"name":"Alice","id":"u2","category":{"title":"news"}}}"#,
    );
    write(
        "articles",
        r#"{"id":"a3","title":"Go Advanced","author":{"name":"Bob","id":"u3","category":{"title":"yep"}}}"#,
    );

    let r1 = list(
        "articles",
        Some(&filters(&[("author.name", &[("eq", "Alice")])])),
    );
    if ids(&r1) != ["a2"] {
        panic!("expected only a2, got {r1:?}");
    }

    let r2 = list(
        "articles",
        Some(&filters(&[("author.name", &[("eq", "mister*")])])),
    );
    if ids(&r2) != ["a1"] {
        panic!("expected only a1, got {r2:?}");
    }

    let r3 = list(
        "articles",
        Some(&filters(&[("author.category.title", &[("eq", "yep")])])),
    );
    if r3.len() != 2 {
        panic!("expected a1 and a3 for category=yep, got {r3:?}");
    }

    let r4 = list(
        "articles",
        Some(&filters(&[
            ("author.category.title", &[("eq", "yep")]),
            ("author.name", &[("neq", "bob")]),
        ])),
    );
    if ids(&r4) != ["a1"] {
        panic!("expected only a1 for combined eq/neq, got {r4:?}");
    }
}

#[test]
fn list_entities_with_numeric_filters() {
    let _s = setup();
    let _db = init_test_store();

    write("products", r#"{"id":"p1","price":10.0}"#);
    write("products", r#"{"id":"p2","price":20.0}"#);
    write("products", r#"{"id":"p3","price":30.0}"#);

    let cases: [(&str, &[&str]); 6] = [
        ("eq", &["p2"]),
        ("neq", &["p1", "p3"]),
        ("lt", &["p1"]),
        ("lte", &["p1", "p2"]),
        ("gt", &["p3"]),
        ("gte", &["p2", "p3"]),
    ];
    for (op, want) in cases {
        let r = list("products", Some(&filters(&[("price", &[(op, "20")])])));
        if ids(&r) != want {
            panic!("expected {want:?} for {op}=20, got {r:?}");
        }
    }
}

#[test]
fn offset_limit_and_sort() {
    let _s = setup();
    let _db = init_test_store();

    for i in 1..=4 {
        write("nums", &format!(r#"{{"id":"{i}","v":{i}}}"#));
    }

    let all = api::list_entities(b"nums", 2, 1, b"", true, None, b"", b"");
    if all.len() != 2 {
        panic!("expected 2 after offset/limit, got {all:?}");
    }
}

#[test]
fn entity_exists_true() {
    let _s = setup();
    let _db = init_test_store();

    write("cars", r#"{"id":"c1","brand":"bmw"}"#);

    if !api::entity_exists(b"cars", b"c1") {
        panic!("expected EntityExists=true for c1");
    }
}

#[test]
fn entity_exists_false() {
    let _s = setup();
    let _db = init_test_store();

    write("cars", r#"{"id":"c1","brand":"bmw"}"#);

    if api::entity_exists(b"cars", b"doesnotexist") {
        panic!("expected EntityExists=false for missing id");
    }
}

#[test]
fn entity_exists_wrong_entity() {
    let _s = setup();
    let _db = init_test_store();

    write("authors", r#"{"id":"a1","name":"Alice"}"#);

    if api::entity_exists(b"books", b"a1") {
        panic!("expected EntityExists=false for mismatched entity");
    }
}

#[test]
fn entity_exists_empty_store() {
    let _s = setup();
    let _db = init_test_store();

    if api::entity_exists(b"ghost", b"id123") {
        panic!("expected EntityExists=false for empty store");
    }
}

#[test]
fn entity_types_and_exists() {
    let _s = setup();
    let _db = init_test_store();

    write("alpha", r#"{"id":"a1"}"#);
    write("beta", r#"{"id":"b1"}"#);
    let types = api::list_entity_types();
    if !types.iter().any(|t| t == b"alpha") || !types.iter().any(|t| t == b"beta") {
        panic!("expected alpha and beta in types, got {types:?}");
    }
    if !api::entity_type_exists(b"alpha") || api::entity_type_exists(b"gamma") {
        panic!("EntityTypeExists mismatch");
    }
}

#[test]
fn write_list_and_update_list() {
    let _s = setup();
    let _db = init_test_store();

    let mut items = vec![doc(r#"{"id":"1","v":1.0}"#), doc(r#"{"id":"2","v":2.0}"#)];
    api::write_list_of_entities(b"list", &mut items);

    let updated = api::update_list_of_entities(
        b"list",
        &[
            doc(r#"{"id":"1","v":9.0}"#),
            doc(r#"{"id":"2","v":8.0}"#),
            doc(r#"{"v":7.0}"#),
        ],
    )
    .unwrap_or_default();

    if updated.len() != 2 {
        panic!("expected 2 updated, got {updated:?}");
    }

    let v1 = api::read_entity_by_id(b"list", b"1").and_then(|m| m.get("v").cloned());
    if v1 != Some(Value::Number(9.0)) {
        panic!("update list failed, got {v1:?}");
    }

    let v2 = api::read_entity_by_id(b"list", b"2").and_then(|m| m.get("v").cloned());
    if v2 != Some(Value::Number(8.0)) {
        panic!("update list failed, got {v2:?}");
    }

    if api::read_entity_by_id(b"list", b"").is_some() {
        panic!("entity without id should not exist");
    }
}

#[test]
fn delete_all_and_count_all() {
    let _s = setup();
    let _db = init_test_store();

    write("a", r#"{"id":"1"}"#);
    write("b", r#"{"id":"1"}"#);
    write("b", r#"{"id":"2"}"#);

    if api::count_all_entities() != 3 {
        panic!("expected 3 before deleteall");
    }

    api::delete_all();

    if api::count_all_entities() != 0 {
        panic!("expected 0 after deleteall");
    }
}

#[test]
fn dump_all_and_import_all() {
    let _s = setup();
    let _db = init_test_store();

    write("x", r#"{"id":"1","v":1}"#);
    write("y", r#"{"id":"2","v":2}"#);

    let d = api::dump_all();
    if d.len() != 2 {
        panic!("dump size mismatch: {d:?}");
    }

    let _db2 = init_test_store();

    let mut data = api::ImportData::new();
    data.insert(
        b"x".to_vec(),
        vec![doc(r#"{"id":"1","v":1}"#), doc(r#"{"id":"3","v":3}"#)],
    );
    api::import_all(&mut data);
    if api::count_all_entities() != 2 {
        panic!("import count mismatch");
    }
    if api::read_entity_by_id(b"x", b"3").is_none() {
        panic!("imported entity missing");
    }
}

#[test]
fn list_entities_auto_include_nested() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "articles",
        r#"{"id":"a1","title":"X","author":{"id":"u1","name":"Alice"}}"#,
    );
    write(
        "articles",
        r#"{"id":"a2","title":"Y","author":{"id":"u2","name":"Bob"}}"#,
    );

    let res = list(
        "articles",
        Some(&filters(&[("author.name", &[("eq", "Alice")])])),
    );

    if ids(&res) != ["a1"] {
        panic!("expected auto-include match a1, got {res:?}");
    }

    if !matches!(res[0].get("author"), Some(Value::Object(_))) {
        panic!(
            "expected author sub-entity to be expanded automatically, got {:?}",
            res[0]
        );
    }
}

#[test]
fn list_entities_filter_excludes_missing_field() {
    let _s = setup();
    let _db = init_test_store();

    write("movies", r#"{"id":"f1","year":1981.0}"#);
    write("movies", r#"{"id":"f2","year":1982.0}"#);
    write("movies", r#"{"id":"f3","year":1983.0}"#);
    write("movies", r#"{"id":"nf","title":"No year"}"#);

    let f = filters(&[("year", &[("gte", "1982")])]);
    let res = api::list_entities(b"movies", 0, 0, b"year", true, Some(&f), b"", b"");

    if ids(&res) != ["f2", "f3"] {
        panic!("expected f2 and f3 only, got {res:?}");
    }
}

#[test]
fn list_entities_filter_traverses_arrays() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "tags",
        r#"{"id":"t1","cats":[{"name":"sf"},{"name":"noir"}]}"#,
    );
    write("tags", r#"{"id":"t2","cats":[{"name":"comedy"}]}"#);

    let res = list("tags", Some(&filters(&[("cats.name", &[("eq", "sf")])])));

    if ids(&res) != ["t1"] {
        panic!("expected only t1, got {res:?}");
    }
}

#[test]
fn list_entities_filter_on_relation_list() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "books",
        r#"{"id":"b1","authors":[{"@entity":"author","id":"32e1f608"}]}"#,
    );
    write(
        "books",
        r#"{"id":"b2","authors":[{"@entity":"author","id":"25667686"}]}"#,
    );
    write("books", r#"{"id":"b3"}"#);

    let res = list(
        "books",
        Some(&filters(&[("authors.fullname", &[("eq", "Alberto")])])),
    );
    if ids(&res) != ["b2"] {
        panic!("expected only b2, got {res:?}");
    }

    let res = list(
        "books",
        Some(&filters(&[(
            "authors.job.designation",
            &[("eq", "Worker")],
        )])),
    );
    if res.len() != 2 {
        panic!("expected b1 and b2, got {res:?}");
    }
}

#[test]
fn list_entities_auto_include_deep_nested() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "articles",
        r#"{"id":"a1","title":"X","author":{"id":"u1","name":"Alice","profile":{"city":"Paris"}}}"#,
    );
    write(
        "articles",
        r#"{"id":"a2","title":"Y","author":{"id":"u2","name":"Bob","profile":{"city":"Lyon"}}}"#,
    );

    let res = list(
        "articles",
        Some(&filters(&[("author.profile.city", &[("eq", "Paris")])])),
    );

    if ids(&res) != ["a1"] {
        panic!("expected only a1, got {res:?}");
    }

    let city = match res[0].get("author") {
        Some(Value::Object(author)) => match author.get("profile") {
            Some(Value::Object(profile)) => text(profile, "city"),
            _ => None,
        },
        _ => None,
    };
    if city != Some("Paris") {
        panic!("expected nested include applied, got {:?}", res[0]);
    }
}

#[test]
fn create_entity_type() {
    let _s = setup();
    let _db = init_test_store();

    if let Err(err) = api::create_entity_type(b"foo") {
        panic!("unexpected error: {err}");
    }

    if !api::entity_type_exists(b"foo") {
        panic!("expected foo to exist after creation");
    }

    match api::create_entity_type(b"foo") {
        Err(err) if err.to_string() == "entity type 'foo' already exists" => {}
        other => panic!("expected error when creating existing type, got {other:?}"),
    }
}

#[test]
fn delete_entity_type() {
    let _s = setup();
    let _db = init_test_store();

    let _ = api::create_entity_type(b"foo");
    write("foo", r#"{"id":"1"}"#);
    write("foo", r#"{"id":"2"}"#);

    if let Err(err) = api::delete_entity_type(b"foo") {
        panic!("unexpected error: {err}");
    }

    if api::entity_type_exists(b"foo") {
        panic!("foo type should not exist after deletion");
    }

    let res = list("foo", None);
    if !res.is_empty() {
        panic!("expected no entities left after delete, got {res:?}");
    }

    match api::delete_entity_type(b"foo") {
        Err(err) if err.to_string() == "entity type 'foo' does not exist" => {}
        other => panic!("expected error deleting non-existent type, got {other:?}"),
    }
}

#[test]
fn update_entity_schema() {
    let _s = setup();
    let _db = init_test_store();

    let out = api::update_entity_schema(b"person", &doc(r#"{"title":"string","age":"number"}"#));

    if text(&out, "id") != Some("person") {
        panic!("schema id mismatch: got {:?}", out.get("id"));
    }

    if out.get("_manual") != Some(&Value::Bool(true)) {
        panic!("expected _manual = true");
    }

    let Some(stored) = api::read_entity_by_id(b"_elysiandb_core_schema", b"person") else {
        panic!("stored schema missing");
    };

    for (k, v) in &out {
        if stored.get(k) != Some(v) {
            panic!(
                "stored schema field mismatch for {k}: got {:?} want {v:?}",
                stored.get(k)
            );
        }
    }
}

#[test]
fn get_list_of_ids_no_sort_field() {
    let _s = setup();
    let _db = init_test_store();

    write("xxx", r#"{"id":"1"}"#);
    write("xxx", r#"{"id":"2"}"#);

    match api::get_list_of_ids(b"xxx", b"", true) {
        Ok(data) if !data.is_empty() => {}
        Ok(_) => panic!("expected id list, got empty"),
        Err(err) => panic!("unexpected error: {err}"),
    }
}

#[test]
fn get_entity_schema() {
    let _s = setup();
    let _db = init_test_store();

    api::update_entity_schema(b"u", &doc(r#"{"name":"string"}"#));

    let Some(out) = api::get_entity_schema(b"u") else {
        panic!("expected schema to be returned, got nil");
    };

    if text(&out, "id") != Some("u") {
        panic!("schema id mismatch");
    }
}

#[test]
fn update_schema_if_needed_disabled() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(false, false);

    write("xxx", r#"{"id":"1","name":"abc"}"#);

    if let Some(sc) = api::read_entity_by_id(b"_elysiandb_core_schema", b"xxx") {
        panic!("schema should not be created when disabled, got {sc:?}");
    }
}

#[test]
fn update_schema_if_needed_enabled() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    write("xxx", r#"{"id":"1","name":"abc"}"#);

    if api::read_entity_by_id(b"_elysiandb_core_schema", b"xxx").is_none() {
        panic!("expected schema to be updated");
    }
}

#[test]
fn write_list_of_entities_returns_validation_errors() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, true);

    api::update_entity_schema(
        b"u",
        &doc(r#"{"name":"string","age":"number","country":"string"}"#),
    );

    let mut input = vec![
        doc(r#"{"id":"1","name":"a","age":20.0,"country":"fr"}"#),
        doc(r#"{"id":"2","name":"b","age":"wrong"}"#),
    ];

    let out = api::write_list_of_entities(b"u", &mut input);

    if out[1].is_empty() {
        panic!("expected validation error for invalid entry");
    }
}

#[test]
fn list_entities_includes_param() {
    let _s = setup();
    let _db = init_test_store();

    write("orders", r#"{"id":"o1","item":{"id":"i1","name":"Item1"}}"#);
    write("orders", r#"{"id":"o2","item":{"id":"i2","name":"Item2"}}"#);

    let out = api::list_entities(b"orders", 0, 0, b"", true, None, b"", b"item");
    if out.len() != 2 {
        panic!("expected 2 results, got {out:?}");
    }

    if !matches!(out[0].get("item"), Some(Value::Object(_))) {
        panic!("expected nested item include");
    }
}

#[test]
fn list_entities_empty() {
    let _s = setup();
    let _db = init_test_store();

    let out = list("nope", None);
    if !out.is_empty() {
        panic!("expected empty list, got {out:?}");
    }
}

#[test]
fn delete_all_entities_removes_indexes_too() {
    let _s = setup();
    let _db = init_test_store();

    write("xxx", r#"{"id":"1"}"#);
    write("xxx", r#"{"id":"2"}"#);

    api::delete_all_entities(b"xxx");

    if !api::list_entity_types().is_empty() {
        panic!("entity type should be removed");
    }
}

#[test]
fn get_entity_schema_no_schema() {
    let _s = setup();
    let _db = init_test_store();

    if let Some(out) = api::get_entity_schema(b"ghost") {
        panic!("expected nil schema, got {out:?}");
    }
}

#[test]
fn dump_all_empty() {
    let _s = setup();
    let _db = init_test_store();

    let out = api::dump_all();
    if !out.is_empty() {
        panic!("expected empty dump, got {out:?}");
    }
}

#[test]
fn import_all_multiple_entities() {
    let _s = setup();
    let _db = init_test_store();

    let mut data = api::ImportData::new();
    data.insert(
        b"a".to_vec(),
        vec![doc(r#"{"id":"1"}"#), doc(r#"{"id":"2"}"#)],
    );
    data.insert(b"b".to_vec(), vec![doc(r#"{"id":"10"}"#)]);
    api::import_all(&mut data);

    if api::count_all_entities() != 3 {
        panic!("expected 3 entries after import");
    }
}

#[test]
fn update_entity_by_id_no_existing() {
    let _s = setup();
    let _db = init_test_store();

    if api::update_entity_by_id(b"x", b"missing", &doc(r#"{"a":1}"#)).is_some() {
        panic!("expected nil when updating non-existing");
    }
}

#[test]
fn update_entity_by_id_keeps_manual_schema_in_strict_mode() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, true);

    api::update_entity_schema(
        b"film",
        &doc(
            r#"{"title":{"type":"string","required":true},"year":{"type":"number","required":false}}"#,
        ),
    );

    write("film", r#"{"id":"1","title":"Alien"}"#);
    api::update_entity_by_id(b"film", b"1", &doc(r#"{"title":"Aliens"}"#));

    let sc = api::read_entity_by_id(b"_elysiandb_core_schema", b"film").unwrap_or_default();
    if !sc.contains_key("_manual") {
        panic!("manual schema must be kept after update, got {sc:?}");
    }

    let year = match sc.get("fields") {
        Some(Value::Object(fields)) => fields.get("year").cloned(),
        _ => None,
    };
    let required = match &year {
        Some(Value::Object(y)) => y.get("required").cloned(),
        _ => None,
    };
    if required != Some(Value::Bool(false)) {
        panic!("manual field definition must be kept, got {year:?}");
    }

    let errs = api::write_entity(b"film", &mut doc(r#"{"id":"2","title":"X","note":9.0}"#));
    if errs.is_empty() {
        panic!("expected strict validation to still reject undeclared fields");
    }
}

#[test]
fn update_entity_by_id_updates_inferred_schema() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    write("film", r#"{"id":"1","title":"Alien"}"#);
    api::update_entity_by_id(b"film", b"1", &doc(r#"{"year":1979.0}"#));

    let sc = api::read_entity_by_id(b"_elysiandb_core_schema", b"film").unwrap_or_default();
    let has_year = matches!(sc.get("fields"), Some(Value::Object(f)) if f.contains_key("year"));
    if !has_year {
        panic!(
            "inferred schema must include fields added by an update, got {:?}",
            sc.get("fields")
        );
    }
}

#[test]
fn write_entity_auto_id_assignment() {
    let _s = setup();
    let _db = init_test_store();

    let mut input = doc(r#"{"name":"x"}"#);
    api::write_entity(b"items", &mut input);

    let id = text(&input, "id").unwrap_or_default().to_string();
    if id.is_empty() {
        panic!("expected generated id");
    }

    if api::read_entity_by_id(b"items", id.as_bytes()).is_none() {
        panic!("expected entity stored with generated id");
    }
}

#[test]
fn write_entity_skip_schema_updates_for_manual_strict() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, true);

    api::update_entity_schema(b"p", &doc(r#"{"name":"string"}"#));

    write("p", r#"{"id":"1","name":"abc"}"#);

    if api::read_entity_by_id(b"_elysiandb_core_schema", b"p").is_none() {
        panic!("manual schema must exist");
    }
}

fn with_fields(base: &Map, extra: &str) -> Map {
    let mut out = base.clone();
    out.extend(doc(extra));
    out
}

#[test]
fn sub_entity_references_keep_target_fields() {
    let _s = setup();
    let movie = doc(r#"{"id":"m1","title":"Blade Runner","year":1982.0,"rating":8.0}"#);
    let person = doc(r#"{"id":"p1","name":"Ridley Scott","born":1937.0,"country":"UK"}"#);

    let cases: Vec<(&str, &str, Map, Map)> = vec![
        (
            "pure reference in an array",
            r#"{"items":[{"@entity":"movie","id":"m1"}]}"#,
            movie.clone(),
            person.clone(),
        ),
        (
            "pure reference with @id in an array",
            r#"{"items":[{"@entity":"movie","@id":"m1"}]}"#,
            movie.clone(),
            person.clone(),
        ),
        (
            "pure reference in an object",
            r#"{"work":{"@entity":"movie","id":"m1"}}"#,
            movie.clone(),
            person.clone(),
        ),
        (
            "reference with a field in an object",
            r#"{"work":{"@entity":"movie","id":"m1","rating":10.0}}"#,
            with_fields(&movie, r#"{"rating":10.0}"#),
            person.clone(),
        ),
        (
            "reference with a field in an array",
            r#"{"items":[{"@entity":"movie","id":"m1","rating":10.0}]}"#,
            with_fields(&movie, r#"{"rating":10.0}"#),
            person.clone(),
        ),
        (
            "reference with a field two levels deep",
            r#"{"items":[{"@entity":"movie","id":"m1","director":{"@entity":"person","id":"p1","role":"director"}}]}"#,
            with_fields(&movie, r#"{"director":{"@entity":"person","id":"p1"}}"#),
            with_fields(&person, r#"{"role":"director"}"#),
        ),
    ];

    for (name, payload, want_movie, want_person) in &cases {
        for mode in ["write", "update"] {
            let _db = init_test_store();

            api::write_entity(b"movie", &mut movie.clone());
            api::write_entity(b"person", &mut person.clone());

            let mut payload = doc(payload);
            if mode == "write" {
                payload.insert("id".to_string(), Value::String("c1".to_string()));
                api::write_entity(b"collection", &mut payload);
            } else {
                write("collection", r#"{"id":"c1","label":"Collection"}"#);
                api::update_entity_by_id(b"collection", b"c1", &payload);
            }

            for (entity, want) in [("movie", want_movie), ("person", want_person)] {
                let id = text(want, "id").unwrap_or_default();
                let got = api::read_entity_by_id(entity.as_bytes(), id.as_bytes());
                if got.as_ref() != Some(want) {
                    panic!("{name}/{mode}: {entity}: got {got:?}, want {want:?}");
                }
            }
        }
    }
}

#[test]
fn write_entity_sub_entity_without_id_is_created() {
    let _s = setup();
    let _db = init_test_store();

    write(
        "collection",
        r#"{"id":"c1","items":[{"@entity":"movie","title":"Heat"}]}"#,
    );

    let stored = api::read_entity_by_id(b"collection", b"c1").unwrap_or_default();
    let link = match stored.get("items") {
        Some(Value::Array(items)) => match items.first() {
            Some(Value::Object(m)) => m.clone(),
            _ => Map::new(),
        },
        _ => Map::new(),
    };
    let id = text(&link, "id").unwrap_or_default().to_string();
    if id.is_empty() || text(&link, "@entity") != Some("movie") {
        panic!("expected a link to the new movie, got {link:?}");
    }

    let got = api::read_entity_by_id(b"movie", id.as_bytes());
    let mut want = Map::new();
    want.insert("id".to_string(), Value::String(id.clone()));
    want.insert("title".to_string(), Value::String("Heat".to_string()));
    if got.as_ref() != Some(&want) {
        panic!("expected the new movie to be created, got {got:?}");
    }
}
