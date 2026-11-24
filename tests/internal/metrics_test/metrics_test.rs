use std::collections::BTreeMap;
use std::io;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration as StdDuration;

use elysiandb::configuration::{AuthenticationConfig, Config, ServerConfig};
use elysiandb::globals;
use elysiandb::gocompat::errors;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::time::Duration;
use elysiandb::kv;
use elysiandb::log::{self, Options};
use elysiandb::metrics::{
    self, BackupSample, Detail, DiskSample, Moment, Problem, Sources, Status,
};
use elysiandb::sync::{Mutex, MutexGuard};

use super::tempdir::TempDir;

static SERIAL: Mutex<()> = Mutex::new(());

struct Setup {
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for Setup {
    fn drop(&mut self) {
        metrics::reset();
    }
}

fn setup(sources: Sources, edit: impl FnOnce(&mut Config)) -> Setup {
    let serial = SERIAL.lock();
    let dir = TempDir::new();

    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.server.http = ServerConfig {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 8089,
        ..ServerConfig::default()
    };
    cfg.server.tcp = ServerConfig {
        enabled: true,
        host: "127.0.0.1".to_string(),
        port: 8088,
        ..ServerConfig::default()
    };
    cfg.security.authentication = AuthenticationConfig {
        enabled: true,
        mode: "user".to_string(),
        ..AuthenticationConfig::default()
    };
    cfg.log.slow_request_ms = 500;
    edit(&mut cfg);
    globals::set_config(Some(Arc::new(cfg)));

    metrics::reset();
    metrics::set_sources(sources);

    Setup {
        _dir: dir,
        _serial: serial,
    }
}

struct SeriesJson {
    step_seconds: i64,
    time: Vec<i64>,
    series: BTreeMap<String, Vec<Option<f64>>>,
}

fn series(range_name: &str, since: i64) -> SeriesJson {
    let (s, ok) = metrics::series(range_name, since);
    if !ok {
        panic!("unknown range {range_name:?}");
    }

    let data = json::marshal(&s).unwrap_or_else(|e| panic!("marshal: {e}"));
    let out = json::unmarshal_value(&data).unwrap_or_else(|e| panic!("unmarshal: {e}"));

    let mut parsed = SeriesJson {
        step_seconds: out
            .get("step_seconds")
            .and_then(Value::as_i64)
            .unwrap_or(-1),
        time: Vec::new(),
        series: BTreeMap::new(),
    };
    for t in out
        .get("time")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
    {
        parsed.time.push(t.as_i64().unwrap_or(-1));
    }
    for (name, values) in out
        .get("series")
        .and_then(Value::as_object)
        .into_iter()
        .flatten()
    {
        let list = values
            .as_array()
            .map(|a| a.iter().map(Value::as_f64).collect())
            .unwrap_or_default();
        parsed.series.insert(name.clone(), list);
    }

    parsed
}

fn last(s: &SeriesJson, name: &str) -> Option<f64> {
    let Some(values) = s.series.get(name) else {
        panic!("missing series {name:?}");
    };
    let Some(v) = values.last() else {
        panic!("series {name:?} is empty");
    };

    *v
}

fn problem_codes(status: &Status) -> Vec<&'static str> {
    status.problems.iter().map(|p| p.code).collect()
}

fn has_problem(status: &Status, code: &str) -> Option<Problem> {
    status.problems.iter().find(|p| p.code == code).cloned()
}

fn source<T: 'static>(f: impl Fn() -> T + Send + Sync + 'static) -> metrics::Source<T> {
    Some(Arc::new(f))
}

fn counts(pairs: &[(&str, i64)]) -> BTreeMap<Vec<u8>, i64> {
    pairs
        .iter()
        .map(|(k, v)| (k.as_bytes().to_vec(), *v))
        .collect()
}

fn micros(n: u64) -> StdDuration {
    StdDuration::from_micros(n)
}

fn millis(n: u64) -> StdDuration {
    StdDuration::from_millis(n)
}

