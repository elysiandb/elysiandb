#[allow(clippy::module_inception)]
mod cache;

pub use cache::{
    CacheStore, DEFAULT_MAX_ENTRY_KB, DEFAULT_MAX_MB, HASH_SIZE, Limits, VersionOf, cache_store,
    clean_expired_entries, default_limits, hash_query, init_cache, limits, set_limits,
    set_version_of, version_of,
};
