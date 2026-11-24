use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::errors;
use elysiandb::gocompat::json::{self, Encode, Map, Value};
use elysiandb::gocompat::time::{Duration, Time};
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::log::{self, Options};
use elysiandb::metrics::counters::HOOK_FAILURE;
use elysiandb::metrics::{self, Moment, Sources};
use elysiandb::transport::http::monitoring;

use super::common;
use super::tempdir::TempDir;
use super::{Restore, log_state};

const START: i64 = 1_800_000_000;

const RENAMED: [(&str, &str); 5] = [
    ("heap_bytes", "allocated_bytes"),
    ("heap_live_bytes", "memory_live_bytes"),
    ("goroutines", "threads"),
    ("gomaxprocs", "usable_cpus"),
    ("go_version", "rust_version"),
];

const REMOVED: [&str; 2] = ["gc_pause_p99_ms", "gc_cycles_rps"];

const RUNTIME_KEYS: [&str; 26] = [
    "allocated_bytes",
    "memory_live_bytes",
    "total_bytes",
    "held_bytes",
    "threads",
    "usable_cpus",
    "rust_version",
    "open_fds",
    "uptime_seconds",
    "uptime",
    "started_at",
    "rss_bytes",
    "gc_pause_p99_ms",
    "gc_cycles_rps",
    "gc_cycles",
    "gc_pause_ms",
    "cpu_percent",
    "cpu_user_seconds",
    "cpu_system_seconds",
    "cpu_seconds",
    "pid",
    "now",
    "version",
    "startup_ms",
    "generated_at",
    "since",
];

const PROM_RENAMED: [(&str, &str); 7] = [
    (
        "# HELP elysiandb_heap_bytes Go heap in use.",
        "# HELP elysiandb_allocated_bytes Bytes allocated by the process.",
    ),
    (
        "# TYPE elysiandb_heap_bytes gauge",
        "# TYPE elysiandb_allocated_bytes gauge",
    ),
    (
        "elysiandb_heap_bytes <MASK>",
        "elysiandb_allocated_bytes <MASK>",
    ),
    (
        "# HELP elysiandb_memory_bytes Memory mapped by the Go runtime.",
        "# HELP elysiandb_memory_bytes Peak memory of the process.",
    ),
    (
        "# HELP elysiandb_goroutines Goroutines.",
        "# HELP elysiandb_threads Threads of the process.",
    ),
    (
        "# TYPE elysiandb_goroutines gauge",
        "# TYPE elysiandb_threads gauge",
    ),
    ("elysiandb_goroutines <MASK>", "elysiandb_threads <MASK>"),
];

const PROM_RUNTIME: [&str; 12] = [
    "threads",
    "usable_cpus",
    "allocated_bytes",
    "memory_live_bytes",
    "memory_bytes",
    "total_bytes",
    "held_bytes",
    "rss_bytes",
    "open_fds",
    "uptime_seconds",
    "start_time_seconds",
    "build_info",
];

const PROM_RUNTIME_PREFIXES: [&str; 3] = ["cpu_", "gc_", "process_"];

fn millis(n: u64) -> StdDuration {
    StdDuration::from_millis(n)
}

fn run_scenario(missing_folder: String) {
    globals::set_config(Some(Arc::new({
        let mut cfg = Config::default();
        cfg.stats.enabled = true;
        cfg
    })));
    metrics::reset();
    metrics::set_sources(Sources {
        data_folder: Some(Arc::new(move || missing_folder.clone())),
        kv_keys: Some(Arc::new(|| (10, 2))),
        entity_counts: Some(Arc::new(|| {
            BTreeMap::from([(b"book".to_vec(), 30), (b"author".to_vec(), 8)])
        })),
        index_backlog: Some(Arc::new(|| 3)),
        cache_entries: Some(Arc::new(|| 12)),
        recovery_sizes: Some(Arc::new(|| (1 << 20, 4 << 20))),
        admin_sessions: Some(Arc::new(|| 1)),
        transactions: Some(Arc::new(|| (2, millis(1500)))),
        http_connections: Some(Arc::new(|| 5)),
        default_admin_password: Some(Arc::new(|| true)),
        heavy_waiting: Some(Arc::new(|| 1)),
        ..Sources::default()
    });

    let start = Moment::from_wall(Time::from_unix(START, 0).utc());
    let boom = errors::new("boom");
    for s in 0..130u64 {
        let now = start.add(Duration::SECOND * s as i64);
        for i in 0..s % 7 {
            metrics::observe_http("GET", "/api/{entity}", 200, millis(1 + i * 3), "list");
        }
        if s % 5 == 0 {
            metrics::observe_http("POST", "/api/{entity}", 400, millis(2), "write");
            metrics::observe_tcp("GET", StdDuration::from_micros(300));
            metrics::kv_hit();
            metrics::kv_miss();
            metrics::cache_hit();
        }
        if s % 11 == 0 {
            metrics::observe_hook("h1", "book", "post_read", millis(4), None);
            metrics::observe_hook_query("h1", "book", "post_read", 20, s % 22 == 0, millis(1));
            metrics::slow_request();
            metrics::failed_login("203.0.113.5");
        }
        if s == 60 {
            metrics::observe_hook("h2", "book", "pre_read", millis(1), Some(&*boom));
            HOOK_FAILURE.lock().at =
                Some(Moment::from_wall(Time::from_unix(START - 30 * 86_400, 0)));
            metrics::record_flush("checkpoint", millis(40), true, None);
            metrics::heavy_query(millis(15));
            metrics::heavy_rejected();
            metrics::request_abandoned();
        }
        metrics::tick(now);
    }
}

