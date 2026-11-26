use std::collections::{BTreeMap, btree_map};
use std::sync::Arc;

use crate::globals::{self, CORE_FIELDS_PREFIX};
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::{Map, Value};
use crate::gocompat::utf8;
use crate::json_struct;
use crate::storage;
use crate::sync::RwLock;

pub const SCHEMA_ENTITY: &str = "_elysiandb_core_schema";

pub const MESSAGE_REQUIRED_FIELD_MISSING: &str = "required field missing";
pub const MESSAGE_REQUIRED_FIELD_NULL: &str = "required field cannot be null";

const MESSAGE_FIELD_NOT_ALLOWED: &str = "field not allowed by strict schema";
const MESSAGE_EXPECTED_OBJECT: &str = "expected object";
const MESSAGE_EXPECTED_ARRAY: &str = "expected array";

pub const INCLUDED_DOCUMENTS_TYPE: &str = "[]map[string]interface {}";
pub const INCLUDED_DOCUMENTS_DISPLAY: &str = "array of included documents";

pub type Fields = BTreeMap<String, Field>;

#[derive(Default)]
pub struct Field {
    pub name: String,
    pub ty: String,
    pub required: bool,
    pub fields: Fields,
}

impl Field {
    pub fn new(name: impl Into<String>, ty: impl Into<String>, required: bool) -> Field {
        Field {
            name: name.into(),
            ty: ty.into(),
            required,
            fields: Fields::new(),
        }
    }

    pub fn with_fields(mut self, fields: Fields) -> Field {
        self.fields = fields;
        self
    }

    fn shallow(&self) -> Field {
        Field::new(self.name.clone(), self.ty.clone(), self.required)
    }
}

impl Drop for Field {
    fn drop(&mut self) {
        if self.fields.is_empty() {
            return;
        }
        let mut stack: Vec<Field> = std::mem::take(&mut self.fields).into_values().collect();
        while let Some(mut f) = stack.pop() {
            stack.extend(std::mem::take(&mut f.fields).into_values());
        }
    }
}

struct CloneFrame<'a> {
    iter: btree_map::Iter<'a, String, Field>,
    out: Fields,
    pending: Option<(String, Field)>,
}

fn clone_fields(src: &Fields) -> Fields {
    let mut stack = vec![CloneFrame {
        iter: src.iter(),
        out: Fields::new(),
        pending: None,
    }];
    loop {
        let Some(top) = stack.last_mut() else {
            return Fields::new();
        };
        match top.iter.next() {
            Some((k, f)) => {
                if f.fields.is_empty() {
                    top.out.insert(k.clone(), f.shallow());
                } else {
                    let pending = Some((k.clone(), f.shallow()));
                    stack.push(CloneFrame {
                        iter: f.fields.iter(),
                        out: Fields::new(),
                        pending,
                    });
                }
            }
            None => {
                let Some(done) = stack.pop() else {
                    return Fields::new();
                };
                let Some((k, mut f)) = done.pending else {
                    return done.out;
                };
                f.fields = done.out;
                if let Some(parent) = stack.last_mut() {
                    parent.out.insert(k, f);
                }
            }
        }
    }
}

impl Clone for Field {
    fn clone(&self) -> Field {
        self.shallow().with_fields(clone_fields(&self.fields))
    }
}

#[derive(Clone, Default)]
pub struct Entity {
    pub id: Vec<u8>,
    pub fields: Fields,
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ValidationError {
        pub field: String => "Field",
        pub message: String => "Message",
    }
}

impl ValidationError {
    pub fn new(field: impl Into<String>, message: impl Into<String>) -> ValidationError {
        ValidationError {
            field: field.into(),
            message: message.into(),
        }
    }

    pub fn to_error(&self) -> BoxError {
        errors::new(format!("Field '{}': {}", self.field, self.message))
    }
}

