use std::collections::{BTreeSet, VecDeque};
use std::sync::Arc;

use crate::cache;
use crate::datastore::{Engine, INDEX_SORT, IndexSpec, parse_index_name};
use crate::globals;
use crate::gocompat::strconv;
use crate::storage;

use super::plan::{ListQuery, plan};
use super::storage::{db, is_core_entity_type};

pub fn register() {
    let mut hooks = storage::hooks();
    hooks.api_key_resolver = Some(Arc::new(resolve_api_key));
    storage::set_hooks(hooks);
    cache::set_version_of(Some(entity_version));
}

pub fn entity_version(entity: &[u8]) -> String {
    let Some(e) = db() else {
        return String::new();
    };

    let snap = e.snapshot(entity);

    format!(
        "{}.{}.{}",
        strconv::format_uint(e.instance(), 36),
        strconv::format_uint(snap.generation(), 36),
        strconv::format_uint(snap.version(), 36)
    )
}

pub fn cache_dependencies(entity: &[u8], includes: &[u8], with_acl: bool) -> Vec<Vec<u8>> {
    let mut deps = vec![entity.to_vec()];
    if with_acl {
        deps.push(ACL_ENTITY.as_bytes().to_vec());
    }

    if includes.is_empty() {
        return deps;
    }

    let Some(e) = db() else {
        return deps;
    };

    let mut seen: BTreeSet<Vec<u8>> = BTreeSet::new();
    seen.insert(entity.to_vec());
    let mut queue: VecDeque<Vec<u8>> = VecDeque::from([entity.to_vec()]);
    while let Some(current) = queue.pop_front() {
        for r in e.snapshot(&current).refs() {
            if !seen.contains(&r.target) && !is_core_entity_type(&r.target) {
                seen.insert(r.target.clone());
                deps.push(r.target.clone());
                queue.push_back(r.target.clone());
            }
        }
    }

    deps
}

const ACL_ENTITY: &str = "_elysiandb_core_acl";

fn sort_spec(field: &[u8]) -> IndexSpec {
    IndexSpec::new(INDEX_SORT, field)
}

pub fn index_exists_for_field(entity: &[u8], field: &[u8]) -> bool {
    let Some(e) = db() else {
        return false;
    };

    e.ensure_index(entity, &sort_spec(field));

    e.snapshot(entity).has_index(&sort_spec(field))
}

pub fn ensure_sort_index(entity: &[u8], field: &[u8]) -> bool {
    db().is_some_and(|e| e.ensure_index(entity, &sort_spec(field)))
}

pub fn get_list_for_indexed_fields(entity: &[u8]) -> Vec<Vec<u8>> {
    let Some(e) = db() else {
        return Vec::new();
    };

    e.index_names(entity)
        .unwrap_or_default()
        .iter()
        .filter_map(|name| parse_index_name(name))
        .filter(|spec| spec.kind == INDEX_SORT)
        .map(|spec| spec.field)
        .collect()
}

pub fn get_sorted_entity_ids_by_field(
    entity: &[u8],
    field: &[u8],
    ascending: bool,
) -> Vec<Vec<u8>> {
    plan(ListQuery {
        entity,
        sort_field: field,
        sort_asc: ascending,
        ..ListQuery::default()
    })
    .ids()
}

pub fn index_backlog() -> usize {
    db().map_or(0, |e| e.index_backlog())
}

fn join_lines(items: &[Vec<u8>]) -> Vec<u8> {
    items.join(b"\n".as_slice())
}

fn resolve_api_key(e: &Engine, key: &[u8]) -> Option<Vec<u8>> {
    if key == globals::api_all_entity_types_list_key().as_bytes() {
        let types = e.types();
        if types.is_empty() {
            return None;
        }

        return Some(join_lines(&types));
    }

    let rest = key.strip_prefix(globals::API_KEY_PREFIX.as_bytes())?;

    const MARKER: &[u8] = b":internal:index:";
    let at = rest.windows(MARKER.len()).position(|w| w == MARKER)?;
    let entity = &rest[..at];
    let index = &rest[at + MARKER.len()..];
    if entity.is_empty() || !e.type_exists(entity) {
        return None;
    }

    if index == b"id" {
        let ids = plan(ListQuery {
            entity,
            ..ListQuery::default()
        })
        .ids();
        if ids.is_empty() {
            return None;
        }

        return Some(join_lines(&ids));
    }

    if index == b"fields:all" {
        let fields = get_list_for_indexed_fields(entity);
        if fields.is_empty() {
            return None;
        }

        return Some(join_lines(&fields));
    }

    if index.starts_with(b"field:")
        && (index.ends_with(b":sort:asc") || index.ends_with(b":sort:desc"))
    {
        let ascending = index.ends_with(b":sort:asc");
        let mut field = &index[b"field:".len()..];
        field = field.strip_suffix(b":sort:asc").unwrap_or(field);
        field = field.strip_suffix(b":sort:desc").unwrap_or(field);
        if !e.snapshot(entity).has_index(&sort_spec(field)) {
            return None;
        }

        return Some(join_lines(&get_sorted_entity_ids_by_field(
            entity, field, ascending,
        )));
    }

    None
}