fn is_rfc_time(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() < 19 {
        return false;
    }
    let digits = [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18];
    digits.iter().all(|&i| b[i].is_ascii_digit())
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
}

fn mask(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                if RUNTIME_KEYS.contains(&k.as_str())
                    || k.starts_with("cpu_")
                    || k.starts_with("gc_")
                {
                    out.insert(k.clone(), Value::String("<MASK>".to_string()));
                    continue;
                }
                out.insert(k.clone(), mask(val));
            }
            Value::Object(out)
        }
        Value::String(s) if is_rfc_time(s) => Value::String("<T>".to_string()),
        Value::Array(list) => Value::Array(list.iter().map(mask).collect()),
        other => other.clone(),
    }
}

fn rename(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, val) in m {
                if REMOVED.contains(&k.as_str()) {
                    continue;
                }
                let k = RENAMED
                    .iter()
                    .find(|(from, _)| from == k)
                    .map(|(_, to)| to.to_string())
                    .unwrap_or_else(|| k.clone());
                out.insert(k, rename(val));
            }
            Value::Object(out)
        }
        Value::Array(list) => Value::Array(list.iter().map(rename).collect()),
        other => other.clone(),
    }
}

fn parse(text: &[u8]) -> Value {
    json::unmarshal_value(text).unwrap_or_else(|e| panic!("cannot parse {text:?}: {e}"))
}

fn marshal(v: &Value) -> String {
    let b = json::marshal(v).unwrap_or_else(|e| panic!("marshal: {e}"));
    String::from_utf8(b).expect("UTF-8 JSON")
}

fn go_side(text: &str) -> String {
    let parsed = parse(text.as_bytes());
    assert_eq!(marshal(&parsed), text, "the Go text must round-trip");
    marshal(&rename(&parsed))
}

fn rust_side<T: Encode>(v: &T) -> String {
    let b = json::marshal(v).unwrap_or_else(|e| panic!("marshal: {e}"));
    marshal(&mask(&parse(&b)))
}

fn prom_name(line: &str) -> Option<&str> {
    let rest = line.strip_prefix("elysiandb_")?;
    let end = rest.find([' ', '{'])?;
    Some(&rest[..end])
}

fn prom_masked(line: &str) -> String {
    let Some(name) = prom_name(line) else {
        return line.to_string();
    };
    let runtime = PROM_RUNTIME.contains(&name)
        || PROM_RUNTIME_PREFIXES
            .iter()
            .any(|p| name.len() > p.len() && name.starts_with(p));
    if !runtime {
        return line.to_string();
    }
    let head_end = match line.find(' ') {
        Some(space) => space,
        None => return line.to_string(),
    };

    format!("{} <MASK>", &line[..head_end])
}

fn log_lines_masked(line: &str) -> String {
    if line.starts_with("elysiandb_log_lines_total{")
        && let Some(space) = line.rfind(' ')
    {
        return format!("{} <LOG>", &line[..space]);
    }

    line.to_string()
}

fn field<'a>(row: &'a Value, key: &str) -> &'a Value {
    row.get(key)
        .unwrap_or_else(|| panic!("missing {key} in row"))
}

