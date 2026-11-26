use std::collections::BTreeMap;
use std::sync::Arc;

use elysiandb::api;
use elysiandb::cache;
use elysiandb::configuration::{Config, StoreConfig};
use elysiandb::crypto::sha256;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::strconv;
use elysiandb::gocompat::time::Duration;
use elysiandb::storage;
use elysiandb::transport::http::api::decode_query_payload;

use super::common::{hex, read_jsonl, str_field, unhex};
use super::serial;
use super::tempdir::TempDir;

const REL: &str = "api/cache_keys.jsonl";
const ROWS: usize = 21;
const HASH_QUERY_ROWS: usize = 7;
const QUERY_HASH_ROWS: usize = 10;
const ENTITY_VERSION_ROWS: usize = 4;
const LOSSY_CASE: i64 = 6;

type Filters = BTreeMap<Vec<u8>, BTreeMap<Vec<u8>, Vec<u8>>>;

fn int_field(row: &Value, key: &str) -> i64 {
    row.get(key)
        .and_then(Value::as_i64)
        .unwrap_or_else(|| panic!("missing integer field {key} in {row:?}"))
}

fn bool_field(row: &Value, key: &str) -> bool {
    row.get(key)
        .and_then(Value::as_bool)
        .unwrap_or_else(|| panic!("missing boolean field {key} in {row:?}"))
}

fn hex_field(row: &Value, key: &str) -> Vec<u8> {
    unhex(str_field(row, key))
}

fn recorded_filters(row: &Value) -> Filters {
    match row.get("filters") {
        Some(Value::Null) | None => Filters::new(),
        Some(Value::Object(fields)) => fields
            .iter()
            .map(|(field, ops)| {
                let Some(ops) = ops.as_object() else {
                    panic!("bad operators {ops:?}");
                };
                let ops = ops
                    .iter()
                    .map(|(op, v)| {
                        let Some(v) = v.as_str() else {
                            panic!("bad value {v:?}");
                        };
                        (op.as_bytes().to_vec(), v.as_bytes().to_vec())
                    })
                    .collect();
                (field.as_bytes().to_vec(), ops)
            })
            .collect(),
        Some(other) => panic!("bad filters {other:?}"),
    }
}

fn lossy_case_filters(recorded: &Filters) -> Filters {
    let replacement = "\u{fffd}".as_bytes().to_vec();
    let want: Filters = BTreeMap::from([(
        replacement.clone(),
        BTreeMap::from([(replacement.clone(), replacement)]),
    )]);
    assert_eq!(
        *recorded, want,
        "case {LOSSY_CASE} must be the lossy record"
    );

    BTreeMap::from([(vec![0xff], BTreeMap::from([(vec![0xfe], vec![0xfd])]))])
}

fn check_hash_query(row: &Value) {
    let case = int_field(row, "case");
    let recorded = recorded_filters(row);
    let filters = if case == LOSSY_CASE {
        lossy_case_filters(&recorded)
    } else {
        recorded
    };

    let got = cache::hash_query(
        &hex_field(row, "entity"),
        int_field(row, "limit"),
        int_field(row, "offset"),
        &hex_field(row, "sort_field"),
        bool_field(row, "asc"),
        &filters,
        &hex_field(row, "fields"),
        &hex_field(row, "search"),
        &hex_field(row, "includes"),
        bool_field(row, "count"),
        &hex_field(row, "user"),
    );

    assert_eq!(hex(&got), str_field(row, "hex"), "case {case}");
}

