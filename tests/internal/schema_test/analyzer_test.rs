use elysiandb::gocompat::json::{Map, Value};
use elysiandb::schema::{self, Entity, Field};

use super::{
    fields, obj, object, patch_get_json_by_key, patch_load_schema, patch_manual_flag, patch_strict,
    s, serial, sub_map,
};

#[test]
fn analyze_entity_schema_simple() {
    let _serial = serial();
    let _restore = patch_strict(false);

    let data = obj(vec![
        ("id", s("u1")),
        ("name", s("Alice")),
        ("age", Value::Int(30)),
    ]);

    let result = schema::analyze_entity_schema(b"users", &data);
    let fields = sub_map(&result, "fields");

    if sub_map(fields, "name").get("required") != Some(&Value::Bool(false)) {
        panic!("expected required=false");
    }
    if sub_map(fields, "age").get("required") != Some(&Value::Bool(false)) {
        panic!("expected required=false");
    }
}

#[test]
fn analyze_entity_schema_strict_required_true() {
    let _serial = serial();
    let _restore = patch_strict(true);

    let data = obj(vec![("name", s("Alice"))]);

    let result = schema::analyze_entity_schema(b"users", &data);
    let fields = sub_map(&result, "fields");

    if sub_map(fields, "name").get("required") != Some(&Value::Bool(true)) {
        panic!("expected required=true");
    }
}

#[test]
fn analyze_entity_schema_nested() {
    let _serial = serial();
    let _restore = patch_strict(false);

    let data = obj(vec![
        ("id", s("p1")),
        ("name", s("Post")),
        (
            "author",
            object(obj(vec![("name", s("John")), ("age", Value::Int(42))])),
        ),
    ]);

    let result = schema::analyze_entity_schema(b"posts", &data);
    let fields = sub_map(&result, "fields");
    let author = sub_map(fields, "author");
    let sub = sub_map(author, "fields");

    if sub_map(sub, "name").get("required") != Some(&Value::Bool(false)) {
        panic!("expected required=false");
    }
}

#[test]
fn analyze_entity_schema_array_nested() {
    let _serial = serial();
    let _restore = patch_strict(false);

    let data = obj(vec![(
        "tags",
        Value::Array(vec![object(obj(vec![
            ("label", s("go")),
            ("score", Value::Int(1)),
        ]))]),
    )]);

    let result = schema::analyze_entity_schema(b"items", &data);
    let fields = sub_map(&result, "fields");
    let tags = sub_map(fields, "tags");
    let sub = sub_map(tags, "fields");

    if sub_map(sub, "label").get("required") != Some(&Value::Bool(false)) {
        panic!("expected required=false");
    }
}

#[test]
fn detect_json_type() {
    let tests = vec![
        (s("x"), "string"),
        (Value::Int(30), "number"),
        (Value::Bool(true), "boolean"),
        (object(obj(vec![("a", Value::Int(1))])), "object"),
        (Value::Array(vec![Value::Int(1), Value::Int(2)]), "array"),
        (Value::Null, "unknown"),
    ];

    for (input, exp) in &tests {
        let got = schema::detect_json_type(input);
        if got != *exp {
            panic!("expected {exp} got {got}");
        }
    }
}

#[test]
fn validate_entity_strict_new_field_rejected() {
    let _serial = serial();
    let mock_schema = Entity {
        id: b"users".to_vec(),
        fields: fields(vec![("name", Field::new("name", "string", true))]),
    };

    let _restore1 = patch_load_schema(Some(mock_schema));
    let _restore2 = patch_manual_flag(true);
    let _restore3 = patch_strict(true);

    let data = obj(vec![("name", s("Alice")), ("age", Value::Int(30))]);

    let errs = schema::validate_entity(b"users", &data, None);
    if errs.len() != 1 {
        panic!("expected 1 error");
    }
}

#[test]
fn validate_entity_strict_required_missing() {
    let _serial = serial();
    let mock_schema = Entity {
        id: b"users".to_vec(),
        fields: fields(vec![
            ("name", Field::new("name", "string", true)),
            ("age", Field::new("age", "number", true)),
        ]),
    };

    let _restore1 = patch_load_schema(Some(mock_schema));
    let _restore2 = patch_manual_flag(true);
    let _restore3 = patch_strict(true);

    let data = obj(vec![("name", s("Alice"))]);

    let errs = schema::validate_entity(b"users", &data, None);
    if errs.len() != 1 {
        panic!("expected 1 error missing required=age");
    }
}

