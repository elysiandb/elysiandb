use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::configuration::Config;
use crate::datastore::{Engine, Tx, decode_map};
use crate::globals;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::{self, Encode, EncodeState, Map, Value};
use crate::gocompat::utf8;
use crate::json_struct;
use crate::kv;
use crate::log::{self, Logger};
use crate::query::Filters;
use crate::schema::{self, ValidationError};
use crate::storage::{self, deep_copy_document};
use crate::uuid;

use super::filter::filters_match_entity;
use super::guard::{self, Guard, GuardError};
use super::include::{apply_includes_with, extract_auto_includes, merge_includes};
use super::plan::{ListQuery, plan};
use super::relationship::{extract_sub_entities, merge_with_existing};

pub const CORE_ENTITY_TYPE_PREFIX: &str = "_elysiandb_core_";

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ByteFields(BTreeMap<String, Vec<u8>>);

impl ByteFields {
    pub fn new() -> ByteFields {
        ByteFields::default()
    }

    pub fn id(id: &[u8]) -> ByteFields {
        ByteFields::new().with("id", id)
    }

    pub fn with(mut self, key: &str, value: &[u8]) -> ByteFields {
        self.0.insert(key.to_string(), value.to_vec());
        self
    }

    pub fn get(&self, key: &str) -> Option<&[u8]> {
        self.0.get(key).map(Vec::as_slice)
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn holds_invalid_utf8(&self) -> bool {
        self.0.values().any(|v| std::str::from_utf8(v).is_err())
    }

    pub fn apply(&self, data: &mut Map) {
        for (key, value) in &self.0 {
            data.insert(key.clone(), Value::String(utf8::decode_string(value)));
        }
    }

    fn value_in<'a>(&'a self, data: &Map, key: &str) -> Option<&'a [u8]> {
        let raw = self.get(key)?;
        match data.get(key) {
            Some(Value::String(text)) if *text == utf8::decode_string(raw) => Some(raw),
            _ => None,
        }
    }
}

struct Document<'a> {
    data: &'a Map,
    fields: &'a ByteFields,
}

impl Encode for Document<'_> {
    fn encode(&self, e: &mut EncodeState) -> Result<(), json::Error> {
        e.write_byte(b'{');
        for (i, (k, v)) in self.data.iter().enumerate() {
            if i > 0 {
                e.write_byte(b',');
            }
            e.write_string(k.as_bytes());
            e.write_byte(b':');
            match self.fields.value_in(self.data, k) {
                Some(raw) => e.write_string(raw),
                None => v.encode(e)?,
            }
        }
        e.write_byte(b'}');
        Ok(())
    }
}

pub fn encode_document(data: &Map, fields: &ByteFields) -> Result<Vec<u8>, json::Error> {
    if !fields.holds_invalid_utf8() {
        return json::marshal(data);
    }

    json::marshal(&Document { data, fields })
}

pub(crate) const API_LOG: Logger = log::for_component("api");

#[derive(Debug)]
struct NotOpen;

impl fmt::Display for NotOpen {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("storage is not open")
    }
}

impl std::error::Error for NotOpen {}

pub(crate) fn db() -> Option<Arc<Engine>> {
    storage::engine()
}

