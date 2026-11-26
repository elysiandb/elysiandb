use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, mpsc};
use std::time::{Duration, Instant};

use arc_swap::ArcSwapOption;

use crate::configuration::Config;
use crate::datastore::{self, Engine};
use crate::globals;
use crate::gocompat::errors::{self, BoxError};
use crate::gocompat::json::Map;
use crate::gocompat::strings;
use crate::gocompat::time::Time;
use crate::journal;
use crate::kv;
use crate::legacy;
use crate::log::{self, Logger, Throttle};
use crate::metrics;
use crate::sync::{Mutex, RwLock, spawn};

pub const DATA_FILE: &str = legacy::DATA_FILE;
pub const JSON_DATA_FILE: &str = legacy::JSON_DATA_FILE;
pub const EXPIRATION_DATA_FILE: &str = legacy::EXPIRATION_FILE;

static CURRENT: ArcSwapOption<Engine> = ArcSwapOption::const_empty();
static LIFECYCLE: Mutex<()> = Mutex::new(());
const STORAGE_LOG: Logger = log::for_component("storage");
static WRITE_FAILS: Throttle = Throttle::new(Duration::from_secs(60));

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OpenMode {
    Server,
    Tool,
}

pub const MODE_SERVER: OpenMode = OpenMode::Server;
pub const MODE_TOOL: OpenMode = OpenMode::Tool;

pub type CacheBytes = Arc<dyn Fn(&Config) -> i64 + Send + Sync>;
pub type ApiKeyResolver = Arc<dyn Fn(&Engine, &[u8]) -> Option<Vec<u8>> + Send + Sync>;
pub type GetJsonByKey = Arc<dyn Fn(&[u8]) -> Result<Map, BoxError> + Send + Sync>;

#[derive(Clone, Default)]
pub struct Hooks {
    pub value_cache_bytes: Option<CacheBytes>,
    pub page_cache_bytes: Option<CacheBytes>,
    pub dirty_flush_bytes: Option<CacheBytes>,
    pub free_space_probe: Option<datastore::FreeSpaceProbe>,
    pub api_key_resolver: Option<ApiKeyResolver>,
    pub get_json_by_key: Option<GetJsonByKey>,
}

static HOOKS: RwLock<Hooks> = RwLock::new(Hooks {
    value_cache_bytes: None,
    page_cache_bytes: None,
    dirty_flush_bytes: None,
    free_space_probe: None,
    api_key_resolver: None,
    get_json_by_key: None,
});

pub fn hooks() -> Hooks {
    HOOKS.read().clone()
}

pub fn set_hooks(h: Hooks) {
    *HOOKS.write() = h;
}

fn current_config() -> Arc<Config> {
    match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    }
}

fn seconds(n: i64) -> Duration {
    let ns = n.wrapping_mul(1_000_000_000);
    if ns <= 0 {
        return Duration::ZERO;
    }

    Duration::from_nanos(ns as u64)
}

pub fn options(cfg: &Config, mode: OpenMode) -> datastore::Options {
    let mut durability = journal::Mode::Background;
    if cfg.store.crash_recovery.enabled {
        durability = journal::Mode::Write;
    }

    if strings::equal_fold(cfg.store.durability.as_bytes(), b"fsync") {
        durability = journal::Mode::Sync;
    }

    let interval = seconds(cfg.store.flush_interval_seconds);

    let cache_bytes = value_cache_bytes(cfg);
    let page_cache_bytes = page_cache_bytes(cfg);
    let dirty_flush_bytes = dirty_flush_bytes(cfg);
    let min_free_bytes = min_free_bytes(cfg);

    datastore::Options {
        folder: PathBuf::from(&cfg.store.folder),
        shards: cfg.store.shards,
        durability,
        flush_interval: interval,
        checkpoint_every: interval,
        max_log_bytes: cfg
            .store
            .crash_recovery
            .max_log_mb
            .wrapping_mul(1024)
            .wrapping_mul(1024),
        cache_bytes,
        page_cache_bytes,
        dirty_flush_bytes,
        index_workers: cfg.api.index.workers,
        background: mode == OpenMode::Server,
        legacy_recovery: cfg.store.crash_recovery.enabled,
        on_kv_expired: Some(Arc::new(|n: usize| {
            metrics::kv_expired(i64::try_from(n).unwrap_or(i64::MAX))
        })),
        on_write_error: Some(Arc::new(report_write_error)),
        min_free_bytes,
        free_space: HOOKS.read().free_space_probe.clone(),
        ..datastore::Options::default()
    }
}