#[test]
fn validate_entity_strict_deep_new_field_rejected() {
    let _serial = serial();
    let mock_schema = Entity {
        id: b"books".to_vec(),
        fields: fields(vec![(
            "author",
            Field::new("author", "object", true).with_fields(fields(vec![
                ("name", Field::new("name", "string", true)),
                ("age", Field::new("age", "number", true)),
            ])),
        )]),
    };

    let _restore1 = patch_load_schema(Some(mock_schema));
    let _restore2 = patch_manual_flag(true);
    let _restore3 = patch_strict(true);

    let data = obj(vec![(
        "author",
        object(obj(vec![
            ("name", s("Alice")),
            ("age", Value::Int(40)),
            ("extra", s("no")),
            ("nested", object(obj(vec![("x", Value::Int(1))]))),
        ])),
    )]);

    let errs = schema::validate_entity(b"books", &data, None);
    if errs.len() != 2 {
        panic!("expected 2 errors");
    }
}

#[test]
fn validate_entity_strict_core_fields_allowed() {
    let _serial = serial();
    let mock_schema = Entity {
        id: b"oeuvre".to_vec(),
        fields: fields(vec![("titre", Field::new("titre", "string", true))]),
    };

    let _restore1 = patch_load_schema(Some(mock_schema));
    let _restore2 = patch_manual_flag(true);
    let _restore3 = patch_strict(true);

    let data = obj(vec![
        ("id", s("1")),
        ("titre", s("Alien")),
        ("_elysiandb_core_username", s("admin")),
    ]);

    let errs = schema::validate_entity(b"oeuvre", &data, None);
    if !errs.is_empty() {
        panic!("expected no errors, got {errs:?}");
    }
}

#[test]
fn validate_entity_type_mismatch() {
    let _serial = serial();
    let mock_schema = Entity {
        id: b"u".to_vec(),
        fields: fields(vec![("age", Field::new("age", "number", false))]),
    };

    let _restore = patch_load_schema(Some(mock_schema));

    let data = obj(vec![("age", s("wrong"))]);

    let errs = schema::validate_entity(b"u", &data, None);
    if errs.len() != 1 {
        panic!("expected 1 error");
    }
}

#[test]
fn validate_entity_no_schema() {
    let _serial = serial();
    let _restore = patch_load_schema(None);

    let errs = schema::validate_entity(b"x", &obj(vec![("a", Value::Int(1))]), None);
    if !errs.is_empty() {
        panic!("expected no errors");
    }
}

#[test]
fn is_manual_schema_no_flag() {
    let _serial = serial();
    let _restore = patch_manual_flag(false);

    if schema::is_manual_schema(b"unknown", None) {
        panic!("expected false");
    }
}

#[test]
fn is_manual_schema_yes_flag() {
    let _serial = serial();
    let _restore = patch_manual_flag(true);

    if !schema::is_manual_schema(b"e", None) {
        panic!("expected true");
    }
}

#[test]
fn fields_to_map_and_back() {
    let fields = fields(vec![(
        "name",
        Field::new("name", "string", true)
            .with_fields(fields(vec![("sub", Field::new("sub", "number", false))])),
    )]);

    let m = schema::fields_to_map(&fields);
    let rev = schema::map_to_fields(&m);

    if !rev["name"].required {
        panic!("expected required=true");
    }
    if rev["name"].fields["sub"].required {
        panic!("expected required=false");
    }
}

#[test]
fn schema_entity_to_storable_structure() {
    let e = Entity {
        id: b"x".to_vec(),
        fields: fields(vec![("a", Field::new("a", "string", true))]),
    };

    let out = schema::schema_entity_to_storable_structure(&e);
    if out.get("id") != Some(&s("x")) {
        panic!("wrong id");
    }

    let f = sub_map(&out, "fields");
    let a = sub_map(f, "a");
    if a.get("required") != Some(&Value::Bool(true)) {
        panic!("required not exported");
    }
}

#[test]
fn analyze_entity_schema_ignores_id() {
    let _serial = serial();
    let _restore = patch_strict(false);

    let data = obj(vec![("id", s("abc")), ("name", s("hello"))]);

    let result = schema::analyze_entity_schema(b"test", &data);
    let fields = sub_map(&result, "fields");

    if fields.contains_key("id") {
        panic!("id field should not be included");
    }
    if !fields.contains_key("name") {
        panic!("expected name field");
    }
}