pub(crate) fn update<F>(f: F) -> Result<(), BoxError>
where
    F: FnMut(&mut Tx<'_>) -> Result<(), BoxError>,
{
    let Some(e) = db() else {
        return Err(Box::new(NotOpen));
    };

    e.update(f)
}

pub(crate) fn log_write_error(op: &str, entity: &[u8], err: Result<(), BoxError>) {
    if let Err(err) = err
        && err.downcast_ref::<ValidationFailed>().is_none()
    {
        API_LOG.error(
            "write failed",
            kv!["op" => op, "entity" => entity, "err" => log::display(&err)],
        );
    }
}

pub fn add_entity_type(entity: &[u8]) {
    if entity_type_exists(entity) {
        return;
    }

    log_write_error(
        "add_type",
        entity,
        update(|tx| {
            tx.ensure_type(entity);
            Ok(())
        }),
    );
}

pub fn entity_type_exists(entity: &[u8]) -> bool {
    db().is_some_and(|e| e.type_exists(entity))
}

pub fn list_entity_types() -> Vec<Vec<u8>> {
    db().map(|e| e.types()).unwrap_or_default()
}

pub fn is_core_entity_type(entity: &[u8]) -> bool {
    entity.starts_with(CORE_ENTITY_TYPE_PREFIX.as_bytes())
}

pub fn list_public_entity_types() -> Vec<Vec<u8>> {
    list_entity_types()
        .into_iter()
        .filter(|t| !is_core_entity_type(t))
        .collect()
}

pub fn read_entity_by_id(entity: &[u8], id: &[u8]) -> Option<Map> {
    let e = db()?;
    let doc = e.snapshot(entity).get(id)?;
    doc.decode().ok()
}

pub fn read_entity_bytes_by_id(entity: &[u8], id: &[u8]) -> Option<Arc<[u8]>> {
    let e = db()?;
    let doc = e.snapshot(entity).get(id)?;
    doc.bytes().ok()
}

#[allow(clippy::too_many_arguments)]
pub fn list_entities(
    entity: &[u8],
    limit: i64,
    offset: i64,
    sort_field: &[u8],
    sort_ascending: bool,
    filters: Option<&Filters>,
    search: &[u8],
    includes_param: &[u8],
) -> Vec<Map> {
    list_entities_guarded(
        entity,
        limit,
        offset,
        sort_field,
        sort_ascending,
        filters,
        search,
        includes_param,
        None,
    )
    .0
}

#[allow(clippy::too_many_arguments)]
pub fn list_entities_guarded(
    entity: &[u8],
    limit: i64,
    offset: i64,
    sort_field: &[u8],
    sort_ascending: bool,
    filters: Option<&Filters>,
    search: &[u8],
    includes_param: &[u8],
    guard: Option<&Guard>,
) -> (Vec<Map>, Option<GuardError>) {
    let auto = filters.map(extract_auto_includes).unwrap_or_default();
    let includes = merge_includes(includes_param, &auto);
    let res = plan(ListQuery {
        entity,
        limit,
        offset,
        sort_field,
        sort_asc: sort_ascending,
        filters,
        search,
        includes: &includes,
        guard,
        ..ListQuery::default()
    });

    if let Some(err) = res.err {
        return (Vec::new(), Some(err));
    }

    let mut out = res.decode();
    if !includes.is_empty() && !out.is_empty() {
        let mut reader = res.reader();
        apply_includes_with(&mut out, &includes, &mut reader);
    }

    (out, guard::err(guard))
}

pub fn apply_filters_to_list(entities: Vec<Map>, filters: Option<&Filters>) -> Vec<Map> {
    let Some(filters) = filters.filter(|f| !f.is_empty()) else {
        return entities;
    };

    entities
        .into_iter()
        .filter(|e| filters_match_entity(e, filters))
        .collect()
}

pub fn paginate<T: Clone>(entities: &[T], offset: i64, limit: i64) -> Vec<T> {
    apply_offset_limit(entities, offset, limit).to_vec()
}

pub(crate) fn apply_offset_limit<T>(input: &[T], offset: i64, limit: i64) -> &[T] {
    let len = input.len() as i64;
    let start = offset.clamp(0, len);

    let mut end = len;
    if limit > 0 && limit < end - start {
        end = start + limit;
    }

    &input[start as usize..end as usize]
}

pub fn get_list_of_ids(
    entity: &[u8],
    sort_field: &[u8],
    sort_ascending: bool,
) -> Result<Vec<u8>, BoxError> {
    let res = plan(ListQuery {
        entity,
        sort_field,
        sort_asc: sort_ascending,
        ..ListQuery::default()
    });
    let ids = res.ids();
    if ids.is_empty() {
        if sort_field.is_empty() {
            return Err(errors::new(format!(
                "key not found: {}",
                globals::api_entity_index_id_key(&text_of(entity))
            )));
        }

        return Ok(Vec::new());
    }

    Ok(ids.join(b"\n".as_slice()))
}

pub fn delete_entity_by_id(entity: &[u8], id: &[u8]) {
    log_write_error(
        "delete",
        entity,
        update(|tx| {
            tx.delete(entity, id);
            Ok(())
        }),
    );
}

pub fn entity_exists(entity: &[u8], id: &[u8]) -> bool {
    db().is_some_and(|e| e.snapshot(entity).get(id).is_some())
}

pub fn count_entities(entity: &[u8]) -> usize {
    db().map_or(0, |e| e.snapshot(entity).len())
}

pub fn count_entities_per_type() -> BTreeMap<Vec<u8>, usize> {
    let mut counts = BTreeMap::new();
    for entity in list_public_entity_types() {
        let n = count_entities(&entity);
        counts.insert(entity, n);
    }

    counts
}

pub fn delete_entity_documents(entity: &[u8]) {
    log_write_error(
        "delete_documents",
        entity,
        update(|tx| {
            tx.reset_entity(entity);
            Ok(())
        }),
    );
}

#[derive(Debug)]
struct ValidationFailed;

impl fmt::Display for ValidationFailed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("validation failed")
    }
}