const DEFAULT_MIN_FREE_DISK_MB: i64 = 64;

fn min_free_bytes(cfg: &Config) -> i64 {
    if cfg.store.min_free_disk_mb < 0 {
        return 0;
    }

    if cfg.store.min_free_disk_mb == 0 {
        return DEFAULT_MIN_FREE_DISK_MB << 20;
    }

    cfg.store.min_free_disk_mb << 20
}

pub fn write_refusal() -> Option<datastore::Error> {
    if let Some(e) = CURRENT.load_full() {
        return e.write_refusal();
    }

    None
}

pub fn note_refused_write() {
    if let Some(e) = CURRENT.load_full() {
        e.note_refused_write();
    }
}

pub fn close_within(limit: Duration) -> Result<(), BoxError> {
    let Some(e) = CURRENT.load_full() else {
        return Ok(());
    };

    if let Err(err) = e.flush_log() {
        STORAGE_LOG.error(
            "cannot flush the log at shutdown",
            kv!["err" => log::display(&err)],
        );
    }
    drop(e);

    let (done, result) = mpsc::channel();
    let closer = spawn("storage-close", move || {
        let _ = done.send(close());
    });

    match result.recv_timeout(limit) {
        Ok(res) => res,
        Err(mpsc::RecvTimeoutError::Timeout) => {
            STORAGE_LOG.warn(
                "the last checkpoint did not finish in time, the log will be replayed at the next start",
                kv!["limit_ms" => i64::try_from(limit.as_millis()).unwrap_or(i64::MAX)],
            );
            Ok(())
        }
        Err(mpsc::RecvTimeoutError::Disconnected) => match closer.join() {
            Err(panic) => std::panic::resume_unwind(panic),
            Ok(()) => Ok(()),
        },
    }
}

pub fn refused_writes() -> i64 {
    if let Some(e) = CURRENT.load_full() {
        return e.refused_writes();
    }

    0
}

