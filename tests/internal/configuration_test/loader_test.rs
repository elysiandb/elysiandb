use elysiandb::configuration::{self as cfgpkg, load_config};

use super::serial;
use super::tempdir::TempDir;

#[test]
fn load_config_ok() {
    let _serial = serial();
    let tmp = TempDir::new();
    let folder = tmp.path().to_string_lossy().into_owned();

    let yaml = format!(
        "
store:
  folder: \"{folder}\"
  shards: 16
  flushIntervalSeconds: 7
server:
  http:
    enabled: true
    host: \"127.0.0.1\"
    port: 9090
  tcp:
    enabled: true
    host: \"0.0.0.0\"
    port: 8088
log:
  flushIntervalSeconds: 3
"
    );

    let path = tmp.path().join("elysian.yaml");
    if let Err(e) = std::fs::write(&path, yaml) {
        panic!("write yaml: {e}");
    }

    let cfg = match load_config(&path) {
        Ok(cfg) => cfg,
        Err(e) => panic!("LoadConfig returned error: {e}"),
    };

    assert_eq!(
        cfg.store.folder, folder,
        "Store.Folder = {:?}, want {folder:?}",
        cfg.store.folder
    );
    assert_eq!(
        cfg.store.shards, 16,
        "Store.Shards = {}, want 16",
        cfg.store.shards
    );
    assert_eq!(
        cfg.store.flush_interval_seconds, 7,
        "Store.FlushIntervalSeconds = {}, want 7",
        cfg.store.flush_interval_seconds
    );

    let http = &cfg.server.http;
    assert!(
        http.enabled && http.host == "127.0.0.1" && http.port == 9090,
        "HTTP server parsed wrong: {http:?}"
    );
    let tcp = &cfg.server.tcp;
    assert!(
        tcp.enabled && tcp.host == "0.0.0.0" && tcp.port == 8088,
        "TCP server parsed wrong: {tcp:?}"
    );

    assert_eq!(
        cfg.log.flush_interval_seconds, 3,
        "Log.FlushIntervalSeconds = {}, want 3",
        cfg.log.flush_interval_seconds
    );
}

#[test]
fn load_config_file_missing_returns_error() {
    let _serial = serial();
    let tmp = TempDir::new();
    let result = load_config(&tmp.path().join("this-file-does-not-exist.yaml"));
    assert!(
        result.is_err(),
        "expected an error for a missing file, got {result:?}"
    );
}

#[test]
fn load_config_invalid_yaml_returns_error() {
    let _serial = serial();
    let tmp = TempDir::new();
    let path = tmp.path().join("bad.yaml");
    if let Err(e) = std::fs::write(&path, "store: [unclosed") {
        panic!("write bad yaml: {e}");
    }

    let result = load_config(&path);
    assert!(
        result.is_err(),
        "expected an error for invalid YAML, got {result:?}"
    );
}

#[test]
fn load_config_log_defaults() {
    let _serial = serial();
    let tmp = TempDir::new();
    let path = tmp.path().join("elysian.yaml");
    std::fs::write(&path, "store:\n  folder: /tmp/x\n").unwrap();

    let cfg = load_config(&path).unwrap_or_else(|e| panic!("LoadConfig: {e}"));

    assert!(
        cfg.log.level == "info" && cfg.log.format == "text" && !cfg.log.access_log,
        "unexpected log defaults {:?}",
        cfg.log
    );
    assert!(
        cfg.log.slow_request_ms == cfgpkg::DEFAULT_SLOW_REQUEST_MS
            && cfg.log.buffer_lines == cfgpkg::DEFAULT_BUFFER_LINES,
        "unexpected log defaults {:?}",
        cfg.log
    );
    assert!(
        !cfg.monitoring.prometheus.enabled,
        "prometheus must be off by default"
    );
}

#[test]
fn load_config_log_values() {
    let _serial = serial();
    let tmp = TempDir::new();
    let path = tmp.path().join("elysian.yaml");
    let content = "log:\n  level: debug\n  format: json\n  accessLog: true\n  slowRequestMs: 0\n  bufferLines: 0\nmonitoring:\n  prometheus:\n    enabled: true\n    token: secret\n";
    std::fs::write(&path, content).unwrap();

    let cfg = load_config(&path).unwrap_or_else(|e| panic!("LoadConfig: {e}"));

    assert!(
        cfg.log.level == "debug"
            && cfg.log.format == "json"
            && cfg.log.access_log
            && cfg.log.slow_request_ms == 0
            && cfg.log.buffer_lines == 0,
        "unexpected log config {:?}",
        cfg.log
    );
    assert!(
        cfg.monitoring.prometheus.enabled && cfg.monitoring.prometheus.token == "secret",
        "unexpected monitoring config {:?}",
        cfg.monitoring
    );
    assert!(
        !cfg.to_json().contains("secret"),
        "the prometheus token must be redacted: {}",
        cfg.to_json()
    );
}

#[test]
fn load_config_rejects_invalid_log_values() {
    let _serial = serial();
    let cases = [
        ("bufferLines", "log:\n  bufferLines: -5\n"),
        ("format", "log:\n  format: xml\n"),
        ("level", "log:\n  level: verbose\n"),
        ("slowRequestMs", "log:\n  slowRequestMs: -1\n"),
    ];

    for (name, content) in cases {
        let tmp = TempDir::new();
        let path = tmp.path().join("elysian.yaml");
        std::fs::write(&path, content).unwrap();

        assert!(load_config(&path).is_err(), "{name}: expected an error");
    }
}

#[test]
fn load_config_query_overload_settings() {
    let _serial = serial();
    let tmp = TempDir::new();
    let write = |content: &str| {
        let path = tmp.path().join("elysian.yaml");
        std::fs::write(&path, format!("store:\n  folder: /tmp/x\n{content}")).unwrap();
        path
    };

    let cfg = load_config(&write("")).unwrap_or_else(|e| panic!("LoadConfig: {e}"));
    assert!(
        cfg.api.query.max_queued == 0
            && cfg.api.query.queue_timeout_ms == 0
            && cfg.api.query.cancels_abandoned_requests(),
        "unexpected defaults {:?}",
        cfg.api.query
    );

    let cfg = load_config(&write(
        "api:\n  query:\n    maxQueued: -1\n    queueTimeoutMs: -1\n    cancelOnDisconnect: false\n",
    ))
    .unwrap_or_else(|e| panic!("LoadConfig: {e}"));
    assert!(
        cfg.api.query.max_queued == -1
            && cfg.api.query.queue_timeout_ms == -1
            && !cfg.api.query.cancels_abandoned_requests(),
        "unexpected values {:?}",
        cfg.api.query
    );

    for content in [
        "api:\n  query:\n    maxQueued: -2\n",
        "api:\n  query:\n    queueTimeoutMs: -5\n",
    ] {
        assert!(
            load_config(&write(content)).is_err(),
            "a value below -1 is rejected: {content:?}"
        );
    }
}
