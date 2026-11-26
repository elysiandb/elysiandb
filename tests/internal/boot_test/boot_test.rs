use std::io;
use std::os::unix::fs::OpenOptionsExt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use elysiandb::acl;
use elysiandb::api;
use elysiandb::boot;
use elysiandb::cache;
use elysiandb::configuration::{AuthenticationConfig, Config, CrashRecoveryConfig, ServerConfig};
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::legacy;
use elysiandb::log::{self, AttrValue, Entry, Level, Options, Query};
use elysiandb::metrics;
use elysiandb::security;
use elysiandb::storage;

use crate::tempdir::TempDir;

struct LogGuard;

impl Drop for LogGuard {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
    }
}

fn setup_log() -> LogGuard {
    if let Err(err) = log::configure(&Options {
        level: "debug".to_string(),
        buffer_lines: 1000,
        ..Options::default()
    }) {
        panic!("{err}");
    }
    log::set_output(Some(Box::new(io::sink())), false);

    LogGuard
}

fn entries(q: Query) -> Vec<Arc<Entry>> {
    log::logs(&q).entries
}

fn text_query(text: &str) -> Query {
    Query {
        text: text.as_bytes().to_vec(),
        ..Query::default()
    }
}

fn level_query(level: Level) -> Query {
    Query {
        min_level: level,
        ..Query::default()
    }
}

fn attr(e: &Entry, key: &str) -> Option<AttrValue> {
    e.attrs.get(key).cloned()
}

fn string(s: &str) -> Option<AttrValue> {
    Some(AttrValue::Str(s.as_bytes().to_vec()))
}

fn server(enabled: bool, host: &str, port: i64) -> ServerConfig {
    ServerConfig {
        enabled,
        host: host.to_string(),
        port,
        ..ServerConfig::default()
    }
}

fn config(folder: &std::path::Path, mutate: impl FnOnce(&mut Config)) -> Arc<Config> {
    let mut cfg = Config::default();
    cfg.store.folder = folder.to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.server.http = server(false, "127.0.0.1", 8089);
    cfg.server.tcp = server(false, "127.0.0.1", 8088);
    cfg.security.authentication = AuthenticationConfig {
        enabled: true,
        mode: "user".to_string(),
        ..AuthenticationConfig::default()
    };
    mutate(&mut cfg);

    let cfg = Arc::new(cfg);
    globals::set_config(Some(cfg.clone()));

    cfg
}

fn object(text: &str) -> Map {
    let mut m = Map::new();
    json::unmarshal(text.as_bytes(), &mut m).unwrap_or_else(|e| panic!("{text}: {e}"));
    m
}

#[test]
fn test_log_configuration() {
    let _log = setup_log();
    let dir = TempDir::new();
    config(dir.path(), |cfg| {
        cfg.server.http.enabled = true;
        cfg.log.flush_interval_seconds = 5;
    });

    boot::log_configuration(b"elysian.yaml");

    let loaded = entries(text_query("configuration loaded"));
    if loaded.len() != 1 {
        panic!("expected one configuration line, got {loaded:?}");
    }

    let e = &loaded[0];
    if attr(e, "path") != string("elysian.yaml")
        || attr(e, "http") != string("127.0.0.1:8089")
        || attr(e, "tcp") != string("disabled")
        || attr(e, "auth_mode") != string("user")
        || attr(e, "log_format") != string("text")
    {
        panic!("unexpected attributes {:?}", e.attrs);
    }

    let deprecated = entries(level_query(Level::WARN));
    if deprecated.len() != 1 || attr(&deprecated[0], "key") != string("log.flushIntervalSeconds") {
        panic!("expected a deprecation warning, got {deprecated:?}");
    }
}

#[test]
fn test_warn_if_authentication_is_off_on_public_address() {
    let _log = setup_log();

    let first = TempDir::new();
    config(first.path(), |cfg| {
        cfg.security.authentication.enabled = false;
        cfg.server.tcp = server(true, "127.0.0.1", 8088);
    });
    boot::warn_if_authentication_is_off_on_public_address();
    let got = entries(level_query(Level::WARN));
    if !got.is_empty() {
        panic!("a loopback address needs no warning, got {got:?}");
    }

    let second = TempDir::new();
    config(second.path(), |cfg| {
        cfg.security.authentication.enabled = false;
        cfg.server.http = server(true, "0.0.0.0", 8089);
    });
    boot::warn_if_authentication_is_off_on_public_address();

    let got = entries(level_query(Level::WARN));
    if got.len() != 1
        || got[0].message != "authentication is disabled on a non-loopback address"
        || attr(&got[0], "http") != string("0.0.0.0:8089")
    {
        panic!("expected one warning, got {got:?}");
    }
}