fn report_write_error(err: &(dyn StdError + Send + Sync + 'static)) {
    metrics::recovery_write_failed(err);
    let (suppressed, ok) = WRITE_FAILS.allow("log");
    if ok {
        STORAGE_LOG.error(
            "log write failed",
            &log::with_suppressed(kv!["err" => err], suppressed),
        );
    }
}

pub fn value_cache_bytes(cfg: &Config) -> i64 {
    let hook = HOOKS.read().value_cache_bytes.clone();
    match hook {
        Some(f) => f(cfg),
        None => 0,
    }
}

pub fn page_cache_bytes(cfg: &Config) -> i64 {
    let hook = HOOKS.read().page_cache_bytes.clone();
    match hook {
        Some(f) => f(cfg),
        None => 0,
    }
}

pub fn dirty_flush_bytes(cfg: &Config) -> i64 {
    let hook = HOOKS.read().dirty_flush_bytes.clone();
    match hook {
        Some(f) => f(cfg),
        None => 0,
    }
}

pub fn open(mode: OpenMode) -> Result<(), BoxError> {
    let _lifecycle = LIFECYCLE.lock();

    if let Some(old) = CURRENT.load_full() {
        if let Err(err) = old.close() {
            STORAGE_LOG.error(
                "cannot close the previous engine",
                kv!["err" => log::display(&err)],
            );
        }
        CURRENT.store(None);
    }

    let e = datastore::open(options(&current_config(), mode))?;

    CURRENT.store(Some(e.clone()));

    let n = e.replayed_records();
    if n > 0 {
        metrics::recovery_replayed(n);
    }

    Ok(())
}

pub fn load_db() {
    if let Err(err) = open(MODE_TOOL) {
        let cfg = current_config();
        log::fatal(
            "cannot open the data folder",
            kv!["component" => "storage", "folder" => cfg.store.folder, "err" => log::display(&err)],
        );
    }
}

pub fn load_json_db() {
    let reload = match CURRENT.load_full() {
        None => true,
        Some(e) => e.folder().as_os_str().as_bytes() != current_config().store.folder.as_bytes(),
    };

    if reload {
        load_db();
    }
}

pub fn engine() -> Option<Arc<Engine>> {
    CURRENT.load_full()
}

pub fn close() -> Result<(), BoxError> {
    let _lifecycle = LIFECYCLE.lock();

    let Some(e) = CURRENT.swap(None) else {
        return Ok(());
    };

    e.close()
}

static CHECKPOINT_FAILURES: AtomicI64 = AtomicI64::new(0);

pub fn checkpoint() -> Result<(), BoxError> {
    let Some(e) = CURRENT.load_full() else {
        return Ok(());
    };

    let start = Instant::now();
    let res = e.checkpoint();
    let failure: Option<&dyn StdError> = match &res {
        Err(err) => Some(err.as_ref()),
        Ok(()) => None,
    };
    metrics::record_flush(FLUSH_TARGET, start.elapsed(), true, failure);

    if let Err(err) = res {
        CHECKPOINT_FAILURES.fetch_add(1, Ordering::SeqCst);
        let (suppressed, ok) = WRITE_FAILS.allow(FLUSH_TARGET);
        if ok {
            STORAGE_LOG.error(
                "flush failed",
                &log::with_suppressed(
                    kv!["target" => FLUSH_TARGET, "err" => log::display(&err)],
                    suppressed,
                ),
            );
        }

        return Err(err);
    }

    let n = CHECKPOINT_FAILURES.swap(0, Ordering::SeqCst);
    if n > 0 {
        WRITE_FAILS.reset(FLUSH_TARGET);
        STORAGE_LOG.info(
            "flush working again",
            kv!["target" => FLUSH_TARGET, "failures" => n],
        );
    }

    Ok(())
}

const FLUSH_TARGET: &str = "store";

pub fn write_to_db() {
    let _ = checkpoint();
}

pub fn write_store_db() {
    let _ = checkpoint();
}

pub fn write_json_db() {
    let _ = checkpoint();
}

pub static ERR_KEY_NOT_FOUND: LazyLock<errors::Error> =
    LazyLock::new(|| errors::Error::new("key not found"));

fn key_not_found(key: &[u8]) -> BoxError {
    errors::new(format!("key not found: {}", String::from_utf8_lossy(key)))
}

pub fn get_by_key(key: &[u8]) -> Result<Vec<u8>, BoxError> {
    let Some(e) = CURRENT.load_full() else {
        return Err(key_not_found(key));
    };

    if globals::is_api_key(key) {
        if let Some(v) = api_key_value(&e, key) {
            return Ok(v);
        }

        return Err(key_not_found(key));
    }

    match e.kv_get(key) {
        Some((v, _)) => Ok(v.unwrap_or_default()),
        None => Err(key_not_found(key)),
    }
}

fn api_key_value(e: &Engine, key: &[u8]) -> Option<Vec<u8>> {
    let resolver = HOOKS.read().api_key_resolver.clone()?;

    resolver(e, key)
}

pub fn get_by_wildcard_key_with_limit(pattern: &[u8], limit: i64) -> BTreeMap<Vec<u8>, Vec<u8>> {
    let mut out = BTreeMap::new();
    let Some(e) = CURRENT.load_full() else {
        return out;
    };

    let now = Time::now().unix();
    let mut expired: Vec<Vec<u8>> = Vec::new();
    e.kv_match(pattern, limit, |key| {
        let Some((value, exp)) = e.kv_get(key) else {
            return true;
        };

        if exp > 0 && now >= exp {
            expired.push(key.to_vec());
            return true;
        }

        out.insert(key.to_vec(), value.unwrap_or_default());

        true
    });

    for key in &expired {
        delete_by_key(key);
    }

    out
}

pub fn get_by_wildcard_key(pattern: &[u8]) -> BTreeMap<Vec<u8>, Vec<u8>> {
    get_by_wildcard_key_with_limit(pattern, -1)
}

pub fn delete_by_wildcard_key(pattern: &[u8]) -> usize {
    let Some(e) = CURRENT.load_full() else {
        return 0;
    };

    let mut keys: Vec<Vec<u8>> = Vec::new();
    e.kv_match(pattern, -1, |key| {
        keys.push(key.to_vec());
        true
    });

    for key in &keys {
        e.kv_delete(key);
    }

    keys.len()
}

pub fn put_key_value(key: &[u8], value: &[u8]) -> Result<(), BoxError> {
    store_key_value(key, value, 0, true)
}

pub fn put_key_value_with_ttl(key: &[u8], value: &[u8], ttl: i64) -> Result<(), BoxError> {
    let mut expires_at = 0i64;
    if ttl > 0 {
        expires_at = Time::now().unix().wrapping_add(ttl);
    }

    store_key_value(key, value, expires_at, expires_at == 0)
}

pub fn put_key_value_with_expiration(
    key: &[u8],
    value: &[u8],
    expires_at: i64,
) -> Result<(), BoxError> {
    store_key_value(key, value, expires_at, expires_at <= 0)
}

fn store_key_value(
    key: &[u8],
    value: &[u8],
    mut expires_at: i64,
    keep_ttl: bool,
) -> Result<(), BoxError> {
    if globals::is_api_key(key) {
        return Ok(());
    }

    let Some(e) = CURRENT.load_full() else {
        return Err(errors::new("storage is not open"));
    };

    if keep_ttl && let Some(exp) = e.kv_expiration(key) {
        expires_at = exp;
    }

    e.kv_put(key, value, expires_at)
}

pub fn delete_by_key(key: &[u8]) {
    if globals::is_api_key(key) {
        return;
    }

    if let Some(e) = CURRENT.load_full() {
        e.kv_delete(key);
    }
}

pub fn reset_store() {
    if let Some(e) = CURRENT.load_full()
        && let Err(err) = e.kv_reset()
    {
        STORAGE_LOG.error("key-value reset failed", kv!["err" => log::display(&err)]);
    }
}

pub fn clear_all() {
    let Some(e) = CURRENT.load_full() else {
        return;
    };

    if let Err(err) = e.update(|tx| {
        tx.clear_all();
        Ok(())
    }) {
        STORAGE_LOG.error("clear failed", kv!["err" => log::display(&err)]);
    }
}

pub fn release_indexes() {
    if let Some(e) = CURRENT.load_full() {
        e.release_indexes();
    }
}

pub fn wake_indexes() {
    if let Some(e) = CURRENT.load_full() {
        e.wake_indexes();
    }
}

pub fn count_keys() -> (u64, u64) {
    let Some(e) = CURRENT.load_full() else {
        return (0, 0);
    };

    let (keys, with_ttl) = e.kv_count();

    (keys as u64, with_ttl as u64)
}

pub fn list_keys() -> Vec<Vec<u8>> {
    let Some(e) = CURRENT.load_full() else {
        return Vec::new();
    };

    e.kv_keys()
}

pub fn get_key_expiration(key: &[u8]) -> Option<i64> {
    let e = CURRENT.load_full()?;

    e.kv_expiration(key)
}

pub fn key_has_expired(key: &[u8]) -> bool {
    let Some(exp) = get_key_expiration(key) else {
        return false;
    };

    Time::now().unix() >= exp
}

pub fn clean_expirated_keys(now: i64) {
    if let Some(e) = CURRENT.load_full() {
        e.expire_due(now);
    }
}

pub fn clean_all_past_keys() {
    clean_expirated_keys(Time::now().unix().wrapping_sub(1));
}

pub fn count_json_documents() -> u64 {
    let Some(e) = CURRENT.load_full() else {
        return 0;
    };

    let mut total = 0u64;
    for snap in e.snapshot_all().values() {
        total = total.wrapping_add(snap.len() as u64);
    }

    total
}

fn cut<'a>(s: &'a [u8], sep: &[u8]) -> Option<(&'a [u8], &'a [u8])> {
    let i = s.windows(sep.len()).position(|w| w == sep)?;

    Some((&s[..i], &s[i + sep.len()..]))
}

