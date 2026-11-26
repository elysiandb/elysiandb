use elysiandb::gocompat::json::{Map, Value};
use elysiandb::schema::{self, MESSAGE_REQUIRED_FIELD_MISSING, MESSAGE_REQUIRED_FIELD_NULL};

use super::{obj, object, s, serial, set_schema_config};

fn field_of<'a>(storable: &'a Map, path: &[&str]) -> &'a Map {
    let mut current = storable;
    for name in path {
        let Some(Value::Object(fields)) = current.get("fields") else {
            panic!("no fields in {current:?}");
        };

        let Some(Value::Object(field)) = fields.get(*name) else {
            panic!("field {name} missing in {fields:?}");
        };

        current = field;
    }

    current
}

fn field_type(field: &Map) -> Option<&str> {
    field.get("type").and_then(Value::as_str)
}

#[test]
fn merge_analyzed_schema_keeps_absent_fields_and_takes_the_latest_types() {
    let _serial = serial();
    set_schema_config(false);

    let existing =
        schema::analyze_entity_schema(b"inf", &obj(vec![("a", s("x")), ("b", Value::Number(1.0))]));
    let merged = schema::merge_analyzed_schema(
        b"inf",
        Some(&existing),
        &obj(vec![("a", Value::Number(5.0)), ("c", Value::Bool(true))]),
    );

    let got = field_type(field_of(&merged, &["a"]));
    if got != Some("number") {
        panic!("expected a to take the type of the latest write, got {got:?}");
    }

    let got = field_type(field_of(&merged, &["b"]));
    if got != Some("number") {
        panic!("expected b to be kept, got {got:?}");
    }

    let got = field_type(field_of(&merged, &["c"]));
    if got != Some("boolean") {
        panic!("expected c to be added, got {got:?}");
    }

    if merged.get("id") != Some(&s("inf")) {
        panic!("expected the schema id, got {:?}", merged.get("id"));
    }

    if merged.contains_key("_manual") {
        panic!("an inferred schema must stay inferred");
    }

    let manual = obj(vec![
        ("id", s("m")),
        ("_manual", Value::Bool(true)),
        (
            "fields",
            object(obj(vec![(
                "title",
                object(obj(vec![
                    ("name", s("title")),
                    ("type", s("string")),
                    ("required", Value::Bool(true)),
                ])),
            )])),
        ),
    ]);
    let merged = schema::merge_analyzed_schema(
        b"m",
        Some(&manual),
        &obj(vec![("title", Value::Number(1.0))]),
    );
    let field = field_of(&merged, &["title"]);
    if field_type(field) != Some("number") || field.get("required") != Some(&Value::Bool(true)) {
        panic!("expected a type change to keep the required flag, got {field:?}");
    }
}

#[test]
fn merge_analyzed_schema_without_existing_schema() {
    let _serial = serial();
    set_schema_config(true);

    let merged = schema::merge_analyzed_schema(b"inf", None, &obj(vec![("a", s("x"))]));
    let field = field_of(&merged, &["a"]);
    if field_type(field) != Some("string") || field.get("required") != Some(&Value::Bool(true)) {
        panic!("expected the analyzed schema, got {field:?}");
    }
}

#[test]
fn merge_analyzed_schema_upgrades_unknown_types() {
    let _serial = serial();
    set_schema_config(false);

    let existing =
        schema::analyze_entity_schema(b"nul", &obj(vec![("x", Value::Null), ("y", s("s"))]));
    let merged = schema::merge_analyzed_schema(
        b"nul",
        Some(&existing),
        &obj(vec![("x", s("a")), ("y", Value::Null)]),
    );

    let got = field_type(field_of(&merged, &["x"]));
    if got != Some("string") {
        panic!("expected x to take the first concrete type, got {got:?}");
    }

    let got = field_type(field_of(&merged, &["y"]));
    if got != Some("string") {
        panic!("expected a null value to keep the type of y, got {got:?}");
    }
}

