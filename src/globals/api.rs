use std::sync::atomic::{AtomicBool, Ordering};

pub const CORE_FIELDS_PREFIX: &str = "_elysiandb_core_";

pub const API_KEY_PREFIX: &str = "api:entity:";

pub const API_ENTITY_TYPES_LIST_PATTERN: &str = "api:entity:types:list";
pub const API_ENTITY_PATTERN: &str = "api:entity:%s";
pub const API_ENTITIES_PATTERN: &str = "api:entity:%s:*";
pub const API_SINGLE_ENTITY_PATTERN: &str = "api:entity:%s:id:%s";
pub const API_ENTITY_INDEX_ID_PATTERN: &str = "api:entity:%s:internal:index:id";
pub const API_ENTITY_INDEX_PATTERN: &str = "api:entity:%s:internal:index:*";
pub const API_ENTITY_INDEX_FIELD_FILTER_PATTERN: &str =
    "api:entity:%s:internal:index:field:%s:filter";
pub const API_ENTITY_INDEX_ALL_FIELDS_PATTERN: &str = "api:entity:%s:internal:index:fields:all";
pub const API_ENTITY_INDEX_FIELD_ALL_PATTERN: &str = "api:entity:%s:internal:index:field:%s:*";
pub const API_ENTITY_INDEX_FIELD_SORT_ASC_PATTERN: &str =
    "api:entity:%s:internal:index:field:%s:sort:asc";
pub const API_ENTITY_INDEX_FIELD_SORT_DESC_PATTERN: &str =
    "api:entity:%s:internal:index:field:%s:sort:desc";

fn fill(pattern: &str, args: &[&str]) -> String {
    let mut out =
        String::with_capacity(pattern.len() + args.iter().map(|a| a.len()).sum::<usize>());
    let mut rest = pattern;
    for arg in args {
        let Some((head, tail)) = rest.split_once("%s") else {
            break;
        };
        out.push_str(head);
        out.push_str(arg);
        rest = tail;
    }
    out.push_str(rest);
    out
}

pub fn is_api_key(key: impl AsRef<[u8]>) -> bool {
    key.as_ref().starts_with(API_KEY_PREFIX.as_bytes())
}

pub fn api_all_entity_types_list_key() -> String {
    API_ENTITY_TYPES_LIST_PATTERN.to_string()
}

pub fn api_entity_key(entity: &str) -> String {
    fill(API_ENTITY_PATTERN, &[entity])
}

pub fn api_entities_all_key(entity: &str) -> String {
    fill(API_ENTITIES_PATTERN, &[entity])
}

pub fn api_single_entity_key(entity: &str, id: &str) -> String {
    fill(API_SINGLE_ENTITY_PATTERN, &[entity, id])
}

pub fn api_entity_index_id_key(entity: &str) -> String {
    fill(API_ENTITY_INDEX_ID_PATTERN, &[entity])
}

pub fn api_entity_index_field_key(entity: &str, field: &str) -> String {
    fill(API_ENTITY_INDEX_FIELD_FILTER_PATTERN, &[entity, field])
}

pub fn api_entity_index_field_sort_asc_key(entity: &str, field: &str) -> String {
    fill(API_ENTITY_INDEX_FIELD_SORT_ASC_PATTERN, &[entity, field])
}

pub fn api_entity_index_field_sort_desc_key(entity: &str, field: &str) -> String {
    fill(API_ENTITY_INDEX_FIELD_SORT_DESC_PATTERN, &[entity, field])
}

pub fn api_entity_index_all_fields_key(entity: &str) -> String {
    fill(API_ENTITY_INDEX_ALL_FIELDS_PATTERN, &[entity])
}

pub fn api_entity_index_pattern_key(entity: &str) -> String {
    fill(API_ENTITY_INDEX_PATTERN, &[entity])
}

pub fn api_entity_index_field_all_key(entity: &str, field: &str) -> String {
    fill(API_ENTITY_INDEX_FIELD_ALL_PATTERN, &[entity, field])
}

static STARTING: AtomicBool = AtomicBool::new(false);

pub fn set_starting(v: bool) {
    STARTING.store(v, Ordering::SeqCst);
}

pub fn is_starting() -> bool {
    STARTING.load(Ordering::SeqCst)
}
