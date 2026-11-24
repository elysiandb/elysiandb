use std::collections::BTreeMap;
use std::fmt;
use std::sync::Arc;

use crate::datastore::{Engine, Tx};
use crate::globals;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::Map;
use crate::kv;
use crate::log::{self, Logger};
use crate::query::Filters;
use crate::storage;

use super::filter::filters_match_entity;
use super::guard::{self, Guard, GuardError};
use super::include::{apply_includes_with, extract_auto_includes, merge_includes};
use super::plan::{ListQuery, plan};

pub const CORE_ENTITY_TYPE_PREFIX: &str = "_elysiandb_core_";

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
    if let Err(err) = err {
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
                globals::api_entity_index_id_key(&String::from_utf8_lossy(entity))
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