#[test]
fn test_rates_and_percentiles_per_second() {
    let _s = setup(Sources::default(), |_| {});
    let t0 = Moment::now();

    metrics::tick(t0.clone());
    for _ in 0..10 {
        metrics::observe_http(
            "GET",
            "/api/{entity}",
            200,
            micros(800),
            metrics::KIND_DOC_READ,
        );
    }
    metrics::observe_http(
        "POST",
        "/api/{entity}",
        500,
        millis(2),
        metrics::KIND_DOC_WRITE,
    );
    metrics::observe_http("GET", "/kv/{key}", 404, millis(1), metrics::KIND_KV_GET);
    for _ in 0..4 {
        metrics::observe_tcp(metrics::TCP_GET, micros(100));
    }
    metrics::observe_tcp(metrics::TCP_OTHER, micros(100));
    metrics::tick(t0.add(Duration::SECOND));

    let s = series("15m", 0);
    if s.step_seconds != 1 || s.time.len() != 1 {
        panic!(
            "unexpected series step={} points={}",
            s.step_seconds,
            s.time.len()
        );
    }

    let checks = [
        ("http_rps", 12.0),
        ("http_2xx_rps", 10.0),
        ("http_4xx_rps", 1.0),
        ("http_5xx_rps", 1.0),
        ("tcp_rps", 5.0),
        ("tcp_get_rps", 4.0),
        ("tcp_other_rps", 1.0),
        ("kv_gets_rps", 5.0),
        ("doc_reads_rps", 10.0),
        ("doc_writes_rps", 1.0),
    ];
    for (name, want) in checks {
        let got = last(&s, name);
        if got != Some(want) {
            panic!("{name} = {got:?}, want {want}");
        }
    }

    let p50 = last(&s, "http_p50_ms");
    if !p50.is_some_and(|p| p > 0.5 && p <= 1.0) {
        panic!("p50 should fall in the 0.5-1 ms bucket, got {p50:?}");
    }

    if let Some(p) = last(&s, "hook_p95_ms") {
        panic!("a percentile without samples must be null, got {p}");
    }
}

#[test]
fn test_series_since_and_resolutions() {
    let _s = setup(Sources::default(), |_| {});
    let t0 = Moment::now().add(-(Duration::SECOND * 20));

    for i in 0..=12 {
        metrics::tick(t0.add(Duration::SECOND * i));
    }

    let all = series("15m", 0);
    if all.time.len() != 12 {
        panic!("expected 12 one-second points, got {}", all.time.len());
    }

    let since = series("15m", all.time[9]);
    if since.time.len() != 2 || since.time[0] != all.time[10] {
        panic!(
            "since should return the newer points only: {:?}",
            since.time
        );
    }

    let hour = series("1h", 0);
    if hour.step_seconds != 10 || hour.time.len() != 1 {
        panic!(
            "expected one 10-second point, got step={} points={}",
            hour.step_seconds,
            hour.time.len()
        );
    }

    let day = series("24h", 0);
    if day.step_seconds != 60 || !day.time.is_empty() {
        panic!(
            "expected no minute point yet, got step={} points={}",
            day.step_seconds,
            day.time.len()
        );
    }

    if metrics::series("2h", 0).1 {
        panic!("unknown ranges must be refused");
    }

    if all.series.len() != metrics::series_names().len() {
        panic!("every series must be returned");
    }
}