pub type LoadSchemaForEntity =
    Arc<dyn Fn(&[u8], Option<&Map>) -> Option<Arc<Entity>> + Send + Sync>;

static LOAD_SCHEMA_FOR_ENTITY: RwLock<Option<LoadSchemaForEntity>> = RwLock::new(None);

pub fn set_load_schema_for_entity(f: Option<LoadSchemaForEntity>) -> Option<LoadSchemaForEntity> {
    std::mem::replace(&mut *LOAD_SCHEMA_FOR_ENTITY.write(), f)
}

pub fn load_schema_for_entity(entity: &[u8], schema_data: Option<&Map>) -> Option<Arc<Entity>> {
    let hook = LOAD_SCHEMA_FOR_ENTITY.read().clone();
    match hook {
        Some(f) => f(entity, schema_data),
        None => default_load_schema_for_entity(entity, schema_data),
    }
}

fn strict_setting() -> bool {
    match globals::config() {
        Some(cfg) => cfg.api.schema.strict,
        None => panic!("the configuration is not set"),
    }
}

fn text(b: &[u8]) -> String {
    if let Ok(s) = std::str::from_utf8(b) {
        return s.to_string();
    }
    let mut out = String::with_capacity(b.len() + 8);
    let mut i = 0;
    while i < b.len() {
        let (r, n) = utf8::decode_rune(&b[i..]);
        out.push(char::from_u32(r as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
        i += n.max(1);
    }
    out
}

fn schema_key(entity: &[u8]) -> Vec<u8> {
    let mut key = globals::api_single_entity_key(SCHEMA_ENTITY, "").into_bytes();
    key.extend_from_slice(entity);
    key
}

pub fn analyze_entity_schema(entity: &[u8], data: &Map) -> Map {
    let required = strict_setting();
    let schema = Entity {
        id: entity.to_vec(),
        fields: analyze_fields(data, required),
    };

    schema_entity_to_storable_structure(&schema)
}

pub fn merge_analyzed_schema(entity: &[u8], existing: Option<&Map>, data: &Map) -> Map {
    let required = strict_setting();
    let analyzed = analyze_fields(data, required);
    let Some(existing) = existing else {
        return schema_entity_to_storable_structure(&Entity {
            id: entity.to_vec(),
            fields: analyzed,
        });
    };

    let existing_fields = match existing.get("fields") {
        Some(Value::Object(m)) => map_to_fields(m),
        _ => Fields::new(),
    };

    let mut merged = schema_entity_to_storable_structure(&Entity {
        id: entity.to_vec(),
        fields: merge_fields(existing_fields, analyzed),
    });

    if let Some(manual) = existing.get("_manual") {
        merged.insert("_manual".to_string(), manual.clone());
    }

    merged
}

struct MergeFrame {
    merged: Fields,
    incoming: btree_map::IntoIter<String, Field>,
    pending: Option<(String, Field)>,
}

fn merge_fields(existing: Fields, incoming: Fields) -> Fields {
    let mut stack = vec![MergeFrame {
        merged: existing,
        incoming: incoming.into_iter(),
        pending: None,
    }];
    loop {
        let Some(top) = stack.last_mut() else {
            return Fields::new();
        };
        match top.incoming.next() {
            Some((name, mut field)) => match top.merged.remove(&name) {
                None => {
                    top.merged.insert(name, field);
                }
                Some(current) if field.ty == "unknown" => {
                    top.merged.insert(name, current);
                }
                Some(mut current)
                    if current.ty == field.ty && (field.ty == "object" || field.ty == "array") =>
                {
                    let merged = std::mem::take(&mut current.fields);
                    let incoming = std::mem::take(&mut field.fields).into_iter();
                    stack.push(MergeFrame {
                        merged,
                        incoming,
                        pending: Some((name, current)),
                    });
                }
                Some(current) => {
                    field.required = current.required;
                    top.merged.insert(name, field);
                }
            },
            None => {
                let Some(done) = stack.pop() else {
                    return Fields::new();
                };
                let Some((name, mut current)) = done.pending else {
                    return done.merged;
                };
                current.fields = done.merged;
                if let Some(parent) = stack.last_mut() {
                    parent.merged.insert(name, current);
                }
            }
        }
    }
}

struct AnalyzeFrame<'a> {
    iter: btree_map::Iter<'a, String, Value>,
    out: Fields,
    pending: Option<Field>,
    root: bool,
}

fn analyze_fields(data: &Map, required: bool) -> Fields {
    let mut stack = vec![AnalyzeFrame {
        iter: data.iter(),
        out: Fields::new(),
        pending: None,
        root: true,
    }];
    loop {
        let Some(top) = stack.last_mut() else {
            return Fields::new();
        };
        match top.iter.next() {
            Some((k, v)) => {
                if top.root && k == "id" || k.starts_with(CORE_FIELDS_PREFIX) {
                    continue;
                }

                let f = Field::new(k.clone(), detect_json_type(v), required);
                let sub = match v {
                    Value::Object(m) => Some(m),
                    Value::Array(arr) => match arr.first() {
                        Some(Value::Object(m)) => Some(m),
                        _ => None,
                    },
                    _ => None,
                };

                match sub {
                    Some(m) => stack.push(AnalyzeFrame {
                        iter: m.iter(),
                        out: Fields::new(),
                        pending: Some(f),
                        root: false,
                    }),
                    None => {
                        top.out.insert(k.clone(), f);
                    }
                }
            }
            None => {
                let Some(done) = stack.pop() else {
                    return Fields::new();
                };
                let Some(mut f) = done.pending else {
                    return done.out;
                };
                f.fields = done.out;
                if let Some(parent) = stack.last_mut() {
                    parent.out.insert(f.name.clone(), f);
                }
            }
        }
    }
}

pub fn detect_json_type(v: &Value) -> &'static str {
    match v {
        Value::String(_) => "string",
        Value::Number(_) | Value::Int(_) => "number",
        Value::Bool(_) => "boolean",
        Value::Object(_) => "object",
        Value::Array(_) => "array",
        Value::DocList(_) => INCLUDED_DOCUMENTS_TYPE,
        Value::Null => "unknown",
    }
}

