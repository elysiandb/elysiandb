use elysiandb::api::{self, WriteSimulation};
use elysiandb::gocompat::json::Value;

use super::{doc, init_test_store, set_schema, setup, value, write};

fn new_simulation() -> WriteSimulation<'static> {
    WriteSimulation::new(
        Box::new(api::get_entity_schema),
        Box::new(api::read_entity_by_id),
    )
}

#[test]
fn test_write_simulation_tracks_schema_evolution() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    let mut sim = new_simulation();
    let errs = sim.write(b"val", &doc(r#"{"id":"x1","a":"s"}"#));
    if !errs.is_empty() {
        panic!("unexpected errors {errs:?}");
    }
    let errs = sim.write(b"val", &doc(r#"{"id":"x2","a":5.0}"#));
    if errs.len() != 1 || errs[0].field != "a" {
        panic!("expected the second write to fail against the simulated schema, got {errs:?}");
    }
    if api::entity_type_exists(b"val") || api::read_entity_by_id(b"val", b"x1").is_some() {
        panic!("a simulation must not write anything");
    }
}

#[test]
fn test_write_simulation_uses_stored_schema() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    write("val", r#"{"id":"x0","a":"s"}"#);
    let errs = new_simulation().write(b"val", &doc(r#"{"id":"x1","a":1.0}"#));
    if errs.len() != 1 {
        panic!("expected the stored schema to be used, got {errs:?}");
    }
}

#[test]
fn test_write_simulation_tracks_existence() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    write("acct", r#"{"id":"B","balance":100.0}"#);
    let mut sim = new_simulation();
    if !sim.update(b"acct", b"B", &doc(r#"{"balance":1.0}"#)) {
        panic!("expected an update on an existing document to succeed");
    }
    sim.delete(b"acct", b"B");
    if sim.update(b"acct", b"B", &doc(r#"{"balance":0.0}"#)) {
        panic!("expected an update after a delete to fail");
    }
    if sim.update(b"acct", b"missing", &doc(r#"{"balance":0.0}"#)) {
        panic!("expected an update on a missing document to fail");
    }
    sim.write(b"acct", &doc(r#"{"id":"B","balance":5.0}"#));
    if !sim.update(b"acct", b"B", &doc(r#"{"balance":6.0}"#)) {
        panic!("expected an update after a write to succeed");
    }
    let got = api::read_entity_by_id(b"acct", b"B").and_then(|m| m.get("balance").cloned());
    if got != Some(Value::Number(100.0)) {
        panic!("a simulation must not change stored documents, got {got:?}");
    }
}

#[test]
fn test_write_simulation_tracks_sub_entities() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    let data = doc(r#"{"id":"p1","title":"T","author":{"@entity":"writer","id":"w1","name":"N"}}"#);
    let mut sim = new_simulation();
    sim.write(b"post", &data);
    if !sim.update(b"writer", b"w1", &doc(r#"{"name":"M"}"#)) {
        panic!("expected the nested writer to exist in the simulation");
    }
    let nested = matches!(data.get("author"), Some(Value::Object(a)) if a.contains_key("name"));
    if !nested {
        panic!("a simulation must not rewrite the payload");
    }
}

#[test]
fn test_nested_entity_types() {
    let _s = setup();
    let data = value(
        r#"{"author":{"@entity":"author","id":"a1","job":{"@entity":"job","id":"j1"}},"tags":[{"@entity":"tag","id":"t1"},{"@entity":"author","id":"a2"},"plain"],"meta":{"x":1.0}}"#,
    );
    let mut got = api::nested_entity_types(&data);
    got.sort();
    if got != [b"author".to_vec(), b"job".to_vec(), b"tag".to_vec()] {
        panic!("unexpected nested entity types {got:?}");
    }
    let list = api::nested_entity_types(&value(r#"[{"a":{"@entity":"x","id":"1"}}]"#));
    if list != [b"x".to_vec()] {
        panic!("expected nested types inside a list, got {list:?}");
    }
    let got = api::nested_entity_types(&value(r#"{"a":1.0}"#));
    if !got.is_empty() {
        panic!("expected no nested types, got {got:?}");
    }
}

#[test]
fn test_write_simulation_sub_entities_follow_the_storage_rules() {
    let _s = setup();
    let _db = init_test_store();
    set_schema(true, false);

    write("author", r#"{"id":"x","age":30.0}"#);
    let data = doc(r#"{"id":"b","author":{"@entity":"author","id":"z","age":"thirty"}}"#);
    let errs = new_simulation().write(b"book", &data);
    if !errs.is_empty() {
        panic!("expected the parent write to pass, got {errs:?}");
    }
    let mut sim = new_simulation();
    sim.write(b"book", &data);
    if !sim.update(b"author", b"z", &doc(r#"{"name":"Z"}"#)) {
        panic!("expected the nested author to be written despite its inferred type");
    }
    let errs = new_simulation().write(b"author", &doc(r#"{"id":"w","age":"thirty"}"#));
    if errs.len() != 1 {
        panic!("expected a direct write to be checked against the inferred schema, got {errs:?}");
    }
}