impl std::error::Error for ValidationFailed {}

pub(crate) fn current_config() -> Arc<Config> {
    match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    }
}

pub(crate) fn schema_enabled() -> bool {
    current_config().api.schema.enabled
}

pub(crate) fn schema_strict() -> bool {
    current_config().api.schema.strict
}

fn schema_entity() -> &'static [u8] {
    schema::SCHEMA_ENTITY.as_bytes()
}

fn text_of(b: &[u8]) -> String {
    utf8::decode_string(b)
}

fn string_field<'a>(data: &'a Map, key: &str) -> &'a [u8] {
    match data.get(key) {
        Some(Value::String(s)) => s.as_bytes(),
        _ => b"",
    }
}

fn replace_contents(dst: &mut Map, src: Map) {
    *dst = src;
}

pub fn write_entity(entity: &[u8], data: &mut Map) -> Vec<ValidationError> {
    write_entity_bytes(entity, data, &ByteFields::new())
}

pub fn write_entity_bytes(
    entity: &[u8],
    data: &mut Map,
    fields: &ByteFields,
) -> Vec<ValidationError> {
    let mut errs: Vec<ValidationError> = Vec::new();
    let mut result: Option<Map> = None;

    fields.apply(data);
    let source: &Map = data;
    let err = update(|tx| {
        let mut attempt = deep_copy_document(source);
        errs = write_entity_tx(tx, entity, &mut attempt, true, fields);
        if !errs.is_empty() {
            return Err(Box::new(ValidationFailed));
        }

        result = Some(attempt);

        Ok(())
    });

    if let Err(e) = &err
        && e.downcast_ref::<ValidationFailed>().is_some()
    {
        return errs;
    }

    log_write_error("write", entity, err);
    if let Some(result) = result {
        replace_contents(data, result);
    }

    Vec::new()
}

fn validate_write(
    tx: &mut Tx<'_>,
    entity: &[u8],
    data: &Map,
    check_inferred_schema: bool,
) -> Vec<ValidationError> {
    if schema_enabled() && entity != schema_entity() {
        let current = tx.read_shared(schema_entity(), entity);
        if let Some(current) = &current
            && (check_inferred_schema || schema::is_manual_schema(entity, Some(current)))
        {
            return schema::validate_entity(entity, data, Some(current));
        }
    }

    Vec::new()
}

fn write_entity_tx(
    tx: &mut Tx<'_>,
    entity: &[u8],
    data: &mut Map,
    check_inferred_schema: bool,
    fields: &ByteFields,
) -> Vec<ValidationError> {
    let errors = validate_write(tx, entity, data, check_inferred_schema);
    if !errors.is_empty() {
        return errors;
    }

    let has_id = matches!(data.get("id"), Some(Value::String(s)) if !s.is_empty());
    if !has_id {
        data.insert("id".to_string(), Value::String(uuid::new_v4()));
    }

    write_sub_entities(tx, entity, data);

    persist_entity(tx, entity, data, fields);
    update_schema_if_needed(tx, entity, data);

    Vec::new()
}