#[test]
fn merge_analyzed_schema_merges_nested_objects_and_arrays() {
    let _serial = serial();
    set_schema_config(false);

    let existing = schema::analyze_entity_schema(
        b"n",
        &obj(vec![
            ("author", object(obj(vec![("name", s("a"))]))),
            (
                "items",
                Value::Array(vec![object(obj(vec![("sku", s("x"))]))]),
            ),
        ]),
    );
    let merged = schema::merge_analyzed_schema(
        b"n",
        Some(&existing),
        &obj(vec![
            ("author", object(obj(vec![("age", Value::Number(3.0))]))),
            (
                "items",
                Value::Array(vec![object(obj(vec![("qty", Value::Number(1.0))]))]),
            ),
        ]),
    );

    if field_type(field_of(&merged, &["author", "name"])) != Some("string")
        || field_type(field_of(&merged, &["author", "age"])) != Some("number")
    {
        panic!("expected nested object fields to be merged, got {merged:?}");
    }

    if field_type(field_of(&merged, &["items", "sku"])) != Some("string")
        || field_type(field_of(&merged, &["items", "qty"])) != Some("number")
    {
        panic!("expected array item fields to be merged, got {merged:?}");
    }
}

#[test]
fn merge_analyzed_schema_keeps_manual_flag_and_required_flags() {
    let _serial = serial();
    set_schema_config(false);

    let existing = obj(vec![
        ("id", s("m")),
        ("_manual", Value::Bool(true)),
        (
            "fields",
            object(obj(vec![(
                "title",
                object(obj(vec![
                    ("name", s("title")),
                    ("type", s("string")),
                    ("required", Value::Bool(true)),
                ])),
            )])),
        ),
    ]);

    let merged = schema::merge_analyzed_schema(
        b"m",
        Some(&existing),
        &obj(vec![("title", s("x")), ("extra", Value::Number(1.0))]),
    );

    if merged.get("_manual") != Some(&Value::Bool(true)) {
        panic!("expected the manual flag to be kept, got {merged:?}");
    }

    if field_of(&merged, &["title"]).get("required") != Some(&Value::Bool(true)) {
        panic!("expected the required flag to be kept, got {merged:?}");
    }

    if field_of(&merged, &["extra"]).get("required") != Some(&Value::Bool(false)) {
        panic!("expected new fields to follow the strict setting, got {merged:?}");
    }
}

fn manual_schema(fields: Map) -> Map {
    obj(vec![
        ("id", s("film")),
        ("_manual", Value::Bool(true)),
        ("fields", object(fields)),
    ])
}

fn typed(ty: &str, required: Option<bool>) -> Value {
    let mut m = obj(vec![("type", s(ty))]);
    if let Some(r) = required {
        m.insert("required".to_string(), Value::Bool(r));
    }
    object(m)
}

#[test]
fn validate_entity_null_values() {
    let _serial = serial();
    set_schema_config(true);

    let sc = manual_schema(obj(vec![
        ("title", typed("string", Some(true))),
        ("year", typed("number", Some(false))),
    ]));

    let errs = schema::validate_entity(
        b"film",
        &obj(vec![("title", s("C")), ("year", Value::Null)]),
        Some(&sc),
    );
    if !errs.is_empty() {
        panic!("expected null to be accepted on an optional field, got {errs:?}");
    }

    let errs = schema::validate_entity(b"film", &obj(vec![("title", Value::Null)]), Some(&sc));
    if errs.len() != 1 || errs[0].field != "title" || errs[0].message != MESSAGE_REQUIRED_FIELD_NULL
    {
        panic!("expected null to be rejected on a required field, got {errs:?}");
    }

    set_schema_config(false);
    let inferred = obj(vec![
        ("id", s("film")),
        (
            "fields",
            object(obj(vec![
                ("title", typed("string", Some(true))),
                ("x", typed("unknown", Some(false))),
            ])),
        ),
    ]);

    let errs = schema::validate_entity(
        b"film",
        &obj(vec![("title", Value::Null), ("x", s("any"))]),
        Some(&inferred),
    );
    if !errs.is_empty() {
        panic!("expected null and unknown types to be accepted without strict mode, got {errs:?}");
    }
}