#[test]
fn test_gauges_come_from_sources() {
    let _s = setup(
        Sources {
            kv_keys: source(|| (42, 7)),
            entity_counts: source(|| counts(&[("books", 3), ("authors", 2)])),
            index_backlog: source(|| 5),
            cache_entries: source(|| 9),
            recovery_sizes: source(|| (100, 200)),
            admin_sessions: source(|| 2),
            transactions: source(|| (1, StdDuration::from_secs(30))),
            http_connections: source(|| 4),
            ..Sources::default()
        },
        |_| {},
    );
    let t0 = Moment::now();

    metrics::tcp_connection_opened();
    metrics::tcp_connection_opened();
    metrics::tcp_connection_closed();

    metrics::tick(t0.clone());
    metrics::tick(t0.add(Duration::SECOND));

    let s = series("15m", 0);
    let checks = [
        ("kv_keys", 42.0),
        ("kv_keys_ttl", 7.0),
        ("entities", 5.0),
        ("index_backlog", 5.0),
        ("cache_entries", 9.0),
        ("recovery_json_bytes", 100.0),
        ("recovery_store_bytes", 200.0),
        ("admin_sessions", 2.0),
        ("open_transactions", 1.0),
        ("oldest_transaction_seconds", 30.0),
        ("http_conns", 4.0),
        ("tcp_conns", 1.0),
    ];
    for (name, want) in checks {
        let got = last(&s, name);
        if got != Some(want) {
            panic!("{name} = {got:?}, want {want}");
        }
    }

    let got = last(&s, "allocated_bytes");
    if !got.is_some_and(|v| v > 0.0) {
        panic!("allocated_bytes should come from the counting allocator, got {got:?}");
    }
}

#[test]
fn test_flush_status_and_problem() {
    let _s = setup(Sources::default(), |_| {});
    let now = Moment::now();

    let disk_full = errors::new("disk full");
    metrics::record_flush("store", millis(3), true, None);
    metrics::record_flush("json", millis(1), true, Some(&*disk_full));
    metrics::tick(now.clone());

    let p = metrics::persistence();
    if p.last_flush_ok
        || p.last_error != "disk full"
        || p.flush_failures != 1
        || p.last_flush_at.is_none()
    {
        panic!("unexpected persistence {p:?}");
    }

    let status = metrics::current_status();
    if status.level != metrics::STATUS_CRITICAL {
        panic!("a failing flush is critical: {status:?}");
    }
    if has_problem(&status, "flush_failing").is_none() {
        panic!("missing flush_failing in {:?}", problem_codes(&status));
    }

    metrics::record_flush("store", millis(1), false, None);
    metrics::tick(now.add(Duration::SECOND));
    if has_problem(&metrics::current_status(), "flush_failing").is_none() {
        panic!("the json target still fails");
    }

    metrics::record_flush("json", millis(1), true, None);
    metrics::tick(now.add(Duration::SECOND * 2));

    let p = metrics::persistence();
    if !p.last_flush_ok || !p.last_error.is_empty() || p.last_success_at.is_none() {
        panic!("persistence should be ok again: {p:?}");
    }
    if has_problem(&metrics::current_status(), "flush_failing").is_some() {
        panic!("flush_failing should be cleared");
    }
}

#[test]
fn test_error_rate_and_latency_problems() {
    let _s = setup(Sources::default(), |cfg| cfg.log.slow_request_ms = 10);
    let t0 = Moment::now();

    metrics::tick(t0.clone());
    for i in 0..30 {
        let status = if i < 3 { 500 } else { 200 };
        metrics::observe_http("GET", "/api/{entity}", status, millis(50), "");
    }
    metrics::tick(t0.add(Duration::SECOND));

    let status = metrics::current_status();
    let rate = has_problem(&status, "error_rate_high");
    if !rate
        .as_ref()
        .is_some_and(|p| p.details.get("error_rate") == Some(&Detail::Float(0.1)))
    {
        panic!("expected error_rate_high at 0.1, got {status:?}");
    }

    let latency = has_problem(&status, "latency_high");
    let p95 = latency
        .as_ref()
        .and_then(|p| match p.details.get("p95_ms") {
            Some(Detail::Float(f)) => Some(*f),
            _ => None,
        });
    if !p95.is_some_and(|p| p > 10.0) {
        panic!("expected latency_high, got {status:?}");
    }
}