fn check_query_hash(row: &Value, seen: &mut BTreeMap<String, String>) {
    let payload = str_field(row, "payload");
    let decoded = json::unmarshal_value(payload.as_bytes())
        .unwrap_or_else(|e| panic!("bad payload {payload}: {e}"));
    let entity = match decoded.get("entity") {
        Some(Value::String(e)) => e.clone(),
        other => panic!("no entity in {payload}: {other:?}"),
    };
    let payload_hash = str_field(row, "payload_hash");
    assert_eq!(payload_hash.len(), 2 * sha256::SIZE, "{row:?}");
    let decoded_payload = decode_query_payload(payload.as_bytes())
        .unwrap_or_else(|e| panic!("bad payload {payload}: {e}"));
    assert_eq!(hex(&decoded_payload.hash()), payload_hash, "{row:?}");

    if let Some(previous) = seen.insert(payload.to_string(), payload_hash.to_string()) {
        assert_eq!(previous, payload_hash, "{row:?}");
    }

    let mut h = sha256::Digest::new();
    h.write(entity.as_bytes());
    h.write(&unhex(payload_hash));
    h.write(str_field(row, "user").as_bytes());

    assert_eq!(hex(&h.sum()), str_field(row, "hex"), "{row:?}");
}

fn write_entity(entity: &[u8], id: &str) {
    let mut doc = Map::from([("id".to_string(), Value::String(id.to_string()))]);
    api::write_entity(entity, &mut doc);
}

fn rest_of_version(version: &str, instance: &str) -> String {
    let Some(rest) = version.strip_prefix(instance) else {
        panic!("{version} does not start with the instance {instance}");
    };
    rest.to_string()
}

struct VersionOfGuard(Option<cache::VersionOf>);

impl Drop for VersionOfGuard {
    fn drop(&mut self) {
        cache::set_version_of(self.0.take());
        let _ = storage::close();
    }
}

fn check_entity_versions(rows: &[&Value]) -> usize {
    let dir = TempDir::new();
    let mut cfg = Config {
        store: StoreConfig {
            folder: dir.path().to_string_lossy().into_owned(),
            shards: 4,
            ..StoreConfig::default()
        },
        ..Config::default()
    };
    cfg.log.level = "error".to_string();
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();
    let _guard = VersionOfGuard(cache::set_version_of(Some(api::index::entity_version)));
    cache::init_cache(Duration::SECOND * 30);

    let Some(e) = storage::engine() else {
        panic!("the store is not open");
    };
    let instance = strconv::format_uint(e.instance(), 36);
    api::delete_all();

    let Some(version_of) = cache::version_of() else {
        panic!("the version hook is not set");
    };

    let mut checked = 0;
    for row in rows {
        match str_field(row, "step") {
            "empty" => {}
            "one-write" => write_entity(b"book", "b1"),
            "forty-one-writes" => {
                for i in 0..40 {
                    write_entity(b"book", &format!("x{i}"));
                }
            }
            "after-reset" => api::delete_all_entities(b"book"),
            other => panic!("unknown step {other}"),
        }

        let go_instance = strconv::format_uint(
            u64::try_from(int_field(row, "store")).unwrap_or_else(|e| panic!("{e}")),
            36,
        );
        for key in ["book", "none"] {
            let want = rest_of_version(str_field(row, key), &go_instance);
            let got = rest_of_version(&version_of(key.as_bytes()), &instance);
            assert_eq!(got, want, "{key} at {row:?}");
        }
        checked += 1;
    }

    checked
}

#[test]
fn cache_keys_match_go() {
    let _serial = serial();
    let rows = read_jsonl(REL);
    assert_eq!(rows.len(), ROWS);

    let (mut hash_query, mut query_hash) = (0, 0);
    let mut seen = BTreeMap::new();
    let mut versions: Vec<&Value> = Vec::new();

    for row in &rows {
        match str_field(row, "fn") {
            "HashQuery" => {
                check_hash_query(row);
                hash_query += 1;
            }
            "QueryHash" => {
                check_query_hash(row, &mut seen);
                query_hash += 1;
            }
            "entityVersion" => versions.push(row),
            other => panic!("unknown function {other}"),
        }
    }

    let entity_versions = check_entity_versions(&versions);

    assert_eq!(hash_query, HASH_QUERY_ROWS);
    assert_eq!(query_hash, QUERY_HASH_ROWS);
    assert_eq!(entity_versions, ENTITY_VERSION_ROWS);
    assert_eq!(hash_query + query_hash + entity_versions, ROWS);
}
