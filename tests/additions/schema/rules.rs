use elysiandb::gocompat::json::{Map, Value};
use elysiandb::schema::{self, ValidationError};

use super::{obj, object, s, serial, set_schema_config};

fn typed(ty: &str, required: bool) -> Value {
    object(obj(vec![
        ("type", s(ty)),
        ("required", Value::Bool(required)),
    ]))
}

fn with_fields(ty: &str, fields: Map) -> Value {
    object(obj(vec![("type", s(ty)), ("fields", object(fields))]))
}

fn manual(fields: Map) -> Map {
    obj(vec![
        ("id", s("o")),
        ("_manual", Value::Bool(true)),
        ("fields", object(fields)),
    ])
}

fn pairs(errs: &[ValidationError]) -> Vec<(&str, &str)> {
    errs.iter()
        .map(|e| (e.field.as_str(), e.message.as_str()))
        .collect()
}

#[test]
fn test_analyze_entity_schema_keeps_nested_ids_and_skips_core_fields() {
    let _serial = serial();
    set_schema_config(false);

    let doc = obj(vec![
        ("id", s("1")),
        ("_elysiandb_core_owner", s("u")),
        (
            "ref",
            object(obj(vec![
                ("@entity", s("person")),
                ("id", s("p1")),
                ("_elysiandb_core_x", Value::Number(1.0)),
            ])),
        ),
        (
            "items",
            Value::Array(vec![
                object(obj(vec![("a", Value::Number(1.0))])),
                object(obj(vec![("b", s("x"))])),
            ]),
        ),
        (
            "mixed",
            Value::Array(vec![Value::Number(1.0), object(obj(vec![("c", s("x"))]))]),
        ),
    ]);

    let analyzed = schema::analyze_entity_schema(b"r", &doc);
    let encoded = elysiandb::gocompat::json::marshal(&analyzed).unwrap_or_else(|e| panic!("{e}"));
    let want = concat!(
        r#"{"fields":{"items":{"fields":{"a":{"name":"a","required":false,"type":"number"}},"#,
        r#""name":"items","required":false,"type":"array"},"#,
        r#""mixed":{"name":"mixed","required":false,"type":"array"},"#,
        r#""ref":{"fields":{"@entity":{"name":"@entity","required":false,"type":"string"},"#,
        r#""id":{"name":"id","required":false,"type":"string"}},"#,
        r#""name":"ref","required":false,"type":"object"}},"id":"r"}"#,
    );
    if String::from_utf8_lossy(&encoded) != want {
        panic!("unexpected schema {}", String::from_utf8_lossy(&encoded));
    }
}

#[test]
fn test_validate_entity_skips_ids_at_every_depth() {
    let _serial = serial();
    set_schema_config(true);

    let sc = manual(obj(vec![
        ("id", typed("number", true)),
        (
            "o",
            with_fields(
                "object",
                obj(vec![
                    ("id", typed("number", true)),
                    ("k", typed("string", false)),
                ]),
            ),
        ),
    ]));
    let doc = obj(vec![
        ("id", s("x")),
        (
            "o",
            object(obj(vec![("id", s("y")), ("_elysiandb_core_owner", s("u"))])),
        ),
    ]);

    let errs = schema::validate_entity(b"o", &doc, Some(&sc));
    if !errs.is_empty() {
        panic!("expected ids to be skipped, got {:?}", pairs(&errs));
    }
}

#[test]
fn test_validate_entity_update_keeps_errors_of_updated_roots() {
    let _serial = serial();
    set_schema_config(true);

    let sc = manual(obj(vec![
        ("a", typed("string", true)),
        ("b", typed("string", true)),
        (
            "o",
            with_fields("object", obj(vec![("k", typed("string", true))])),
        ),
    ]));
    let existing = obj(vec![("b", Value::Number(1.0)), ("o", object(Map::new()))]);

    let errs = schema::validate_entity_update(
        b"o",
        Some(&existing),
        &obj(vec![("c", Value::Bool(true))]),
        Some(&sc),
    );
    let want = vec![
        ("c", "field not allowed by strict schema"),
        ("a", "required field missing"),
    ];
    if pairs(&errs) != want {
        panic!("unexpected errors {:?}", pairs(&errs));
    }
}