#[test]
fn test_few_requests_do_not_raise_error_rate() {
    let _s = setup(Sources::default(), |_| {});
    let t0 = Moment::now();

    metrics::tick(t0.clone());
    metrics::observe_http("GET", "/api/{entity}", 500, millis(1), "");
    metrics::tick(t0.add(Duration::SECOND));

    if has_problem(&metrics::current_status(), "error_rate_high").is_some() {
        panic!("one failed request out of one is not enough to raise the problem");
    }
}

#[test]
fn test_security_and_configuration_problems() {
    let _s = setup(
        Sources {
            default_admin_password: source(|| true),
            ..Sources::default()
        },
        |cfg| {
            cfg.security.authentication.enabled = false;
            cfg.server.http.host = "0.0.0.0".to_string();
        },
    );
    let now = Moment::now();

    for _ in 0..21 {
        metrics::failed_login("10.0.0.7");
    }
    metrics::tick(now);

    let status = metrics::current_status();
    for code in [
        "default_admin_password",
        "auth_disabled_public",
        "auth_failures",
    ] {
        if has_problem(&status, code).is_none() {
            panic!("missing {code} in {:?}", problem_codes(&status));
        }
    }

    let p = has_problem(&status, "auth_failures");
    if p.as_ref().and_then(|p| p.details.get("remote_addr"))
        != Some(&Detail::Str(b"10.0.0.7".to_vec()))
    {
        panic!("auth_failures should name the address: {p:?}");
    }

    if status.level != metrics::STATUS_WARNING {
        panic!("expected warning, got {}", status.level);
    }
}

#[test]
fn test_hook_recovery_replay_transaction_and_backlog_problems() {
    let backlog = Arc::new(AtomicI64::new(3));
    let read = backlog.clone();
    let _s = setup(
        Sources {
            index_backlog: source(move || read.load(Ordering::SeqCst)),
            transactions: source(|| (1, StdDuration::from_secs(11 * 60))),
            ..Sources::default()
        },
        |_| {},
    );
    let now = Moment::now();

    let reference = errors::new("ReferenceError");
    metrics::observe_hook("h1", "books", "post_read", millis(1), Some(&*reference));
    let no_space = errors::new("no space left");
    metrics::recovery_write_failed(&*no_space);
    metrics::recovery_replayed(12);

    metrics::tick(now.clone());
    metrics::tick(now.add(Duration::SECOND * 61));

    let status = metrics::current_status();
    for code in [
        "hook_failing",
        "recovery_write_failing",
        "unclean_shutdown",
        "old_transaction",
        "index_backlog",
    ] {
        if has_problem(&status, code).is_none() {
            panic!("missing {code} in {:?}", problem_codes(&status));
        }
    }

    let p = has_problem(&status, "hook_failing");
    if p.as_ref().and_then(|p| p.details.get("hook_id")) != Some(&Detail::Str(b"h1".to_vec())) {
        panic!("hook_failing should name the hook: {p:?}");
    }
    let p = has_problem(&status, "unclean_shutdown");
    if !p.as_ref().is_some_and(|p| {
        p.severity == metrics::SEVERITY_INFO && p.details.get("entries") == Some(&Detail::Int(12))
    }) {
        panic!("unexpected unclean_shutdown {p:?}");
    }
    if status.problems[0].severity != metrics::SEVERITY_CRITICAL {
        panic!("critical problems come first: {:?}", status.problems);
    }

    backlog.store(0, Ordering::SeqCst);
    metrics::tick(now.add(Duration::SECOND * 62));
    if has_problem(&metrics::current_status(), "index_backlog").is_some() {
        panic!("index_backlog should clear when the backlog is empty");
    }
}

#[test]
fn test_problem_keeps_its_start_time() {
    let _s = setup(
        Sources {
            default_admin_password: source(|| true),
            ..Sources::default()
        },
        |_| {},
    );
    let t0 = Moment::now();

    metrics::tick(t0.clone());
    let first = has_problem(&metrics::current_status(), "default_admin_password");
    metrics::tick(t0.add(Duration::SECOND));
    let second = has_problem(&metrics::current_status(), "default_admin_password");

    let first = first.and_then(|p| p.since);
    let second = second.and_then(|p| p.since);
    match (&first, &second) {
        (Some(a), Some(b)) if !a.is_zero() && a.equal(b) => {}
        _ => panic!("since must stay the same: {first:?} {second:?}"),
    }
}

