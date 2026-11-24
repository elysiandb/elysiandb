#[path = "../internal/tempdir.rs"]
mod tempdir;

use std::sync::{Arc, Once};

use elysiandb::api::{self, BatchOperation, ByteFields, WriteSimulation};
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::schema;
use elysiandb::storage;

use tempdir::TempDir;

const E: &[u8] = b"e\xfc";
const R: &[u8] = b"\xff";
const F: &str = "\u{fffd}";

static INIT: Once = Once::new();

struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn open_store() -> Store {
    INIT.call_once(api::index::register);
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.api.schema.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();
    Store { _dir: dir }
}

fn doc(text: &str) -> Map {
    let mut m = Map::new();
    json::unmarshal(text.as_bytes(), &mut m).unwrap_or_else(|e| panic!("{text}: {e}"));
    m
}

fn encoded<T: json::Encode + ?Sized>(v: &T) -> String {
    String::from_utf8(json::marshal(v).expect("marshal")).expect("utf8")
}

fn stored(entity: &[u8]) -> Vec<(Vec<u8>, String)> {
    let mut out = Vec::new();
    let engine = storage::engine().expect("engine");
    engine.snapshot(entity).ascend(0, |d| {
        let bytes = d.bytes().expect("bytes");
        out.push((
            d.id().to_vec(),
            String::from_utf8(bytes.to_vec()).expect("utf8 document"),
        ));
        true
    });
    out.sort();
    out
}

fn entry(id: &[u8], bytes: &str) -> (Vec<u8>, String) {
    (id.to_vec(), bytes.to_string())
}

fn schema_entity() -> &'static [u8] {
    schema::SCHEMA_ENTITY.as_bytes()
}