pub fn parse_document_key(key: &[u8]) -> Option<(&[u8], &[u8])> {
    let rest = key.strip_prefix(globals::API_KEY_PREFIX.as_bytes())?;

    let (entity, id) = cut(rest, b":id:")?;
    if entity.is_empty() {
        return None;
    }

    Some((entity, id))
}

pub fn get_json_by_key_no_copy(key: &[u8]) -> Result<Map, BoxError> {
    let Some(e) = CURRENT.load_full() else {
        return Err(errors::new("json store not initialized"));
    };

    let Some((entity, id)) = parse_document_key(key) else {
        return Err(key_not_found(key));
    };

    let Some(doc) = e.snapshot(entity).get(id) else {
        return Err(key_not_found(key));
    };

    doc.decode()
}

pub fn get_json_by_key(key: &[u8]) -> Result<Map, BoxError> {
    let hook = HOOKS.read().get_json_by_key.clone();
    match hook {
        Some(f) => f(key),
        None => get_json_by_key_no_copy(key),
    }
}

pub fn document_version(key: &[u8]) -> Option<String> {
    let e = CURRENT.load_full()?;

    let (entity, id) = parse_document_key(key)?;

    let snap = e.snapshot(entity);
    let doc = snap.get(id)?;

    Some(format!(
        "{}.{}.{}",
        e.instance(),
        snap.generation(),
        doc.lsn()
    ))
}

