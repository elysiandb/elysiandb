use std::sync::Arc;
use std::time::Instant;

use crate::api;
use crate::backup;
use crate::cache;
use crate::configuration::{Config, ServerConfig};
use crate::globals;
use crate::gocompat::time::Duration;
use crate::kv;
use crate::log::{self, Logger, Value};
use crate::metrics;
use crate::storage::{self, MODE_SERVER, MODE_TOOL, OpenMode};

use super::acl::boot_acl;
use super::cache::boot_api_cache_cleaner;
use super::hook::boot_hooks;
use super::memory::apply_memory_limit;

pub(crate) const BOOT_LOG: Logger = log::for_component("boot");
const STORAGE_LOG: Logger = log::for_component("storage");

const OPEN_HINT: &str =
    "stop the server first, or use GET /api/backup or elysiandb backup --url on the running server";

pub(crate) fn config() -> Arc<Config> {
    match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    }
}

pub fn log_configuration(path: &[u8]) {
    let cfg = config();

    let engine = globals::engine();
    let auth_mode = metrics::auth_mode();
    let http = listener(&cfg.server.http);
    let tcp = listener(&cfg.server.tcp);
    BOOT_LOG.info(
        "configuration loaded",
        kv![
            "path" => Value::Str(path),
            "engine" => engine,
            "auth_mode" => auth_mode,
            "http" => http,
            "tcp" => tcp,
            "admin_ui" => cfg.admin_ui.enabled,
            "crash_recovery" => cfg.store.crash_recovery.enabled,
            "cache" => cfg.api.cache.enabled,
            "hooks" => cfg.api.hooks.enabled,
            "schema" => cfg.api.schema.enabled,
            "stats" => cfg.stats.enabled,
            "log_level" => log::level_name(log::configured_level()),
            "log_format" => log::format(),
        ],
    );

    if cfg.log.flush_interval_seconds != 0 {
        BOOT_LOG.warn(
            "deprecated configuration key is ignored",
            kv![
                "key" => "log.flushIntervalSeconds",
                "hint" => "logs are now written immediately, remove the key",
            ],
        );
    }
}

pub(crate) fn listener(server: &ServerConfig) -> String {
    if !server.enabled {
        return "disabled".to_string();
    }

    format!("{}:{}", server.host, server.port)
}

fn warn_about_interrupted_restore() {
    backup::remove_abandoned_uploads();
    if backup::restore_was_interrupted() {
        BOOT_LOG.error(
            "a restore was interrupted before its end: the data is incomplete, run the restore again",
            kv![],
        );
    }
}

pub fn init_db() {
    api::index::register();
    let cfg = config();
    warn_about_interrupted_restore();

    if cfg.api.cache.enabled {
        cache::init_cache(Duration(
            cfg.api
                .cache
                .cleanup_interval_seconds
                .wrapping_mul(Duration::SECOND.0),
        ));
    }

    let start = Instant::now();
    if let Err(err) = storage::open(MODE_SERVER) {
        log::fatal(
            "cannot open the data folder",
            kv![
                "component" => "storage",
                "folder" => cfg.store.folder,
                "err" => log::display(&err),
            ],
        );
    }

    log_storage_loaded(&cfg, start);

    boot_acl();
    boot_hooks();
    boot_api_cache_cleaner();
}

pub fn init_bulk_tool() {
    apply_memory_limit();
    open_tool(MODE_SERVER);
}

pub fn init_tool() {
    open_tool(MODE_TOOL);
}

fn open_tool(mode: OpenMode) {
    api::index::register();
    let cfg = config();

    let start = Instant::now();
    if let Err(err) = storage::open(mode) {
        log::fatal(
            "cannot open the data folder",
            kv![
                "component" => "storage",
                "folder" => cfg.store.folder,
                "err" => log::display(&err),
                "hint" => OPEN_HINT,
            ],
        );
    }

    log_storage_loaded(&cfg, start);
}

fn log_storage_loaded(cfg: &Config, start: Instant) {
    let (keys, with_ttl) = storage::count_keys();
    let documents = storage::count_json_documents();
    STORAGE_LOG.info(
        "storage loaded",
        kv![
            "folder" => cfg.store.folder,
            "kv_keys" => keys,
            "kv_keys_with_ttl" => with_ttl,
            "documents" => documents,
            "duration_ms" => log::since(start),
        ],
    );
}