fn write_sub_entities(tx: &mut Tx<'_>, entity: &[u8], data: &mut Map) {
    let subs = extract_sub_entities(entity, data);
    for mut sub in subs {
        let sub_entity = match sub.remove("@entity").as_ref() {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let id = string_field(&sub, "id").to_vec();
        let existing = tx.read(sub_entity.as_bytes(), &id);
        let mut merged = merge_with_existing(sub, existing);
        write_entity_tx(
            tx,
            sub_entity.as_bytes(),
            &mut merged,
            false,
            &ByteFields::new(),
        );
    }
}

pub fn update_entity_schema(entity: &[u8], fields_raw: &Map) -> Map {
    let fields = schema::map_to_fields(fields_raw);

    let entity_schema = schema::Entity {
        id: entity.to_vec(),
        fields,
    };

    let mut storable = schema::schema_entity_to_storable_structure(&entity_schema);
    storable.insert("_manual".to_string(), Value::Bool(true));

    write_entity_bytes(schema_entity(), &mut storable, &ByteFields::id(entity));

    storable
}

fn entity_type_message(entity: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut msg = b"entity type '".to_vec();
    msg.extend_from_slice(entity);
    msg.extend_from_slice(suffix);
    msg
}

pub fn create_entity_type(entity: &[u8]) -> Result<(), BoxError> {
    if entity_type_exists(entity) {
        return Err(errors::new_bytes(entity_type_message(
            entity,
            b"' already exists",
        )));
    }

    add_entity_type(entity);

    Ok(())
}

pub fn delete_entity_type(entity: &[u8]) -> Result<(), BoxError> {
    if !entity_type_exists(entity) {
        return Err(errors::new_bytes(entity_type_message(
            entity,
            b"' does not exist",
        )));
    }

    delete_all_entities(entity);

    Ok(())
}

fn put_document(
    tx: &mut Tx<'_>,
    entity: &[u8],
    id: &[u8],
    data: &Map,
    fields: &ByteFields,
) -> Result<(), BoxError> {
    if !fields.holds_invalid_utf8() {
        return tx.put(entity, id, data);
    }

    let value = encode_document(data, fields)?;
    let decoded = decode_map(&value).ok().flatten();
    tx.put_bytes(entity, id, value, decoded)
}

fn persist_entity(tx: &mut Tx<'_>, entity: &[u8], data: &Map, fields: &ByteFields) {
    let id = fields
        .value_in(data, "id")
        .unwrap_or_else(|| string_field(data, "id"));
    if let Err(err) = put_document(tx, entity, id, data, fields) {
        API_LOG.error(
            "cannot encode a document",
            kv!["entity" => entity, "id" => id, "err" => log::display(&err)],
        );
        return;
    }

    tx.ensure_type(entity);
}

fn update_schema_if_needed(tx: &mut Tx<'_>, entity: &[u8], data: &Map) {
    if !schema_enabled() || entity == schema_entity() {
        return;
    }

    let current = tx.read_shared(schema_entity(), entity);
    if schema_strict()
        && let Some(current) = &current
        && schema::is_manual_schema(entity, Some(current))
    {
        return;
    }

    let schema_fields = ByteFields::id(entity);
    let mut merged = schema::merge_analyzed_schema(entity, current.as_deref(), data);
    if let Some(current) = &current {
        if merged == **current {
            return;
        }

        if let Ok(encoded) = encode_document(&merged, &schema_fields)
            && let Some(stored) = tx.read_bytes(schema_entity(), entity)
            && encoded.as_slice() == &*stored
        {
            return;
        }
    }

    write_entity_tx(tx, schema_entity(), &mut merged, true, &schema_fields);
}

pub fn write_list_of_entities(entity: &[u8], list: &mut [Map]) -> Vec<Vec<ValidationError>> {
    write_list_of_entities_bytes(entity, list, &ByteFields::new())
}

pub fn write_list_of_entities_bytes(
    entity: &[u8],
    list: &mut [Map],
    fields: &ByteFields,
) -> Vec<Vec<ValidationError>> {
    let mut errors: Vec<Vec<ValidationError>> = Vec::new();
    let mut results: Vec<Option<Map>> = Vec::new();

    for data in list.iter_mut() {
        fields.apply(data);
    }

    let source: &[Map] = list;
    let err = update(|tx| {
        errors = Vec::with_capacity(source.len());
        results = vec![None; source.len()];
        for (i, data) in source.iter().enumerate() {
            let mut attempt = deep_copy_document(data);
            let errs = write_entity_tx(tx, entity, &mut attempt, true, fields);
            let valid = errs.is_empty();
            errors.push(errs);
            if valid {
                results[i] = Some(attempt);
            }
        }

        Ok(())
    });

    log_write_error("write_list", entity, err);
    for (i, result) in results.into_iter().enumerate() {
        if let Some(result) = result {
            replace_contents(&mut list[i], result);
        }
    }

    errors
}

pub fn get_entity_schema(entity: &[u8]) -> Option<Map> {
    read_entity_by_id(schema_entity(), entity)
}

fn delete_all_entities_tx(tx: &mut Tx<'_>, entity: &[u8]) {
    let current = tx.read(schema_entity(), entity);
    if !schema::is_manual_schema(entity, current.as_ref()) {
        tx.delete(schema_entity(), entity);
    }

    tx.reset_entity(entity);
    tx.drop_type(entity);
}

pub fn delete_all_entities(entity: &[u8]) {
    log_write_error(
        "delete_all",
        entity,
        update(|tx| {
            delete_all_entities_tx(tx, entity);
            Ok(())
        }),
    );
}

pub fn delete_all() {
    log_write_error(
        "delete_all",
        b"*",
        update(|tx| {
            for entity in list_entity_types() {
                delete_all_entities_tx(tx, &entity);
            }
            Ok(())
        }),
    );
}

pub fn update_entity_by_id(entity: &[u8], id: &[u8], updated: &Map) -> Option<Map> {
    let mut result = None;
    let err = update(|tx| {
        result = update_entity_by_id_tx(tx, entity, id, updated);
        Ok(())
    });

    log_write_error("update", entity, err);

    result
}

fn update_entity_by_id_tx(tx: &mut Tx<'_>, entity: &[u8], id: &[u8], updated: &Map) -> Option<Map> {
    let mut existing = tx.read(entity, id)?;

    existing.extend(deep_copy_document(updated));
    existing.insert("id".to_string(), Value::String(text_of(id)));

    write_sub_entities(tx, entity, &mut existing);

    persist_entity(tx, entity, &existing, &ByteFields::id(id));
    update_schema_if_needed(tx, entity, &existing);

    Some(existing)
}

pub fn update_list_of_entities(entity: &[u8], updates: &[Map]) -> Option<Vec<Map>> {
    let mut results: Option<Vec<Map>> = None;
    let err = update(|tx| {
        let mut out = Vec::with_capacity(updates.len());
        for upd in updates {
            let id = string_field(upd, "id");
            if id.is_empty() {
                continue;
            }

            if let Some(res) = update_entity_by_id_tx(tx, entity, id, upd) {
                out.push(res);
            }
        }
        results = Some(out);

        Ok(())
    });

    log_write_error("update_list", entity, err);

    results
}

pub fn dump_all() -> BTreeMap<Vec<u8>, Vec<Map>> {
    let mut result = BTreeMap::new();
    for entity in list_entity_types() {
        if entity == schema_entity() {
            continue;
        }

        let docs = list_entities(&entity, 0, 0, b"", true, None, b"", b"");
        result.insert(entity, docs);
    }

    result
}

pub fn count_all_entities() -> usize {
    let mut counter = 0;
    for entity in list_entity_types() {
        if entity == schema_entity() {
            continue;
        }

        counter += count_entities(&entity);
    }

    counter
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct ImportRejection {
        pub entity: Vec<u8> => "entity",
        pub index: i64 => "index",
        pub id: String => "id" [omitempty],
        pub errors: Vec<ValidationError> => "errors",
    }
}

pub type ImportData = BTreeMap<Vec<u8>, Vec<Map>>;

pub const NULL_DOCUMENT_PANIC: &str = "cannot write a null document";

pub trait ImportItem {
    fn document(&self) -> Option<&Map>;
    fn document_mut(&mut self) -> Option<&mut Map>;
}

impl ImportItem for Map {
    fn document(&self) -> Option<&Map> {
        Some(self)
    }

    fn document_mut(&mut self) -> Option<&mut Map> {
        Some(self)
    }
}

impl ImportItem for Option<Map> {
    fn document(&self) -> Option<&Map> {
        self.as_ref()
    }

    fn document_mut(&mut self) -> Option<&mut Map> {
        self.as_mut()
    }
}

pub fn validate_import<T: ImportItem>(
    data: &BTreeMap<Vec<u8>, Vec<T>>,
    schema_for: &dyn Fn(&[u8]) -> Option<Map>,
) -> Vec<ImportRejection> {
    let mut rejections = Vec::new();
    if !schema_enabled() {
        return rejections;
    }

    let empty = Map::new();
    for (entity, items) in data {
        if entity.as_slice() == schema_entity() {
            continue;
        }

        let schema_data = schema_for(entity);
        if !schema::is_manual_schema(entity, schema_data.as_ref()) {
            continue;
        }

        for (index, item) in items.iter().enumerate() {
            let item = item.document().unwrap_or(&empty);
            let errs = schema::validate_entity(entity, item, schema_data.as_ref());
            if !errs.is_empty() {
                rejections.push(new_import_rejection(entity, index, item, errs));
            }
        }
    }

    rejections
}

pub fn new_import_rejection(
    entity: &[u8],
    index: usize,
    item: &Map,
    errs: Vec<ValidationError>,
) -> ImportRejection {
    ImportRejection {
        entity: entity.to_vec(),
        index: index as i64,
        id: text_of(string_field(item, "id")),
        errors: errs,
    }
}

const IMPORT_BATCH: usize = 1000;

pub fn import_all<T: ImportItem>(data: &mut BTreeMap<Vec<u8>, Vec<T>>) -> Vec<ImportRejection> {
    let mut rejections = Vec::new();
    for (entity, items) in data.iter_mut() {
        delete_entity_documents(entity);

        let mut start = 0;
        while start < items.len() {
            let end = (start + IMPORT_BATCH).min(items.len());
            let mut batch: Vec<ImportRejection> = Vec::new();
            let err = update(|tx| {
                batch = Vec::new();
                for (index, item) in items.iter_mut().enumerate().take(end).skip(start) {
                    let Some(item) = item.document_mut() else {
                        let empty = Map::new();
                        let errs = validate_write(tx, entity, &empty, false);
                        if !errs.is_empty() {
                            batch.push(new_import_rejection(entity, index, &empty, errs));
                            continue;
                        }
                        std::panic::panic_any(NULL_DOCUMENT_PANIC.to_string());
                    };
                    let mut attempt = deep_copy_document(item);
                    let errs = write_entity_tx(tx, entity, &mut attempt, false, &ByteFields::new());
                    if !errs.is_empty() {
                        batch.push(new_import_rejection(entity, index, item, errs));
                        continue;
                    }
                    replace_contents(item, attempt);
                }
                Ok(())
            });
            log_write_error("import", entity, err);
            rejections.append(&mut batch);
            start += IMPORT_BATCH;
        }

        log::debug(
            "entity type imported",
            kv!["component" => "api", "entity" => entity.as_slice(), "documents" => items.len() as i64],
        );
    }

    rejections
}

#[derive(Clone, Debug, Default)]
pub struct BatchOperation {
    pub kind: String,
    pub entity: Vec<u8>,
    pub id: Vec<u8>,
    pub data: Map,
    pub fields: ByteFields,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BatchError {
    Validation,
    Update,
}

impl fmt::Display for BatchError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BatchError::Validation => "validation error",
            BatchError::Update => "update failed",
        })
    }
}

impl std::error::Error for BatchError {}

pub const ERR_BATCH_VALIDATION: BatchError = BatchError::Validation;
pub const ERR_BATCH_UPDATE: BatchError = BatchError::Update;

pub fn commit_batch(ops: &[BatchOperation]) -> Result<(), BoxError> {
    update(|tx| {
        for op in ops {
            match op.kind.as_str() {
                "write" => {
                    let mut data = deep_copy_document(&op.data);
                    op.fields.apply(&mut data);
                    if !write_entity_tx(tx, &op.entity, &mut data, true, &op.fields).is_empty() {
                        return Err(Box::new(BatchError::Validation));
                    }
                }
                "update" => {
                    if update_entity_by_id_tx(tx, &op.entity, &op.id, &op.data).is_none() {
                        return Err(Box::new(BatchError::Update));
                    }
                }
                "delete" => tx.delete(&op.entity, &op.id),
                _ => {}
            }
        }

        Ok(())
    })
}