#[test]
fn test_overview() {
    let _s = setup(
        Sources {
            kv_keys: source(|| (10, 1)),
            entity_counts: source(|| {
                counts(&[("a", 1), ("b", 6), ("c", 5), ("d", 4), ("e", 3), ("f", 2)])
            }),
            ..Sources::default()
        },
        |_| {},
    );
    let t0 = Moment::now();

    metrics::tick(t0.clone());
    for _ in 0..9 {
        metrics::observe_http("GET", "/api/{entity}", 200, millis(1), "");
    }
    metrics::observe_http("GET", "/api/{entity}", 503, millis(1), "");
    metrics::tick(t0.add(Duration::SECOND));

    let o = metrics::current_overview();

    if o.instance.auth_mode != "user"
        || o.instance.engine != "internal"
        || !o.instance.http.enabled
        || o.instance.http.address != "127.0.0.1:8089"
    {
        panic!("unexpected instance {:?}", o.instance);
    }
    if o.traffic.http_requests_per_second.map(|n| n.0) != Some(10.0) {
        panic!("unexpected traffic {:?}", o.traffic);
    }
    if o.traffic.error_rate.map(|n| n.0) != Some(0.1) {
        panic!("unexpected error rate {:?}", o.traffic.error_rate);
    }
    if o.data.entities != 21
        || o.data.entity_types != 6
        || o.data.top_entity_types.len() != 5
        || o.data.top_entity_types[0].name != b"b"
    {
        panic!("unexpected data {:?}", o.data);
    }
    if o.data.kv_keys.map(|n| n.0) != Some(10.0) {
        panic!("unexpected kv keys {:?}", o.data.kv_keys);
    }

    let data = json::marshal(&o).unwrap_or_else(|e| panic!("marshal: {e}"));
    let text = String::from_utf8_lossy(&data);
    for key in [
        "\"instance\"",
        "\"status\"",
        "\"traffic\"",
        "\"data\"",
        "\"persistence\"",
        "\"runtime\"",
        "\"connections\"",
        "\"logs\"",
    ] {
        if !text.contains(key) {
            panic!("missing {key} in {text}");
        }
    }
}

#[derive(Clone)]
struct Sink;

