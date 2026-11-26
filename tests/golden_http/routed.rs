use std::collections::BTreeMap;
use std::io::Write;
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::{Arc, Once};
use std::time::Duration;

use elysiandb::acl;
use elysiandb::api as api_storage;
use elysiandb::cache;
use elysiandb::configuration;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::hook;
use elysiandb::httprouter::Router;
use elysiandb::log;
use elysiandb::metrics;
use elysiandb::routing;
use elysiandb::security;
use elysiandb::stat;
use elysiandb::storage;
use elysiandb::transport::http::api;
use elysiandb::transport::http::{adminui, observe};

use super::{Case, LogBuffer, Running, load_case, new_http_server_config, read_response};
use crate::common;
use crate::normalize::Normalizer;
use crate::tempdir::TempDir;

pub const SEEDED: [&str; 2] = ["off", "deadline"];

pub const SERVER_LEVEL: [(&str, &str); 11] = [
    ("off", "098-garbage-request-line"),
    ("off", "099-garbage-bytes"),
    ("off", "100-duplicate-content-length"),
    ("off", "102-body-4mib-plus-one"),
    ("off", "104-body-4mib-plus-one-chunked"),
    ("off", "106-head-70kib"),
    ("timeouts", "001-header-read-timeout"),
    ("timeouts", "002-body-read-timeout"),
    ("timeouts", "004-silent-connection"),
    ("limit", "001-hold-connection"),
    ("limit", "002-concurrency-503"),
];

pub const EXCLUDED: [(&str, &str, &str); 12] = [
    ("off", "061-monitoring-overview", MONITORING),
    ("off", "062-monitoring-series", MONITORING),
    ("off", "063-monitoring-details", MONITORING),
    ("off", "064-monitoring-logs", MONITORING),
    ("basic", "065-monitoring-overview", MONITORING),
    ("basic", "066-monitoring-series", MONITORING),
    ("basic", "067-monitoring-details", MONITORING),
    ("basic", "068-monitoring-logs", MONITORING),
    ("disk-full", "001-write-refused", REFUSED_WRITES),
    ("disk-full", "002-kv-refused", REFUSED_WRITES),
    ("disk-full", "003-delete-allowed", REFUSED_WRITES),
    ("disk-full", "004-read-allowed", REFUSED_WRITES),
];

const MONITORING: &str = "monitoring sources and sampler of boot";

const REFUSED_WRITES: &str = "refuse_writes_without_disk of boot";

const ADMIN_UI_CASES: [&str; 6] = [
    "090-admin-redirect",
    "091-admin-root",
    "092-admin-gitkeep",
    "093-admin-spa",
    "094-admin-asset-missing",
    "095-admin-dotdot-panic",
];

const D17_RENAMES: [(&str, &str); 5] = [
    ("go_version", "rust_version"),
    ("goroutines", "threads"),
    ("gomaxprocs", "usable_cpus"),
    ("heap_bytes", "allocated_bytes"),
    ("heap_live_bytes", "memory_live_bytes"),
];

const D17_REMOVED: [&str; 2] = ["gc_pause_p99_ms", "gc_cycles_rps"];

const D17_PPROF: &str = ",\"Pprof\":{\"Enabled\":false}";

static INDEX: Once = Once::new();

struct Booted {
    data: TempDir,
    _conf: TempDir,
    logs: LogBuffer,
    running: Running,
}

impl Drop for Booted {
    fn drop(&mut self) {
        let _ = self
            .running
            .server
            .shutdown_with_timeout(Duration::from_secs(5));
        let _ = storage::close();
        log::set_output(None, false);
        let _ = log::configure(&log::Options::default());
        metrics::reset();
    }
}

fn new_router() -> Router {
    let mut r = Router::new();
    r.save_matched_route_path = true;
    r.panic_handler = Some(observe::panic_handler());
    routing::register_routes(&mut r);
    r
}

