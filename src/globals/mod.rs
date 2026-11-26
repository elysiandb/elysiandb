mod api;
mod config;
mod format;
mod procs;

pub use api::{
    API_ENTITIES_PATTERN, API_ENTITY_INDEX_ALL_FIELDS_PATTERN, API_ENTITY_INDEX_FIELD_ALL_PATTERN,
    API_ENTITY_INDEX_FIELD_FILTER_PATTERN, API_ENTITY_INDEX_FIELD_SORT_ASC_PATTERN,
    API_ENTITY_INDEX_FIELD_SORT_DESC_PATTERN, API_ENTITY_INDEX_ID_PATTERN,
    API_ENTITY_INDEX_PATTERN, API_ENTITY_PATTERN, API_ENTITY_TYPES_LIST_PATTERN, API_KEY_PREFIX,
    API_SINGLE_ENTITY_PATTERN, CORE_FIELDS_PREFIX, api_all_entity_types_list_key,
    api_entities_all_key, api_entity_index_all_fields_key, api_entity_index_field_all_key,
    api_entity_index_field_key, api_entity_index_field_sort_asc_key,
    api_entity_index_field_sort_desc_key, api_entity_index_id_key, api_entity_index_pattern_key,
    api_entity_key, api_single_entity_key, is_api_key, is_starting, set_starting,
};
pub use config::{config, engine, set_config};
pub use format::{BLUE, BOLD, FAINT, GOLD, GRAY, RED, RESET, VIOLET};
pub use procs::{PROCS_ENV, parse_procs, procs};

pub const VERSION: &str = env!("ELYSIANDB_BUILD_VERSION");

static VERSION_OVERRIDE: crate::sync::RwLock<Option<String>> = crate::sync::RwLock::new(None);

pub fn version() -> String {
    match VERSION_OVERRIDE.read().as_ref() {
        Some(v) => v.clone(),
        None => VERSION.to_string(),
    }
}

pub fn set_version(v: Option<String>) {
    *VERSION_OVERRIDE.write() = v;
}