fn display_type(t: &str) -> &str {
    if t == INCLUDED_DOCUMENTS_TYPE {
        return INCLUDED_DOCUMENTS_DISPLAY;
    }
    t
}

pub fn validate_entity(
    entity: &[u8],
    data: &Map,
    schema_data: Option<&Map>,
) -> Vec<ValidationError> {
    let mut errors = Vec::new();
    let Some(s) = load_schema_for_entity(entity, schema_data) else {
        return errors;
    };

    let is_strict = strict_setting() && is_manual_schema(entity, schema_data);

    if is_strict {
        validate_no_extra_fields(&s.fields, data, &mut errors);
    }

    validate_fields(&s.fields, data, &mut errors, is_strict);

    errors
}

pub fn validate_entity_update(
    entity: &[u8],
    existing: Option<&Map>,
    updates: &Map,
    schema_data: Option<&Map>,
) -> Vec<ValidationError> {
    let Some(existing) = existing else {
        return validate_entity(entity, updates, schema_data);
    };

    let mut merged = existing.clone();
    for (k, v) in updates {
        merged.insert(k.clone(), v.clone());
    }

    let mut errors = Vec::new();
    for e in validate_entity(entity, &merged, schema_data) {
        let root = root_field_name(&e.field);
        let updated = updates.contains_key(root);
        let missing_at_root = root == e.field && e.message == MESSAGE_REQUIRED_FIELD_MISSING;

        if updated || missing_at_root {
            errors.push(e);
        }
    }

    errors
}

fn root_field_name(field: &str) -> &str {
    match field.find(['.', '[']) {
        Some(i) => &field[..i],
        None => field,
    }
}