fn boot(scenario: &str) -> Booted {
    INDEX.call_once(api_storage::index::register);
    let data = TempDir::new();
    let conf = TempDir::new();
    let folder = data.path().to_string_lossy().into_owned();
    let yaml = String::from_utf8_lossy(&common::read_golden(&format!(
        "http/{scenario}/config.yaml"
    )))
    .replace("<DATA>", &folder);
    let path = conf.path().join("config.yaml");
    if let Err(e) = std::fs::write(&path, yaml) {
        panic!("{scenario}: {e}");
    }
    let cfg = match configuration::load_config(&path) {
        Ok(cfg) => cfg,
        Err(e) => panic!("{scenario}: {e}"),
    };
    globals::set_config(Some(Arc::new(cfg.clone())));

    let logs = LogBuffer::default();
    if let Err(e) = log::configure(&log::Options {
        level: cfg.log.level.clone(),
        format: cfg.log.format.clone(),
        buffer_lines: cfg.log.buffer_lines,
    }) {
        panic!("{scenario}: {e}");
    }
    log::set_output(Some(Box::new(logs.clone())), false);
    metrics::reset();
    observe::reset_throttles();
    observe::install_panic_hook();
    globals::set_starting(false);
    security::reset_login_attempts();

    if cfg.api.cache.enabled {
        cache::init_cache(GoDuration(
            cfg.api.cache.cleanup_interval_seconds * GoDuration::SECOND.0,
        ));
    }
    if let Err(e) = storage::open(storage::MODE_SERVER) {
        panic!("{scenario}: {e}");
    }
    let _ = acl::init_acl();
    hook::init_hooks();
    let _ = security::init_admin_user_if_not_exists();
    security::protect_sessions_file();
    if cfg.stats.enabled {
        stat::init();
    }

    let chain = api::close_after_unread_body(observe::middleware(security::protect(
        new_router().into_handler(),
    )));
    let mut server_cfg = new_http_server_config(100_000);
    server_cfg.handler = chain;
    server_cfg.header_received = Some(api::streaming_routes);
    let running = Running::start(server_cfg);

    Booted {
        data,
        _conf: conf,
        logs,
        running,
    }
}

fn post_json(addr: SocketAddr, path: &str, body: &str) {
    let mut conn = TcpStream::connect(addr).unwrap_or_else(|e| panic!("seed {path}: {e}"));
    let req = format!(
        "POST {path} HTTP/1.1\r\nHost: h\r\nConnection: close\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{body}",
        body.len()
    );
    let _ = conn.write_all(req.as_bytes());
    let (raw, _) = read_response(&mut conn, true, 0, 10000);
    if !raw.starts_with(b"HTTP/1.1 200 ") {
        panic!(
            "seed {path}: {}",
            String::from_utf8_lossy(&raw[..raw.len().min(200)])
        );
    }
}