#[test]
fn test_validate_entity_reports_errors_in_d20_order() {
    let _serial = serial();
    set_schema_config(true);

    let sc = manual(obj(vec![
        (
            "a",
            with_fields("object", obj(vec![("x", typed("number", true))])),
        ),
        ("a-b", typed("string", true)),
        (
            "items",
            with_fields("array", obj(vec![("x", typed("number", true))])),
        ),
        ("z", typed("boolean", true)),
    ]));

    let mut items: Vec<Value> = (0..11)
        .map(|_| object(obj(vec![("x", Value::Number(1.0))])))
        .collect();
    items[2] = object(obj(vec![("x", s("two")), ("q", Value::Null)]));
    items[10] = object(obj(vec![("x", s("ten"))]));

    let doc = obj(vec![
        (
            "a",
            object(obj(vec![("x", s("no")), ("y", Value::Bool(true))])),
        ),
        ("a-b", Value::Number(3.0)),
        ("b", Value::Null),
        ("items", Value::Array(items)),
    ]);

    let errs = schema::validate_entity(b"o", &doc, Some(&sc));
    let want = vec![
        ("a.y", "field not allowed by strict schema"),
        ("b", "field not allowed by strict schema"),
        ("items[2].q", "field not allowed by strict schema"),
        ("a.x", "expected type number but got string"),
        ("a-b", "expected type string but got number"),
        ("items[2].x", "expected type number but got string"),
        ("items[10].x", "expected type number but got string"),
        ("z", "required field missing"),
    ];
    if pairs(&errs) != want {
        panic!("unexpected order {:?}", pairs(&errs));
    }

    let encoded = elysiandb::gocompat::json::marshal(&errs).unwrap_or_else(|e| panic!("{e}"));
    let first = r#"[{"Field":"a.y","Message":"field not allowed by strict schema"},"#;
    if !String::from_utf8_lossy(&encoded).starts_with(first) {
        panic!("unexpected encoding {}", String::from_utf8_lossy(&encoded));
    }
}

#[test]
fn test_validate_entity_displays_the_included_documents_type() {
    let _serial = serial();
    set_schema_config(false);

    let sc = obj(vec![(
        "fields",
        object(obj(vec![
            ("members", typed("[]map[string]interface {}", false)),
            ("tags", typed("array", false)),
        ])),
    )]);
    let doc = obj(vec![
        ("members", Value::Array(Vec::new())),
        ("tags", Value::DocList(vec![Map::new()])),
    ]);

    let errs = schema::validate_entity(b"t", &doc, Some(&sc));
    let want = vec![
        (
            "members",
            "expected type array of included documents but got array",
        ),
        (
            "tags",
            "expected type array but got array of included documents",
        ),
    ];
    if pairs(&errs) != want {
        panic!("unexpected errors {:?}", pairs(&errs));
    }

    let analyzed = schema::analyze_entity_schema(b"t", &doc);
    let encoded = elysiandb::gocompat::json::marshal(&analyzed).unwrap_or_else(|e| panic!("{e}"));
    let want = r#"{"fields":{"members":{"name":"members","required":false,"type":"array"},"tags":{"name":"tags","required":false,"type":"[]map[string]interface {}"}},"id":"t"}"#;
    if String::from_utf8_lossy(&encoded) != want {
        panic!("unexpected schema {}", String::from_utf8_lossy(&encoded));
    }
}

#[test]
fn test_validation_error_to_error() {
    let e = ValidationError::new("a.b", "required field missing");
    if e.to_error().to_string() != "Field 'a.b': required field missing" {
        panic!("unexpected error text {}", e.to_error());
    }
}