fn enter_key(path: &mut String, prefix_len: usize, key: &str) {
    path.truncate(prefix_len);
    if prefix_len > 0 {
        path.push('.');
    }
    path.push_str(key);
}

fn enter_item(path: &mut String, full_len: usize, i: usize) {
    path.truncate(full_len);
    path.push('[');
    path.push_str(&i.to_string());
    path.push(']');
}

enum ExtraFrame<'a> {
    Keys {
        iter: btree_map::Iter<'a, String, Value>,
        fields: &'a Fields,
        prefix_len: usize,
    },
    Items {
        iter: std::iter::Enumerate<std::slice::Iter<'a, Value>>,
        fields: &'a Fields,
        full_len: usize,
    },
}

fn validate_no_extra_fields(fields: &Fields, data: &Map, errors: &mut Vec<ValidationError>) {
    let mut path = String::new();
    let mut stack = vec![ExtraFrame::Keys {
        iter: data.iter(),
        fields,
        prefix_len: 0,
    }];
    while let Some(top) = stack.last_mut() {
        let next = match top {
            ExtraFrame::Keys {
                iter,
                fields,
                prefix_len,
            } => {
                let (fields, prefix_len) = (*fields, *prefix_len);
                let Some((key, val)) = iter.next() else {
                    stack.pop();
                    continue;
                };
                if key == "id" || key.starts_with(CORE_FIELDS_PREFIX) {
                    continue;
                }

                enter_key(&mut path, prefix_len, key);
                let Some(field_def) = fields.get(key) else {
                    errors.push(ValidationError::new(
                        path.clone(),
                        MESSAGE_FIELD_NOT_ALLOWED,
                    ));
                    continue;
                };

                match val {
                    Value::Object(sub) if field_def.ty == "object" => ExtraFrame::Keys {
                        iter: sub.iter(),
                        fields: &field_def.fields,
                        prefix_len: path.len(),
                    },
                    Value::Array(arr)
                        if field_def.ty == "array" && !field_def.fields.is_empty() =>
                    {
                        ExtraFrame::Items {
                            iter: arr.iter().enumerate(),
                            fields: &field_def.fields,
                            full_len: path.len(),
                        }
                    }
                    _ => continue,
                }
            }
            ExtraFrame::Items {
                iter,
                fields,
                full_len,
            } => {
                let (fields, full_len) = (*fields, *full_len);
                let Some((i, item)) = iter.next() else {
                    stack.pop();
                    continue;
                };
                let Value::Object(obj) = item else {
                    continue;
                };
                enter_item(&mut path, full_len, i);
                ExtraFrame::Keys {
                    iter: obj.iter(),
                    fields,
                    prefix_len: path.len(),
                }
            }
        };
        stack.push(next);
    }
}

enum FieldFrame<'a> {
    Fields {
        iter: btree_map::Iter<'a, String, Field>,
        data: &'a Map,
        prefix_len: usize,
    },
    Items {
        iter: std::iter::Enumerate<std::slice::Iter<'a, Value>>,
        fields: &'a Fields,
        full_len: usize,
    },
}

