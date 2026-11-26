use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use crate::api;
use crate::backup;
use crate::cache;
use crate::durable;
use crate::engine;
use crate::hook;
use crate::kv;
use crate::metrics::{
    self, BackupSample, DiskSample, EngineSample, HookCacheSample, IndexState, Sources,
};
use crate::security;
use crate::storage;
use crate::transaction;

use super::init::{BOOT_LOG, config, listener};
use super::server::http_connections;

fn count(n: usize) -> i64 {
    i64::try_from(n).unwrap_or(i64::MAX)
}

pub fn monitoring_sources() -> Sources {
    let mut sources = Sources {
        data_folder: Some(Arc::new(|| config().store.folder.clone())),
        kv_keys: Some(Arc::new(storage::count_keys)),
        entity_counts: Some(Arc::new(entity_counts)),
        index_backlog: Some(Arc::new(|| count(api::index_backlog()))),
        recovery_sizes: Some(Arc::new(storage_log_sizes)),
        engine: Some(Arc::new(engine_sample)),
        hook_caches: Some(Arc::new(hook_caches)),
        heavy_waiting: Some(Arc::new(api::heavy_waiting)),
        indexes: Some(Arc::new(index_states)),
        transactions: Some(Arc::new(|| {
            let (open, oldest) = transaction::open_transactions();
            (count(open), oldest)
        })),
        http_connections: Some(Arc::new(http_connections)),
        cache_entries: Some(Arc::new(|| {
            cache::cache_store().map_or(0, |store| count(store.entries()))
        })),
        ..Sources::default()
    };

    sources.disk = Some(Arc::new(disk_sample));

    sources.backup = Some(Arc::new(backup_sample));

    if security::user_authentication_is_enabled() {
        sources.admin_sessions = Some(Arc::new(|| count(security::active_session_count())));
    }

    if security::basic_authentication_is_enabled() || security::user_authentication_is_enabled() {
        sources.default_admin_password = Some(Arc::new(security::default_admin_password_is_in_use));
    }

    sources
}

pub fn boot_monitoring() {
    metrics::set_sources(monitoring_sources());
    metrics::start();
}

pub fn warn_if_authentication_is_off_on_public_address() {
    let cfg = config();

    let mut hosts: Vec<&str> = Vec::new();
    if cfg.server.http.enabled {
        hosts.push(&cfg.server.http.host);
    }
    if cfg.server.tcp.enabled {
        hosts.push(&cfg.server.tcp.host);
    }

    if metrics::authentication_off_on_public_address(cfg.security.authentication.enabled, &hosts) {
        let http = listener(&cfg.server.http);
        let tcp = listener(&cfg.server.tcp);
        BOOT_LOG.warn(
            "authentication is disabled on a non-loopback address",
            kv![
                "http" => http,
                "tcp" => tcp,
                "hint" => "anyone who can reach these addresses can read and change all the data: enable security.authentication",
            ],
        );
    }
}

fn entity_counts() -> BTreeMap<Vec<u8>, i64> {
    engine::count_entities_per_type()
        .into_iter()
        .map(|(name, n)| (name, count(n)))
        .collect()
}

fn storage_log_sizes() -> (i64, i64) {
    let Some(e) = storage::engine() else {
        return (0, 0);
    };

    (0, e.log_since_checkpoint())
}

fn engine_sample() -> EngineSample {
    let Some(e) = storage::engine() else {
        return EngineSample::default();
    };

    let st = e.stats();

    EngineSample {
        log_since_checkpoint: st.log_since_checkpoint,
        log_bytes: st.log_bytes,
        live_bytes: st.live_bytes,
        value_cache_hits: st.value_cache_hits,
        value_cache_misses: st.value_cache_misses,
        resident_bytes: st.resident_struct_bytes,
        value_cache_bytes: st.value_cache_bytes,
        dirty_page_bytes: st.dirty_page_bytes,
        index_count: count(st.index_count),
        index_bytes: st.index_bytes,
        checkpoint_ms: st.last_checkpoint_ms,
    }
}

fn disk_sample() -> DiskSample {
    let io = durable::stats();
    let mut sample = DiskSample {
        read_bytes: io.read_bytes,
        write_bytes: io.write_bytes,
        syncs: io.syncs,
        sync_nanos: io.sync_nanos,
        refused_writes: storage::refused_writes(),
        ..DiskSample::default()
    };

    if let Ok(free) = durable::free_bytes(Path::new(&config().store.folder)) {
        sample.free_bytes = free;
    }

    if let Some(e) = storage::engine() {
        sample.index_upkeep_bytes = e.index_backlog_bytes();
    }

    sample
}

fn backup_sample() -> BackupSample {
    let st = backup::current_status();

    BackupSample {
        running: st.running,
        completed: st.completed,
        failed: st.failed,
        last_ok: st.last_ok,
        last_at: st.last_at,
        duration: st.duration,
        bytes: st.bytes,
    }
}

fn hook_caches() -> HookCacheSample {
    let (q, o, f) = (hook::query_cache(), hook::output_memo(), hook::folds());

    HookCacheSample {
        query_hits: q.hits,
        query_misses: q.misses,
        output_hits: o.hits,
        output_misses: o.misses,
        fold_hits: f.cached + f.incremental,
        fold_passes: f.full_passes,
    }
}

fn index_states() -> Vec<IndexState> {
    let Some(e) = storage::engine() else {
        return Vec::new();
    };

    e.index_states()
        .into_iter()
        .map(|st| IndexState {
            entity: st.entity,
            index: st.index,
            state: st.state.to_string(),
            entries: st.entries,
            last_used_at: if st.last_used.is_zero() {
                None
            } else {
                Some(st.last_used)
            },
            kind: String::from_utf8_lossy(&st.kind).into_owned(),
            origin: String::from_utf8_lossy(&st.origin).into_owned(),
            pinned: st.pinned,
            bytes: st.bytes,
            uses: st.uses,
        })
        .collect()
}