#[test]
fn validate_entity_update_validates_the_merged_document() {
    let _serial = serial();
    set_schema_config(true);

    let sc = manual_schema(obj(vec![
        ("title", typed("string", Some(true))),
        ("year", typed("number", Some(false))),
    ]));
    let existing = obj(vec![
        ("id", s("f1")),
        ("title", s("C")),
        ("year", Value::Number(1983.0)),
    ]);

    let errs = schema::validate_entity_update(
        b"film",
        Some(&existing),
        &obj(vec![("year", Value::Number(1984.0))]),
        Some(&sc),
    );
    if !errs.is_empty() {
        panic!("expected a partial update to pass, got {errs:?}");
    }

    let errs = schema::validate_entity_update(
        b"film",
        Some(&existing),
        &obj(vec![("year", s("x"))]),
        Some(&sc),
    );
    if errs.len() != 1 || errs[0].field != "year" {
        panic!("expected a type error on year, got {errs:?}");
    }

    let errs = schema::validate_entity_update(
        b"film",
        Some(&existing),
        &obj(vec![("extra", Value::Number(1.0))]),
        Some(&sc),
    );
    if errs.len() != 1 || errs[0].field != "extra" {
        panic!("expected an unknown field to be rejected, got {errs:?}");
    }

    let errs = schema::validate_entity_update(
        b"film",
        Some(&obj(vec![("id", s("f2")), ("year", Value::Number(1.0))])),
        &obj(vec![("year", Value::Number(2.0))]),
        Some(&sc),
    );
    if errs.len() != 1 || errs[0].message != MESSAGE_REQUIRED_FIELD_MISSING {
        panic!("expected a missing required field to be reported, got {errs:?}");
    }

    let legacy = obj(vec![
        ("id", s("f3")),
        ("title", s("C")),
        ("legacy", s("x")),
        ("year", s("bad")),
    ]);
    let errs = schema::validate_entity_update(
        b"film",
        Some(&legacy),
        &obj(vec![("title", s("D"))]),
        Some(&sc),
    );
    if !errs.is_empty() {
        panic!("expected stored fields the update does not touch to be ignored, got {errs:?}");
    }

    let nested = manual_schema(obj(vec![
        ("title", typed("string", Some(true))),
        (
            "author",
            object(obj(vec![
                ("type", s("object")),
                (
                    "fields",
                    object(obj(vec![("name", typed("string", Some(true)))])),
                ),
            ])),
        ),
        (
            "tags",
            object(obj(vec![
                ("type", s("array")),
                (
                    "fields",
                    object(obj(vec![("label", typed("string", None))])),
                ),
            ])),
        ),
    ]));
    let stored = obj(vec![
        ("id", s("f4")),
        ("title", s("C")),
        (
            "tags",
            Value::Array(vec![object(obj(vec![("label", Value::Number(1.0))]))]),
        ),
    ]);

    let errs = schema::validate_entity_update(
        b"film",
        Some(&stored),
        &obj(vec![(
            "author",
            object(obj(vec![("name", Value::Number(5.0))])),
        )]),
        Some(&nested),
    );
    if errs.len() != 1 || errs[0].field != "author.name" {
        panic!("expected the nested error of an updated field, got {errs:?}");
    }

    let errs = schema::validate_entity_update(
        b"film",
        Some(&stored),
        &obj(vec![("title", s("D"))]),
        Some(&nested),
    );
    if !errs.is_empty() {
        panic!("expected nested errors of untouched fields to be ignored, got {errs:?}");
    }

    let errs = schema::validate_entity_update(
        b"film",
        None,
        &obj(vec![("year", Value::Number(1.0))]),
        Some(&sc),
    );
    if errs.len() != 1 || errs[0].field != "title" {
        panic!("expected the body alone to be validated without a stored document, got {errs:?}");
    }
}

#[test]
fn map_to_fields_shorthand_types() {
    let fields = schema::map_to_fields(&obj(vec![
        ("title", s("string")),
        (
            "pages",
            object(obj(vec![
                ("type", s("number")),
                ("required", Value::Bool(true)),
            ])),
        ),
    ]));

    let title = &fields["title"];
    if title.ty != "string" || title.name != "title" || title.required {
        panic!(
            "expected the shorthand form to define a string field, got {} {} {}",
            title.name, title.ty, title.required
        );
    }

    let pages = &fields["pages"];
    if pages.ty != "number" || !pages.required {
        panic!(
            "expected the full form to keep working, got {} {} {}",
            pages.name, pages.ty, pages.required
        );
    }
}