#[test]
fn analyze_entity_schema_array_no_subfields() {
    let _serial = serial();
    let _restore = patch_strict(false);

    let data = obj(vec![("items", Value::Array(Vec::new()))]);

    let result = schema::analyze_entity_schema(b"arr", &data);
    let fields = sub_map(&result, "fields");

    let item = sub_map(fields, "items");
    if item.get("type") != Some(&s("array")) {
        panic!("expected array");
    }
    if item.contains_key("fields") {
        panic!("expected no subfields for empty array");
    }
}

#[test]
fn validate_entity_array_object_validation() {
    let _serial = serial();
    let mock = Entity {
        id: b"e".to_vec(),
        fields: fields(vec![(
            "items",
            Field::new("items", "array", true)
                .with_fields(fields(vec![("label", Field::new("label", "string", true))])),
        )]),
    };

    let _restore1 = patch_load_schema(Some(mock));

    let data = obj(vec![(
        "items",
        Value::Array(vec![
            object(obj(vec![("label", s("ok"))])),
            object(obj(vec![("label", Value::Int(123))])),
        ]),
    )]);

    let errs = schema::validate_entity(b"e", &data, None);
    if errs.len() != 1 {
        panic!("expected 1 error");
    }
}

#[test]
fn validate_entity_array_missing_required() {
    let _serial = serial();
    let mock = Entity {
        id: b"test".to_vec(),
        fields: fields(vec![(
            "arr",
            Field::new("arr", "array", true)
                .with_fields(fields(vec![("n", Field::new("n", "number", true))])),
        )]),
    };

    let _restore1 = patch_load_schema(Some(mock));
    let _restore2 = patch_strict(true);
    let _restore3 = patch_manual_flag(true);

    let data = obj(vec![("arr", Value::Array(vec![object(Map::new())]))]);

    let errs = schema::validate_entity(b"test", &data, None);
    if errs.len() != 1 {
        panic!("expected missing required error");
    }
}

#[test]
fn validate_entity_object_wrong_type() {
    let _serial = serial();
    let mock = Entity {
        id: b"wrong".to_vec(),
        fields: fields(vec![("meta", Field::new("meta", "object", false))]),
    };

    let _restore = patch_load_schema(Some(mock));

    let data = obj(vec![("meta", s("not-an-object"))]);

    let errs = schema::validate_entity(b"wrong", &data, None);
    if errs.len() != 1 {
        panic!("expected type mismatch error");
    }
}

#[test]
fn validate_entity_array_wrong_type() {
    let _serial = serial();
    let mock = Entity {
        id: b"wrong".to_vec(),
        fields: fields(vec![("items", Field::new("items", "array", false))]),
    };

    let _restore = patch_load_schema(Some(mock));

    let data = obj(vec![("items", s("not-array"))]);

    let errs = schema::validate_entity(b"wrong", &data, None);
    if errs.len() != 1 {
        panic!("expected array mismatch error");
    }
}

#[test]
fn map_to_fields_name_override() {
    let m = obj(vec![(
        "x",
        object(obj(vec![
            ("name", s("real")),
            ("type", s("string")),
            ("required", Value::Bool(true)),
        ])),
    )]);

    let fields = schema::map_to_fields(&m);

    if !fields.contains_key("real") {
        panic!("expected key 'real'");
    }
}

#[test]
fn load_schema_for_entity_no_data() {
    let _serial = serial();
    let _restore = patch_get_json_by_key(None);

    let res = schema::load_schema_for_entity(b"x", None);
    if res.is_none() {
        return;
    }
    panic!("expected nil schema");
}

#[test]
fn load_schema_for_entity_with_data() {
    let _serial = serial();
    let _restore = patch_get_json_by_key(Some(obj(vec![(
        "fields",
        object(obj(vec![(
            "a",
            object(obj(vec![
                ("name", s("a")),
                ("type", s("string")),
                ("required", Value::Bool(true)),
            ])),
        )])),
    )])));

    let Some(res) = schema::load_schema_for_entity(b"x", None) else {
        panic!("expected schema");
    };
    if !res.fields.contains_key("a") {
        panic!("expected field a");
    }
}