#[test]
fn metrics_golden_matches_go() {
    let _state = log_state();
    let _restore = Restore;
    log::configure(&Options {
        level: "debug".to_string(),
        ..Options::default()
    })
    .expect("configure");
    log::set_output(Some(Box::new(std::io::sink())), false);

    let tmp = TempDir::new();
    run_scenario(tmp.path().join("data").to_string_lossy().into_owned());

    let prom = String::from_utf8(metrics::prometheus()).expect("UTF-8 Prometheus text");
    let (warnings, errors_count) = log::counts();
    assert!(
        prom.contains(&format!(
            "elysiandb_log_lines_total{{level=\"warn\"}} {warnings}\n"
        )),
        "warn log lines"
    );
    assert!(
        prom.contains(&format!(
            "elysiandb_log_lines_total{{level=\"error\"}} {errors_count}\n"
        )),
        "error log lines"
    );

    let golden = String::from_utf8(common::read_golden("ops/metrics/prometheus.prom"))
        .expect("UTF-8 golden");
    assert!(golden.ends_with('\n') && prom.ends_with('\n'));
    let mut applied = [0usize; PROM_RENAMED.len()];
    let want: Vec<String> = golden
        .lines()
        .map(|line| {
            let mut line = line.to_string();
            for (i, (from, to)) in PROM_RENAMED.iter().enumerate() {
                if line == *from {
                    line = to.to_string();
                    applied[i] += 1;
                }
            }
            log_lines_masked(&line)
        })
        .collect();
    assert_eq!(applied, [1; PROM_RENAMED.len()], "D17 renames applied once");
    let got: Vec<String> = prom
        .lines()
        .map(|l| log_lines_masked(&prom_masked(l)))
        .collect();
    let mut prom_checked = 0;
    for (i, w) in want.iter().enumerate() {
        assert_eq!(got.get(i), Some(w), "prometheus line {}", i + 1);
        prom_checked += 1;
    }
    assert_eq!(got.len(), want.len(), "prometheus line count");
    assert_eq!(prom_checked, golden.lines().count());

    let rows = common::read_jsonl("ops/metrics/series.jsonl");
    let mut series_checked = 0;
    for row in &rows {
        if field(row, "fn").as_str() == Some("SeriesNames") {
            let mut want: Vec<String> = field(row, "names")
                .as_array()
                .expect("names")
                .iter()
                .map(|n| n.as_str().expect("name").to_string())
                .filter(|n| !REMOVED.contains(&n.as_str()))
                .map(|n| {
                    RENAMED
                        .iter()
                        .find(|(from, _)| *from == n)
                        .map(|(_, to)| to.to_string())
                        .unwrap_or(n)
                })
                .collect();
            want.sort();
            let mut got: Vec<String> = metrics::series_names()
                .iter()
                .map(|n| n.to_string())
                .collect();
            got.sort();
            assert_eq!(got, want, "series names");
            series_checked += 1;
            continue;
        }

        assert_eq!(field(row, "fn").as_str(), Some("Series"));
        let range = field(row, "range").as_str().expect("range");
        let since = field(row, "since").as_i64().expect("since");
        let ok = field(row, "ok").as_bool().expect("ok");
        let body = field(row, "body").as_str().expect("body");

        let (resp, got_ok) = metrics::series(range, since);
        assert_eq!(got_ok, ok, "range {range:?} since {since}");
        assert_eq!(
            rust_side(&resp),
            go_side(body),
            "range {range:?} since {since}"
        );
        series_checked += 1;
    }
    assert_eq!(series_checked, rows.len());
    assert_eq!(series_checked, 16);

    let (raw, _) = metrics::series("15m", 0);
    let raw = String::from_utf8(raw.marshal_json()).expect("UTF-8");
    assert!(
        raw.starts_with("{\"range\":\"15m\",\"step_seconds\":1,\"slow_request_ms\":0,\"time\":[")
    );
    let mut at = raw.find("\"series\":{").expect("series");
    for name in metrics::series_names() {
        let key = format!("\"{name}\":[");
        let pos = raw[at..].find(&key).map(|p| p + at).expect("series key");
        at = pos + key.len();
    }

    let mut files_checked = 0;
    for (name, got) in [
        ("overview", rust_side(&metrics::current_overview())),
        ("details", rust_side(&metrics::current_details())),
        ("status", rust_side(&metrics::current_status())),
        ("instance", rust_side(&metrics::current_instance())),
    ] {
        let golden = String::from_utf8(common::read_golden(&format!("ops/metrics/{name}.json")))
            .expect("UTF-8 golden");
        let text = golden.strip_suffix('\n').expect("trailing newline");
        assert_eq!(got, go_side(text), "{name}.json");
        files_checked += 1;
    }
    assert_eq!(files_checked, 4);

    let overview = String::from_utf8(json::marshal(&metrics::current_overview()).expect("marshal"))
        .expect("UTF-8");
    let keys = [
        "{\"generated_at\":",
        ",\"instance\":{\"version\":",
        ",\"rust_version\":",
        ",\"engine\":",
        ",\"started_at\":",
        ",\"uptime_seconds\":",
        ",\"http\":{\"enabled\":",
        ",\"tcp\":{\"enabled\":",
        ",\"auth_mode\":",
        ",\"features\":{\"access_log\":",
        ",\"data_folder\":",
        ",\"status\":{\"level\":",
        ",\"traffic\":{\"requests_per_second\":",
        ",\"data\":{\"kv_keys\":",
        ",\"persistence\":{\"last_flush_at\":",
        ",\"runtime\":{\"allocated_bytes\":",
        ",\"total_bytes\":",
        ",\"threads\":",
        ",\"cpu_percent\":",
        ",\"connections\":{\"http\":",
        ",\"logs\":{\"level\":",
    ];
    let mut at = 0;
    for key in keys {
        let pos = overview[at..]
            .find(key)
            .map(|p| p + at)
            .unwrap_or_else(|| panic!("{key} out of order in {overview}"));
        at = pos + key.len();
    }

    metrics::reset();
}