fn validate_fields(fields: &Fields, data: &Map, errors: &mut Vec<ValidationError>, strict: bool) {
    let mut path = String::new();
    let mut stack = vec![FieldFrame::Fields {
        iter: fields.iter(),
        data,
        prefix_len: 0,
    }];
    while let Some(top) = stack.last_mut() {
        let next = match top {
            FieldFrame::Fields {
                iter,
                data,
                prefix_len,
            } => {
                let (data, prefix_len) = (*data, *prefix_len);
                let Some((field_name, field_def)) = iter.next() else {
                    stack.pop();
                    continue;
                };
                if field_name == "id" {
                    continue;
                }

                enter_key(&mut path, prefix_len, field_name);

                let Some(value) = data.get(field_name) else {
                    if strict && field_def.required {
                        errors.push(ValidationError::new(
                            path.clone(),
                            MESSAGE_REQUIRED_FIELD_MISSING,
                        ));
                    }
                    continue;
                };

                if value.is_null() {
                    if strict && field_def.required {
                        errors.push(ValidationError::new(
                            path.clone(),
                            MESSAGE_REQUIRED_FIELD_NULL,
                        ));
                    }
                    continue;
                }

                let expected = field_def.ty.as_str();
                if expected == "unknown" {
                    continue;
                }

                let actual = detect_json_type(value);
                if actual != expected {
                    errors.push(ValidationError::new(
                        path.clone(),
                        format!(
                            "expected type {} but got {}",
                            display_type(expected),
                            display_type(actual)
                        ),
                    ));
                    continue;
                }

                if expected == "object" {
                    let Value::Object(sub) = value else {
                        errors.push(ValidationError::new(path.clone(), MESSAGE_EXPECTED_OBJECT));
                        continue;
                    };
                    FieldFrame::Fields {
                        iter: field_def.fields.iter(),
                        data: sub,
                        prefix_len: path.len(),
                    }
                } else if expected == "array" {
                    let Value::Array(arr) = value else {
                        errors.push(ValidationError::new(path.clone(), MESSAGE_EXPECTED_ARRAY));
                        continue;
                    };
                    if field_def.fields.is_empty() {
                        continue;
                    }
                    FieldFrame::Items {
                        iter: arr.iter().enumerate(),
                        fields: &field_def.fields,
                        full_len: path.len(),
                    }
                } else {
                    continue;
                }
            }
            FieldFrame::Items {
                iter,
                fields,
                full_len,
            } => {
                let (fields, full_len) = (*fields, *full_len);
                let Some((i, item)) = iter.next() else {
                    stack.pop();
                    continue;
                };
                let Value::Object(obj) = item else {
                    continue;
                };
                enter_item(&mut path, full_len, i);
                FieldFrame::Fields {
                    iter: fields.iter(),
                    data: obj,
                    prefix_len: path.len(),
                }
            }
        };
        stack.push(next);
    }
}

pub fn is_manual_schema(entity: &[u8], schema_data: Option<&Map>) -> bool {
    match schema_data {
        Some(data) => data.contains_key("_manual"),
        None => {
            if let Some(p) = stored_schema(entity) {
                return p.manual;
            }

            match storage::get_json_by_key(&schema_key(entity)) {
                Ok(data) => data.contains_key("_manual"),
                Err(_) => false,
            }
        }
    }
}

pub fn schema_entity_to_storable_structure(entity: &Entity) -> Map {
    Map::from([
        ("id".to_string(), Value::String(text(&entity.id))),
        (
            "fields".to_string(),
            Value::Object(fields_to_map(&entity.fields)),
        ),
    ])
}

fn field_map(v: &Field) -> Map {
    Map::from([
        ("name".to_string(), Value::String(v.name.clone())),
        ("type".to_string(), Value::String(v.ty.clone())),
        ("required".to_string(), Value::Bool(v.required)),
    ])
}

struct ToMapFrame<'a> {
    iter: btree_map::Iter<'a, String, Field>,
    out: Map,
    pending: Option<(String, Map)>,
}

pub fn fields_to_map(fields: &Fields) -> Map {
    let mut stack = vec![ToMapFrame {
        iter: fields.iter(),
        out: Map::new(),
        pending: None,
    }];
    loop {
        let Some(top) = stack.last_mut() else {
            return Map::new();
        };
        match top.iter.next() {
            Some((_, v)) => {
                let m = field_map(v);
                if v.fields.is_empty() {
                    top.out.insert(v.name.clone(), Value::Object(m));
                } else {
                    let pending = Some((v.name.clone(), m));
                    stack.push(ToMapFrame {
                        iter: v.fields.iter(),
                        out: Map::new(),
                        pending,
                    });
                }
            }
            None => {
                let Some(done) = stack.pop() else {
                    return Map::new();
                };
                let Some((name, mut m)) = done.pending else {
                    return done.out;
                };
                m.insert("fields".to_string(), Value::Object(done.out));
                if let Some(parent) = stack.last_mut() {
                    parent.out.insert(name, Value::Object(m));
                }
            }
        }
    }
}