#[test]
fn test_monitoring_sources() {
    let dir = TempDir::new();
    config(dir.path(), |_| {});
    storage::load_db();
    storage::load_json_db();

    let sources = boot::monitoring_sources();
    if sources.admin_sessions.is_none() || sources.default_admin_password.is_none() {
        panic!("user mode reports sessions and the default password");
    }
    if sources.disk.is_none() {
        panic!("the internal engine reports its disk activity");
    }

    let call = |s: &Option<Arc<dyn Fn() -> i64 + Send + Sync>>| s.as_ref().map_or(-1, |f| f());

    let (keys, _) = sources.kv_keys.as_ref().map_or((1, 1), |f| f());
    if keys != 0 {
        panic!("expected an empty store, got {keys} keys");
    }
    let folder = sources
        .data_folder
        .as_ref()
        .map(|f| f())
        .unwrap_or_default();
    let want = globals::config().map(|c| c.store.folder.clone());
    if Some(folder) != want {
        panic!("unexpected data folder");
    }
    let n = call(&sources.cache_entries);
    if n != 0 {
        panic!("no cache, no entries, got {n}");
    }
    cache::init_cache(GoDuration::MINUTE);
    let n = call(&sources.cache_entries);
    if n != 0 {
        panic!("an empty cache has no entries, got {n}");
    }
    let (open, _) = sources
        .transactions
        .as_ref()
        .map_or((-1, Duration::ZERO), |f| f());
    if open != 0 {
        panic!("expected no transaction, got {open}");
    }
    let (json, store) = sources.recovery_sizes.as_ref().map_or((-1, -1), |f| f());
    if json != 0 || store != 0 {
        panic!("expected no recovery log, got {json} {store}");
    }
    let n = call(&sources.index_backlog);
    if n < 0 {
        panic!("unexpected backlog {n}");
    }
    let n = call(&sources.http_connections);
    if n != 0 {
        panic!("no server, no connection, got {n}");
    }
    let counts = sources
        .entity_counts
        .as_ref()
        .map(|f| f())
        .unwrap_or_default();
    if !counts.is_empty() {
        panic!("expected no entity type, got {counts:?}");
    }

    let token = TempDir::new();
    config(token.path(), |cfg| {
        cfg.security.authentication = AuthenticationConfig {
            enabled: true,
            mode: "token".to_string(),
            token: "t".to_string(),
        };
    });
    let sources = boot::monitoring_sources();
    if sources.admin_sessions.is_some() || sources.default_admin_password.is_some() {
        panic!("token mode has no sessions and no default password");
    }
}

struct MetricsGuard;

impl Drop for MetricsGuard {
    fn drop(&mut self) {
        metrics::reset();
    }
}

#[test]
fn test_boot_monitoring_starts_the_sampler() {
    let dir = TempDir::new();
    config(dir.path(), |_| {});
    storage::load_db();
    storage::load_json_db();
    metrics::reset();
    let _cleanup = MetricsGuard;

    boot::boot_monitoring();

    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let (s, _) = metrics::series("15m", 0);
        if !s.points.is_empty() {
            break;
        }
        if Instant::now() > deadline {
            panic!("the sampler did not record a point");
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

struct CloseGuard;

impl Drop for CloseGuard {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

#[test]
fn test_init_db_migrates_a024_folder_with_a_pending_recovery_log() {
    let seed = TempDir::new();
    config(seed.path(), |_| {});
    storage::load_db();
    if let Err(err) = security::init_admin_user_if_not_exists() {
        panic!("{err}");
    }
    let errs = api::write_entity(b"books", &mut object(r#"{"id":"b1","title":"Dune"}"#));
    assert!(errs.is_empty());

    let folder = TempDir::new();
    let Some(engine) = storage::engine() else {
        panic!("the data folder is not open");
    };
    if let Err(err) = engine.export_legacy(folder.path()) {
        panic!("{err}");
    }
    drop(engine);
    let key = match std::fs::read(seed.path().join(security::KEY_FILENAME)) {
        Ok(key) => key,
        Err(err) => panic!("{err}"),
    };
    write_private(&folder.path().join(security::KEY_FILENAME), &key);
    if let Err(err) = storage::close() {
        panic!("{err}");
    }

    let line = "{\"op\":\"put\",\"key\":\"api:entity:books:id:b2\",\"value\":{\"id\":\"b2\",\"title\":\"Emma\"}}\n";
    write_private(
        &folder.path().join(legacy::JSON_RECOVERY_FILE),
        line.as_bytes(),
    );

    let _log = setup_log();
    config(folder.path(), |cfg| {
        cfg.store.crash_recovery = CrashRecoveryConfig {
            enabled: true,
            max_log_mb: 100,
        };
    });

    boot::init_db();
    let _close = CloseGuard;

    let migrated = entries(text_query("storage migrated"));
    if migrated.len() != 1
        || !matches!(
            attr(&migrated[0], "recovery_entries_replayed"),
            Some(AttrValue::Int(1) | AttrValue::Uint(1))
        )
    {
        panic!(
            "expected one migration line, got {:?}",
            entries(level_query(Level::DEBUG))
        );
    }

    let loaded = entries(text_query("storage loaded"));
    if loaded.len() != 1 {
        panic!("expected one storage loaded line, got {loaded:?}");
    }

    let got = api::count_entities(b"books");
    if got != 2 {
        panic!("both books should be listed, got {got}");
    }

    if acl::get_acl_entity_for_username(b"books", security::DEFAULT_ADMIN_USERNAME.as_bytes())
        .is_none()
    {
        panic!("the admin ACL of books should be created at startup");
    }
}

fn write_private(path: &std::path::Path, data: &[u8]) {
    use std::io::Write;
    let mut f = match std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
    {
        Ok(f) => f,
        Err(err) => panic!("{err}"),
    };
    if let Err(err) = f.write_all(data) {
        panic!("{err}");
    }
}

#[test]
fn test_an_explicit_budget_stays_under_the_container_limit() {
    let _log = setup_log();

    let gb: i64 = 1 << 30;
    let cases = [
        (4 * gb, 8 * gb, 4 * gb),
        (4 * gb, 0, 4 * gb),
        (9 * gb, 8 * gb, ((8 * gb) as f64 * 0.90) as i64),
        (8 * gb, 8 * gb, ((8 * gb) as f64 * 0.90) as i64),
    ];

    for (budget, container, want) in cases {
        let got = boot::budget_within_container(budget, container);
        if got != want {
            panic!("budget {budget} in a container of {container}: {got}, expected {want}");
        }
    }
}