fn get(h: fn(&mut RequestCtx), uri: &str) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(uri);
    req.header.set_method("GET");
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    h(&mut ctx);
    ctx
}

fn json_body(ctx: &RequestCtx) -> String {
    assert_eq!(ctx.response.status_code(), 200);
    assert_eq!(ctx.response.header.content_type(), b"application/json");
    rust_side(&parse(ctx.response.body_bytes()))
}

#[test]
fn monitoring_endpoints_serve_the_go_metrics() {
    let _state = log_state();
    let _restore = Restore;
    log::configure(&Options {
        level: "debug".to_string(),
        ..Options::default()
    })
    .expect("configure");
    log::set_output(Some(Box::new(std::io::sink())), false);

    let tmp = TempDir::new();
    run_scenario(tmp.path().join("data").to_string_lossy().into_owned());

    let rows = common::read_jsonl("ops/metrics/series.jsonl");
    let series_rows: Vec<&Value> = rows
        .iter()
        .filter(|row| field(row, "fn").as_str() == Some("Series"))
        .collect();
    let mut series_checked = 0;
    for row in &series_rows {
        let range = field(row, "range").as_str().expect("range");
        let since = field(row, "since").as_i64().expect("since");
        let ctx = get(
            monitoring::series_controller,
            &format!("/monitoring/series?range={range}&since={since}"),
        );

        let effective = if range.is_empty() { "15m" } else { range };
        let want = series_rows
            .iter()
            .find(|r| {
                field(r, "range").as_str() == Some(effective)
                    && field(r, "since").as_i64() == Some(since)
            })
            .expect("row of the effective range");
        if field(want, "ok").as_bool() == Some(true) {
            let body = field(want, "body").as_str().expect("body");
            assert_eq!(
                json_body(&ctx),
                go_side(body),
                "GET /monitoring/series range {range:?} since {since}"
            );
        } else {
            assert_eq!(ctx.response.status_code(), 400, "range {range:?}");
            assert_eq!(
                ctx.response.body_bytes(),
                b"{\"error\":\"range must be 15m, 1h or 24h\"}",
                "range {range:?}"
            );
        }
        series_checked += 1;
    }
    assert_eq!(series_checked, 15);

    let mut files_checked = 0;
    for (name, h) in [
        (
            "overview",
            monitoring::overview_controller as fn(&mut RequestCtx),
        ),
        ("details", monitoring::details_controller),
    ] {
        let golden = String::from_utf8(common::read_golden(&format!("ops/metrics/{name}.json")))
            .expect("UTF-8 golden");
        let text = golden.strip_suffix('\n').expect("trailing newline");
        let ctx = get(h, &format!("/monitoring/{name}"));
        assert_eq!(json_body(&ctx), go_side(text), "GET /monitoring/{name}");
        files_checked += 1;
    }
    assert_eq!(files_checked, 2);

    metrics::reset();
}
