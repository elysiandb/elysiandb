use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::schema::{self, ValidationError};

use super::{serial, set_schema_config, sub_map};

const LEVELS: usize = 9_990;
const SMALL_STACK: usize = 64 << 10;

fn nested(leaf: Value) -> Map {
    let mut v = leaf;
    let mut depth = 1usize;
    let mut level = 0usize;
    while depth < LEVELS {
        if level % 3 == 2 && depth + 2 <= LEVELS {
            v = Value::Array(vec![Value::Object(Map::from([("c".to_string(), v)]))]);
            depth += 2;
        } else {
            v = Value::Object(Map::from([
                ("c".to_string(), v),
                ("n".to_string(), Value::Number(level as f64)),
            ]));
            depth += 1;
        }
        level += 1;
    }

    Map::from([("c".to_string(), v)])
}

fn levels(doc: &Map) -> usize {
    let mut n = 1usize;
    let mut cur = doc.get("c");
    while let Some(v) = cur {
        cur = match v {
            Value::Object(m) => {
                n += 1;
                m.get("c")
            }
            Value::Array(a) => match a.first() {
                Some(Value::Object(m)) => {
                    n += 2;
                    m.get("c")
                }
                _ => None,
            },
            _ => None,
        };
    }

    n
}

fn leaf_path(doc: &Map) -> (String, String) {
    let mut path = String::from("c");
    let mut parent = String::new();
    let mut cur = doc.get("c");
    while let Some(v) = cur {
        cur = match v {
            Value::Object(m) => {
                parent = path.clone();
                path.push_str(".c");
                m.get("c")
            }
            Value::Array(a) => match a.first() {
                Some(Value::Object(m)) => {
                    path.push_str("[0]");
                    parent = path.clone();
                    path.push_str(".c");
                    m.get("c")
                }
                _ => None,
            },
            _ => None,
        };
    }

    (path, parent)
}

fn deepest_object(doc: &mut Map) -> &mut Map {
    let mut cur = doc;
    loop {
        let container = matches!(cur.get("c"), Some(Value::Object(_) | Value::Array(_)));
        if !container {
            return cur;
        }
        cur = match cur.get_mut("c") {
            Some(Value::Object(m)) => m,
            Some(Value::Array(a)) => match a.first_mut() {
                Some(Value::Object(m)) => m,
                _ => panic!("an array level has no object"),
            },
            _ => panic!("unexpected level"),
        };
    }
}

fn run() {
    let doc = nested(Value::String("bottom".to_string()));
    if levels(&doc) != LEVELS {
        panic!(
            "the document must be nested on {LEVELS} levels, got {}",
            levels(&doc)
        );
    }

    let analyzed = schema::analyze_entity_schema(b"deep", &doc);
    let merged = schema::merge_analyzed_schema(b"deep", Some(&analyzed), &doc);
    if merged != analyzed {
        panic!("merging a document into its own schema must not change it");
    }

    let encoded = json::marshal(&analyzed).unwrap_or_else(|e| panic!("{e}"));
    let merged_encoded = json::marshal(&merged).unwrap_or_else(|e| panic!("{e}"));
    if encoded != merged_encoded {
        panic!("the encoded schemas differ");
    }

    let fields = schema::map_to_fields(sub_map(&analyzed, "fields"));
    let back = schema::fields_to_map(&fields);
    if &back != sub_map(&analyzed, "fields") {
        panic!("the field map does not survive a round trip");
    }

    let copy = fields.clone();
    drop(fields);
    if schema::fields_to_map(&copy) != back {
        panic!("the cloned fields differ");
    }
    drop(copy);

    let mut schema_data = analyzed.clone();
    schema_data.insert("_manual".to_string(), Value::Bool(true));

    let errs = schema::validate_entity(b"deep", &doc, Some(&schema_data));
    if !errs.is_empty() {
        panic!("expected no errors, got {}", errs.len());
    }

    let (path, parent) = leaf_path(&doc);
    let type_error = ValidationError::new(path, "expected type string but got number");

    let mut bad = nested(Value::Number(1.0));
    let errs = schema::validate_entity(b"deep", &bad, Some(&schema_data));
    if errs != vec![type_error.clone()] {
        panic!("expected the type error of the deepest field");
    }

    deepest_object(&mut bad).insert("extra".to_string(), Value::Bool(true));
    let extra_error = ValidationError::new(
        format!("{parent}.extra"),
        "field not allowed by strict schema",
    );
    let errs = schema::validate_entity(b"deep", &bad, Some(&schema_data));
    if errs != vec![extra_error.clone(), type_error.clone()] {
        panic!("expected the strict error first, then the type error");
    }

    let updates = Map::from([(
        "c".to_string(),
        bad.get("c").cloned().unwrap_or(Value::Null),
    )]);
    let errs = schema::validate_entity_update(b"deep", Some(&doc), &updates, Some(&schema_data));
    if errs != vec![extra_error, type_error] {
        panic!("expected the errors of the updated field");
    }

    drop(bad);
    drop(updates);
    drop(schema_data);
    drop(merged);
    drop(analyzed);
    drop(doc);
}

#[test]
fn test_schema_handles_9990_levels_on_a_small_stack() {
    let _serial = serial();
    set_schema_config(true);

    let worker = std::thread::Builder::new()
        .name("deep-schema".to_string())
        .stack_size(SMALL_STACK)
        .spawn(run)
        .unwrap_or_else(|e| panic!("cannot start the thread: {e}"));

    if worker.join().is_err() {
        panic!("the schema functions failed on a small stack");
    }
}