impl io::Write for Sink {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct RestoreLog;

impl Drop for RestoreLog {
    fn drop(&mut self) {
        log::set_output(None, false);
    }
}

#[test]
fn test_overview_counts_log_lines() {
    let _s = setup(Sources::default(), |_| {});
    let _ = log::configure(&Options::default());
    log::set_output(Some(Box::new(Sink)), false);
    let _restore = RestoreLog;

    let t0 = Moment::now().add(-(Duration::SECOND * 15));
    metrics::tick(t0.clone());
    log::warn("w1", kv![]);
    log::warn("w2", kv![]);
    log::error("e1", kv![]);
    for i in 1..=11 {
        metrics::tick(t0.add(Duration::SECOND * i));
    }

    let o = metrics::current_overview();
    if o.logs.warnings_1h != 2 || o.logs.errors_1h != 1 || o.logs.level != "info" {
        panic!("unexpected log summary {:?}", o.logs);
    }
}

#[test]
fn test_details() {
    let _s = setup(
        Sources {
            entity_counts: source(|| counts(&[("books", 2), ("authors", 9)])),
            ..Sources::default()
        },
        |_| {},
    );
    let now = Moment::now();

    for _ in 0..3 {
        metrics::observe_http("GET", "/api/{entity}", 200, millis(1), "");
    }
    metrics::observe_http("POST", "/api/{entity}", 500, millis(1), "");
    metrics::observe_hook("h1", "books", "pre_read", millis(2), None);
    let boom = errors::new("boom");
    metrics::observe_hook("h1", "books", "pre_read", millis(4), Some(&*boom));
    metrics::tick(now);

    let d = metrics::current_details();

    let routes = &d.routes.since_startup;
    if routes.len() != 2
        || routes[0].route != b"/api/{entity}"
        || routes[0].method != b"GET"
        || routes[0].count != 3
    {
        panic!("unexpected routes {routes:?}");
    }
    if routes[1].errors != 1 || routes[1].error_rate != 1.0 {
        panic!("unexpected error counts {:?}", routes[1]);
    }

    if d.hooks.len() != 1
        || d.hooks[0].executions != 2
        || d.hooks[0].errors != 1
        || d.hooks[0].last_error != "boom"
        || d.hooks[0].phase != b"pre_read"
    {
        panic!("unexpected hooks {:?}", d.hooks);
    }
    if d.hooks[0].avg_ms.map(|n| n.0) != Some(3.0) {
        panic!("unexpected average {:?}", d.hooks[0].avg_ms);
    }

    if d.entity_types.len() != 2 || d.entity_types[0].name != b"authors" {
        panic!("unexpected entity types {:?}", d.entity_types);
    }
}

#[test]
fn test_prometheus() {
    let _s = setup(
        Sources {
            entity_counts: source(|| counts(&[("we\"ird", 1)])),
            ..Sources::default()
        },
        |_| {},
    );
    let now = Moment::now();

    metrics::observe_http("GET", "/api/{entity}", 200, millis(1), "");
    metrics::observe_http("GET", "/api/{entity}", 500, StdDuration::from_secs(30), "");
    metrics::observe_tcp(metrics::TCP_SET, millis(1));
    metrics::tick(now);

    let out = String::from_utf8(metrics::prometheus()).unwrap_or_default();

    for want in [
        "# TYPE elysiandb_http_requests_total counter",
        "elysiandb_http_requests_total{status_class=\"2xx\"} 1",
        "elysiandb_http_requests_total{status_class=\"5xx\"} 1",
        "elysiandb_tcp_commands_total{command=\"SET\"} 1",
        "# TYPE elysiandb_http_request_duration_seconds histogram",
        "elysiandb_http_request_duration_seconds_bucket{le=\"+Inf\"} 2",
        "elysiandb_http_request_duration_seconds_count 2",
        "elysiandb_http_request_duration_seconds_bucket{le=\"10\"} 1",
        "elysiandb_entities{entity_type=\"we\\\"ird\"} 1",
        "# TYPE elysiandb_uptime_seconds gauge",
    ] {
        if !out.contains(want) {
            panic!("missing {want:?} in:\n{out}");
        }
    }

    if out.matches("# TYPE elysiandb_http_requests_total").count() != 1 {
        panic!("each metric must be declared once");
    }
}

#[test]
fn test_authentication_off_on_public_address() {
    let cases: [(bool, &[&str], bool); 5] = [
        (false, &["0.0.0.0"], true),
        (false, &["127.0.0.1", "::1", "localhost"], false),
        (false, &["127.0.0.1", "192.168.1.4"], true),
        (true, &["0.0.0.0"], false),
        (false, &[], false),
    ];

    for (enabled, hosts, want) in cases {
        let got = metrics::authentication_off_on_public_address(enabled, hosts);
        if got != want {
            panic!("authentication_off_on_public_address({enabled}, {hosts:?}) = {got}");
        }
    }

    if !metrics::is_loopback_host("[::1]") || metrics::is_loopback_host("") {
        panic!("unexpected loopback detection");
    }
}

#[test]
fn test_slow_sources_disk_and_default_password() {
    let folder = TempDir::new();
    let path = folder.path().join("data.json");
    if let Err(err) = std::fs::write(&path, vec![0u8; 1234]) {
        panic!("{err}");
    }

    let data_folder = folder.path().to_string_lossy().into_owned();
    let _s = setup(
        Sources {
            data_folder: source(move || data_folder.clone()),
            default_admin_password: source(|| false),
            ..Sources::default()
        },
        |_| {},
    );
    let now = Moment::now();

    metrics::tick(now.clone());
    metrics::tick(now.add(Duration::SECOND));

    let s = series("15m", 0);
    let got = last(&s, "disk_bytes");
    if got != Some(1234.0) {
        panic!("disk_bytes = {got:?}, want 1234");
    }

    if has_problem(&metrics::current_status(), "default_admin_password").is_some() {
        panic!("the default password is not in use");
    }
}

#[test]
fn test_start_and_stop() {
    let _s = setup(Sources::default(), |_| {});

    metrics::start();
    metrics::start();
    metrics::stop();
    metrics::stop();

    let (s, _) = metrics::series("15m", 0);
    if !s.points.is_empty() {
        panic!(
            "the first tick only records a baseline, got {} points",
            s.points.len()
        );
    }
}

#[test]
fn test_disk_and_backup_series_follow_their_sources() {
    let disk = Arc::new(Mutex::new(DiskSample {
        read_bytes: 1000,
        write_bytes: 2000,
        syncs: 10,
        sync_nanos: 10_000_000,
        free_bytes: 1 << 30,
        refused_writes: 1,
        index_upkeep_bytes: 77,
    }));
    let last_at = Moment::now().add(-Duration::MINUTE);
    let read = disk.clone();
    let _s = setup(
        Sources {
            disk: source(move || *read.lock()),
            backup: source(move || BackupSample {
                running: 1,
                completed: 3,
                failed: 2,
                last_ok: true,
                last_at: Some(last_at.clone()),
                duration: StdDuration::from_secs(2),
                bytes: 4096,
            }),
            ..Sources::default()
        },
        |_| {},
    );
    metrics::set_startup(millis(1500));

    let t0 = Moment::now();
    metrics::tick(t0.clone());
    {
        let mut d = disk.lock();
        d.read_bytes += 10000;
        d.write_bytes += 40000;
        d.syncs += 5;
        d.sync_nanos += 50_000_000;
        d.refused_writes += 20;
    }
    metrics::tick(t0.add(Duration::SECOND * 10));

    let hour = series("1h", 0);
    let want = [
        ("disk_read_bytes_rps", 1000.0),
        ("disk_write_bytes_rps", 4000.0),
        ("disk_syncs_rps", 0.5),
        ("disk_sync_ms", 10.0),
        ("disk_free_bytes", (1u64 << 30) as f64),
        ("refused_writes_rps", 2.0),
        ("index_upkeep_bytes", 77.0),
        ("backup_running", 1.0),
        ("backup_last_ok", 1.0),
        ("backup_duration_ms", 2000.0),
        ("backup_bytes", 4096.0),
        ("startup_ms", 1500.0),
    ];
    for (name, v) in want {
        let values = hour.series.get(name).cloned().unwrap_or_default();
        if values.last().copied().flatten() != Some(v) {
            panic!("series {name}: {values:?}, expected {v}");
        }
    }

    let ages = hour
        .series
        .get("backup_age_seconds")
        .cloned()
        .unwrap_or_default();
    if !ages.last().copied().flatten().is_some_and(|a| a >= 60.0) {
        panic!("backup age: {ages:?}");
    }

    let text = String::from_utf8(metrics::prometheus()).unwrap_or_default();
    for line in [
        "elysiandb_disk_read_bytes_total 11000",
        "elysiandb_disk_write_bytes_total 42000",
        "elysiandb_disk_syncs_total 15",
        "elysiandb_refused_writes_total 21",
        "elysiandb_disk_free_bytes 1.073741824e+09",
        "elysiandb_backups_total{result=\"ok\"} 3",
        "elysiandb_backups_total{result=\"failed\"} 2",
        "elysiandb_backup_last_duration_seconds 2",
        "elysiandb_startup_seconds 1.5",
    ] {
        if !text.contains(line) {
            panic!("{line:?} is missing:\n{text}");
        }
    }
}