struct ToFieldsFrame<'a> {
    iter: btree_map::Iter<'a, String, Value>,
    out: Fields,
    pending: Option<(String, Field)>,
}

pub fn map_to_fields(m: &Map) -> Fields {
    let mut stack = vec![ToFieldsFrame {
        iter: m.iter(),
        out: Fields::new(),
        pending: None,
    }];
    loop {
        let Some(top) = stack.last_mut() else {
            return Fields::new();
        };
        match top.iter.next() {
            Some((k, v)) => match v {
                Value::String(type_name) => {
                    top.out
                        .insert(k.clone(), Field::new(k.clone(), type_name.clone(), false));
                }
                Value::Object(field_map) => {
                    let mut f = Field::new(k.clone(), String::new(), false);
                    if let Some(Value::String(type_name)) = field_map.get("type") {
                        f.ty = type_name.clone();
                    }

                    if let Some(Value::Bool(req)) = field_map.get("required") {
                        f.required = *req;
                    }

                    let key = match field_map.get("name") {
                        Some(Value::String(name)) => {
                            f.name = name.clone();
                            name.clone()
                        }
                        _ => k.clone(),
                    };

                    match field_map.get("fields") {
                        Some(Value::Object(sub_fields)) => stack.push(ToFieldsFrame {
                            iter: sub_fields.iter(),
                            out: Fields::new(),
                            pending: Some((key, f)),
                        }),
                        _ => {
                            top.out.insert(key, f);
                        }
                    }
                }
                _ => {}
            },
            None => {
                let Some(done) = stack.pop() else {
                    return Fields::new();
                };
                let Some((key, mut f)) = done.pending else {
                    return done.out;
                };
                f.fields = done.out;
                if let Some(parent) = stack.last_mut() {
                    parent.out.insert(key, f);
                }
            }
        }
    }
}

struct ParsedSchema {
    version: String,
    entity: Arc<Entity>,
    manual: bool,
}

static PARSED_SCHEMAS: RwLock<BTreeMap<Vec<u8>, Arc<ParsedSchema>>> = RwLock::new(BTreeMap::new());

fn stored_schema(entity: &[u8]) -> Option<Arc<ParsedSchema>> {
    let key = schema_key(entity);
    let version = storage::document_version(&key)?;

    if let Some(p) = PARSED_SCHEMAS.read().get(entity)
        && p.version == version
    {
        return Some(p.clone());
    }

    let raw = storage::get_json_by_key(&key).ok()?;
    let p = Arc::new(ParsedSchema {
        version,
        entity: Arc::new(parse_schema(entity, &raw)),
        manual: raw.contains_key("_manual"),
    });
    let previous = PARSED_SCHEMAS.write().insert(entity.to_vec(), p.clone());
    drop(previous);

    Some(p)
}

fn default_load_schema_for_entity(entity: &[u8], schema_data: Option<&Map>) -> Option<Arc<Entity>> {
    if let Some(data) = schema_data {
        return Some(Arc::new(parse_schema(entity, data)));
    }

    if let Some(p) = stored_schema(entity) {
        return Some(p.entity.clone());
    }

    let raw = storage::get_json_by_key(&schema_key(entity)).ok()?;

    Some(Arc::new(parse_schema(entity, &raw)))
}

fn parse_schema(entity: &[u8], schema_data: &Map) -> Entity {
    let fields = match schema_data.get("fields") {
        Some(Value::Object(fields)) => map_to_fields(fields),
        _ => Fields::new(),
    };

    Entity {
        id: entity.to_vec(),
        fields,
    }
}