fn seed_big(addr: SocketAddr) {
    let text = "lorem ipsum ".repeat(40);
    let docs: Vec<String> = (0..2600)
        .map(|i| format!(r#"{{"id":"big{i:04}","text":"{text}","n":{i}}}"#))
        .collect();
    for chunk in docs.chunks(500) {
        post_json(addr, "/api/big", &format!("[{}]", chunk.join(",")));
    }
}

fn capture(pattern: &str, raw: &[u8]) -> Option<String> {
    let open = pattern.find("([")?;
    let close = pattern[open..].find("]+)")? + open;
    let prefix = &pattern.as_bytes()[..open];
    let class = &pattern[open + 2..close];
    let suffix = &pattern.as_bytes()[close + 3..];
    let accepts = |c: u8| match class {
        "0-9a-f" => c.is_ascii_digit() || (b'a'..=b'f').contains(&c),
        "^\"" => c != b'"',
        other => panic!("unsupported capture class {other}"),
    };

    let mut from = 0;
    while let Some(i) = raw[from..].windows(prefix.len()).position(|w| w == prefix) {
        let start = from + i + prefix.len();
        let len = raw[start..].iter().take_while(|&&c| accepts(c)).count();
        if len > 0 && raw[start + len..].starts_with(suffix) {
            return Some(String::from_utf8_lossy(&raw[start..start + len]).into_owned());
        }
        from = from + i + 1;
    }
    None
}

fn exchange(case: &Case, addr: SocketAddr, captured: &BTreeMap<String, String>) -> (Vec<u8>, bool) {
    let mut conn = TcpStream::connect(addr).unwrap_or_else(|e| panic!("{}: {e}", case.id));
    for (file, pause) in &case.steps {
        if *pause > 0 {
            std::thread::sleep(Duration::from_millis(*pause));
        }
        let mut data = common::read_golden(&format!("http/{}/{file}", case.scenario));
        for (name, value) in captured {
            let placeholder = format!("{{{{{name}}}}}");
            let text = String::from_utf8_lossy(&data).into_owned();
            if text.contains(&placeholder) {
                data = text.replace(&placeholder, value).into_bytes();
            }
        }
        let _ = conn.write_all(&data);
    }
    if case.half_close {
        let _ = conn.shutdown(Shutdown::Write);
    }
    read_response(&mut conn, case.until_close, case.quiet_ms, case.max_ms)
}

fn rename_keys(v: &Value) -> Value {
    match v {
        Value::Object(m) => {
            let mut out = Map::new();
            for (k, x) in m {
                if D17_REMOVED.contains(&k.as_str()) {
                    continue;
                }
                let key = D17_RENAMES
                    .iter()
                    .find(|(from, _)| from == k)
                    .map(|(_, to)| to.to_string())
                    .unwrap_or_else(|| k.clone());
                out.insert(key, rename_keys(x));
            }
            Value::Object(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(rename_keys).collect()),
        other => other.clone(),
    }
}

fn with_d17_names(golden: &[u8]) -> Vec<u8> {
    let Some(end) = golden.windows(4).position(|w| w == b"\r\n\r\n") else {
        return golden.to_vec();
    };
    let body = &golden[end + 4..];
    let Ok(value) = json::unmarshal_value(body) else {
        return golden.to_vec();
    };
    let Ok(renamed) = json::marshal(&rename_keys(&value)) else {
        return golden.to_vec();
    };
    let mut out = golden[..end + 4].to_vec();
    out.extend_from_slice(&renamed);
    out
}

fn compare(case: &Case, norm: &mut Normalizer, raw: &[u8], closed: bool, logs: &str) {
    let mut want = common::read_golden(&format!("http/{}/{}.resp", case.scenario, case.id));
    if case.compare == "json-shape" {
        want = with_d17_names(&want);
    }
    let text = String::from_utf8_lossy(&want).into_owned();
    if text.contains(D17_PPROF) {
        want = text.replace(D17_PPROF, "").into_bytes();
    }
    let got = norm.normalize(raw, case.head, &case.compare);
    assert_eq!(
        String::from_utf8_lossy(&got),
        String::from_utf8_lossy(&want),
        "{}/{}: normalized response differs",
        case.scenario,
        case.id
    );
    assert_eq!(
        closed, case.closed,
        "{}/{}: closed flag",
        case.scenario, case.id
    );
    let logged = logs.contains("msg=\"http connection error\"")
        || logs.contains("msg=\"http connection limit reached\"");
    assert_eq!(
        logged, case.logged,
        "{}/{}: logged flag, log output: {logs}",
        case.scenario, case.id
    );
    if case.compare == "d27" {
        super::check_d27(case, raw);
    }
}

fn captures_of(scenario: &str, id: &str) -> Vec<(String, String)> {
    let cases = common::read_json(&format!("http/{scenario}/cases.json"));
    let row = cases
        .as_array()
        .and_then(|list| {
            list.iter()
                .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
        })
        .unwrap_or_else(|| panic!("{scenario}/{id}: missing case"));
    match row.get("capture").and_then(Value::as_object) {
        None => Vec::new(),
        Some(m) => m
            .iter()
            .map(|(k, v)| (k.clone(), v.as_str().unwrap_or("").to_string()))
            .collect(),
    }
}

pub fn case_ids(scenario: &str) -> Vec<String> {
    let cases = common::read_json(&format!("http/{scenario}/cases.json"));
    cases
        .as_array()
        .unwrap_or_else(|| panic!("{scenario}: cases.json is not an array"))
        .iter()
        .map(|c| common::str_field(c, "id").to_string())
        .collect()
}

pub fn excluded(scenario: &str, id: &str) -> bool {
    EXCLUDED.iter().any(|(s, i, _)| *s == scenario && *i == id)
}

pub fn server_level(scenario: &str, id: &str) -> bool {
    SERVER_LEVEL.iter().any(|(s, i)| *s == scenario && *i == id)
}

pub struct Replay {
    pub replayed: usize,
    pub skipped: usize,
    pub total: usize,
}

pub fn send_raw(scenario: &str, requests: &[&[u8]]) -> Vec<Vec<u8>> {
    let booted = boot(scenario);
    let mut out = Vec::new();
    for req in requests {
        let mut conn = TcpStream::connect(booted.running.addr).unwrap_or_else(|e| panic!("{e}"));
        let _ = conn.write_all(req);
        let _ = conn.shutdown(Shutdown::Write);
        let (raw, _) = read_response(&mut conn, true, 0, 10000);
        out.push(raw);
    }
    out
}

pub fn replay(scenario: &str) -> Replay {
    let ids = case_ids(scenario);
    let booted = boot(scenario);
    if SEEDED.contains(&scenario) {
        seed_big(booted.running.addr);
    }
    let folder = booted.data.path().to_string_lossy().into_owned();
    let mut norm = Normalizer::new(&folder, &std::env::temp_dir().to_string_lossy());
    let mut captured: BTreeMap<String, String> = BTreeMap::new();
    let built = adminui::is_built();
    let mut replayed = 0;
    let mut skipped = 0;

    for id in &ids {
        if excluded(scenario, id)
            || server_level(scenario, id)
            || (built && ADMIN_UI_CASES.contains(&id.as_str()))
        {
            skipped += 1;
            continue;
        }
        let case = load_case(scenario, id);
        let mark = booted.logs.len();
        let (raw, closed) = exchange(&case, booted.running.addr, &captured);
        std::thread::sleep(Duration::from_millis(20));
        compare(&case, &mut norm, &raw, closed, &booted.logs.since(mark));
        for (name, pattern) in captures_of(scenario, id) {
            let value = capture(&pattern, &raw)
                .unwrap_or_else(|| panic!("{scenario}/{id}: nothing captured for {name}"));
            captured.insert(name, value);
        }
        replayed += 1;
    }

    Replay {
        replayed,
        skipped,
        total: ids.len(),
    }
}