pub fn put_json_value(key: &[u8], value: &Map) -> Result<(), BoxError> {
    let Some(e) = CURRENT.load_full() else {
        return Err(errors::new("json store not initialized"));
    };

    let Some((entity, id)) = parse_document_key(key) else {
        return Err(errors::new(format!(
            "invalid document key: {}",
            String::from_utf8_lossy(key)
        )));
    };

    e.update(|tx| tx.put(entity, id, value))
}

pub fn delete_json_by_key(key: &[u8]) {
    let Some(e) = CURRENT.load_full() else {
        return;
    };

    let Some((entity, id)) = parse_document_key(key) else {
        return;
    };

    let _ = e.update(|tx| {
        tx.delete(entity, id);
        Ok(())
    });
}

fn api_single_entity_key(entity: &[u8], id: &[u8]) -> Vec<u8> {
    let pattern = globals::API_SINGLE_ENTITY_PATTERN.as_bytes();
    let mut out = Vec::with_capacity(pattern.len() + entity.len() + id.len());
    let mut rest = pattern;
    for arg in [entity, id] {
        let Some((head, tail)) = cut(rest, b"%s") else {
            break;
        };
        out.extend_from_slice(head);
        out.extend_from_slice(arg);
        rest = tail;
    }
    out.extend_from_slice(rest);

    out
}

pub fn list_json_keys() -> Vec<Vec<u8>> {
    let Some(e) = CURRENT.load_full() else {
        return Vec::new();
    };

    let mut keys: Vec<Vec<u8>> = Vec::new();
    for (name, snap) in e.snapshot_all() {
        snap.ascend(0, |d| {
            keys.push(api_single_entity_key(&name, &d.id()));
            true
        });
    }

    keys
}