#[test]
fn entity_names_with_invalid_utf8_are_stored_as_in_go() {
    let _store = open_store();

    assert!(api::write_entity(E, &mut doc(r#"{"id":"a","n":1}"#)).is_empty());
    assert!(api::write_entity(E, &mut doc(r#"{"id":"b","n":2,"s":"x"}"#)).is_empty());
    assert_eq!(
        stored(E),
        [
            entry(b"a", r#"{"id":"a","n":1}"#),
            entry(b"b", r#"{"id":"b","n":2,"s":"x"}"#),
        ]
    );
    assert_eq!(
        stored(schema_entity()),
        [entry(
            E,
            r#"{"fields":{"n":{"name":"n","required":false,"type":"number"},"s":{"name":"s","required":false,"type":"string"}},"id":"e\ufffd"}"#
        )]
    );
    assert_eq!(
        api::list_entity_types(),
        [E.to_vec(), schema_entity().to_vec()]
    );

    let current = api::get_entity_schema(E).expect("schema");
    assert_eq!(
        encoded(&current),
        format!(
            r#"{{"fields":{{"n":{{"name":"n","required":false,"type":"number"}},"s":{{"name":"s","required":false,"type":"string"}}}},"id":"e{F}"}}"#
        )
    );
    assert!(api::get_entity_schema(format!("e{F}").as_bytes()).is_none());
    assert!(!api::entity_type_exists(format!("e{F}").as_bytes()));

    let listed = api::list_entities(E, 0, 0, b"", true, None, b"", b"");
    assert_eq!(
        encoded(&listed),
        r#"[{"id":"a","n":1},{"id":"b","n":2,"s":"x"}]"#
    );
    assert_eq!(api::count_entities(E), 2);
    assert_eq!(api::get_list_of_ids(E, b"", true).expect("ids"), b"a\nb");

    let returned = api::update_entity_schema(E, &doc(r#"{"n":{"type":"number","required":true}}"#));
    let manual = r#"{"_manual":true,"fields":{"n":{"name":"n","required":true,"type":"number"}},"id":"e\ufffd"}"#;
    assert_eq!(
        String::from_utf8(api::encode_document(&returned, &ByteFields::id(E)).expect("encode"))
            .expect("utf8"),
        manual
    );
    assert_eq!(stored(schema_entity()), [entry(E, manual)]);
    assert!(schema::is_manual_schema(E, None));

    assert!(api::write_entity(E, &mut doc(r#"{"id":"c"}"#)).is_empty());

    let err = api::create_entity_type(E).expect_err("existing type");
    assert_eq!(
        err.to_string(),
        format!("entity type 'e{F}' already exists")
    );

    api::delete_entity_type(E).expect("delete type");
    assert!(stored(E).is_empty());
    assert_eq!(stored(schema_entity()), [entry(E, manual)]);

    let err = api::delete_entity_type(format!("e{F}").as_bytes()).expect_err("missing type");
    assert_eq!(
        err.to_string(),
        format!("entity type 'e{F}' does not exist")
    );
}

#[test]
fn document_ids_with_invalid_utf8_are_stored_as_in_go() {
    let _store = open_store();

    assert!(api::write_entity_bytes(b"doc", &mut doc(r#"{"v":1}"#), &ByteFields::id(R)).is_empty());
    assert!(api::write_entity(b"doc", &mut doc(r#"{"id":"t","v":9}"#)).is_empty());
    assert_eq!(
        stored(b"doc"),
        [
            entry(b"t", r#"{"id":"t","v":9}"#),
            entry(R, r#"{"id":"\ufffd","v":1}"#),
        ]
    );

    let read = api::read_entity_by_id(b"doc", R).expect("raw id");
    assert_eq!(encoded(&read), format!(r#"{{"id":"{F}","v":1}}"#));
    assert!(api::read_entity_by_id(b"doc", F.as_bytes()).is_none());
    assert_eq!(
        api::read_entity_bytes_by_id(b"doc", R)
            .expect("bytes")
            .to_vec(),
        br#"{"id":"\ufffd","v":1}"#
    );

    let updated =
        api::update_entity_by_id(b"doc", R, &doc(r#"{"v":2,"id":"zzz"}"#)).expect("update raw id");
    assert_eq!(
        String::from_utf8(api::encode_document(&updated, &ByteFields::id(R)).expect("encode"))
            .expect("utf8"),
        r#"{"id":"\ufffd","v":2}"#
    );
    assert!(api::update_entity_by_id(b"doc", F.as_bytes(), &doc(r#"{"v":3}"#)).is_none());
    assert_eq!(
        stored(b"doc"),
        [
            entry(b"t", r#"{"id":"t","v":9}"#),
            entry(R, r#"{"id":"\ufffd","v":2}"#),
        ]
    );

    assert!(api::entity_exists(b"doc", R));
    assert!(!api::entity_exists(b"doc", F.as_bytes()));
    assert_eq!(
        api::get_list_of_ids(b"doc", b"v", true).expect("ids"),
        b"\xff\nt"
    );
    assert_eq!(
        api::get_sorted_entity_ids_by_field(b"doc", b"v", false),
        [b"t".to_vec(), R.to_vec()]
    );
    assert_eq!(
        encoded(&api::list_entities(
            b"doc", 0, 0, b"v", true, None, b"", b""
        )),
        format!(r#"[{{"id":"{F}","v":2}},{{"id":"t","v":9}}]"#)
    );

    api::commit_batch(&[BatchOperation {
        kind: "update".to_string(),
        entity: b"doc".to_vec(),
        id: R.to_vec(),
        data: doc(r#"{"v":5}"#),
    }])
    .expect("batch");
    assert_eq!(
        stored(b"doc"),
        [
            entry(b"t", r#"{"id":"t","v":9}"#),
            entry(R, r#"{"id":"\ufffd","v":5}"#),
        ]
    );

    let mut simulation = WriteSimulation::new(
        Box::new(api::get_entity_schema),
        Box::new(api::read_entity_by_id),
    );
    assert!(simulation.update(b"doc", R, &doc(r#"{"v":6}"#)));
    let simulated = simulation.read(b"doc", R).expect("simulated raw id");
    assert_eq!(
        String::from_utf8(api::encode_document(&simulated, &ByteFields::id(R)).expect("encode"))
            .expect("utf8"),
        r#"{"id":"\ufffd","v":6}"#
    );
    assert!(simulation.read(b"doc", F.as_bytes()).is_none());

    api::delete_entity_by_id(b"doc", R);
    assert_eq!(stored(b"doc"), [entry(b"t", r#"{"id":"t","v":9}"#)]);
    assert_eq!(
        stored(schema_entity()),
        [entry(
            b"doc",
            r#"{"fields":{"v":{"name":"v","required":false,"type":"number"}},"id":"doc"}"#
        )]
    );
    assert!(matches!(
        api::read_entity_by_id(b"doc", b"t").and_then(|m| m.get("v").and_then(Value::as_i64)),
        Some(9)
    ));
}
