mod copy;
#[allow(clippy::module_inception)]
mod storage;
mod wildcard;

pub use copy::deep_copy_document;
pub use storage::{
    ApiKeyResolver, CacheBytes, DATA_FILE, ERR_KEY_NOT_FOUND, EXPIRATION_DATA_FILE, GetJsonByKey,
    Hooks, JSON_DATA_FILE, MODE_SERVER, MODE_TOOL, OpenMode, checkpoint, clean_all_past_keys,
    clean_expirated_keys, clear_all, close, close_within, count_json_documents, count_keys,
    delete_by_key, delete_by_wildcard_key, delete_json_by_key, dirty_flush_bytes, document_version,
    engine, get_by_key, get_by_wildcard_key, get_by_wildcard_key_with_limit, get_json_by_key,
    get_json_by_key_no_copy, get_key_expiration, hooks, key_has_expired, list_json_keys, list_keys,
    load_db, load_json_db, note_refused_write, open, options, page_cache_bytes, parse_document_key,
    put_json_value, put_key_value, put_key_value_with_expiration, put_key_value_with_ttl,
    refused_writes, release_indexes, reset_store, set_hooks, value_cache_bytes, wake_indexes,
    write_json_db, write_refusal, write_store_db, write_to_db,
};
pub use wildcard::match_glob;
