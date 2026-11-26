#[path = "../common/mod.rs"]
mod common;
#[path = "../internal/tempdir.rs"]
mod tempdir;

mod d27;
mod normalize;
mod routed;

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use elysiandb::gocompat::json::Value;
use elysiandb::httpd::{RequestCtx, Server, ServerConfig, bind_tcp4, handler};
use elysiandb::log;
use elysiandb::sync::Mutex;

#[derive(Clone, Default)]
struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    fn len(&self) -> usize {
        self.0.lock().len()
    }

    fn since(&self, start: usize) -> String {
        let b = self.0.lock();
        String::from_utf8_lossy(&b[start.min(b.len())..]).into_owned()
    }
}

fn minimal_handler(ctx: &mut RequestCtx) {
    ctx.set_status_code(200);
}

fn new_http_server_config(concurrency: usize) -> ServerConfig {
    let mut cfg = ServerConfig::new(handler(minimal_handler));
    cfg.name = "ElysianDB".to_string();
    cfg.concurrency = concurrency;
    cfg.read_buffer_size = 64 << 10;
    cfg.write_buffer_size = 64 << 10;
    cfg.read_timeout = Duration::from_secs(3);
    cfg.write_timeout = Duration::from_secs(3);
    cfg.idle_timeout = Duration::from_secs(30);
    cfg
}

struct Running {
    server: Server,
    addr: SocketAddr,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl Running {
    fn start(cfg: ServerConfig) -> Running {
        let ln = bind_tcp4("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
        let addr = ln.local_addr().unwrap_or_else(|e| panic!("{e}"));
        let server = Server::new(cfg);
        let s = server.clone();
        let thread = Some(std::thread::spawn(move || s.serve(ln)));
        Running {
            server,
            addr,
            thread,
        }
    }
}

impl Drop for Running {
    fn drop(&mut self) {
        let _ = self.server.shutdown_with_timeout(Duration::from_secs(5));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

struct Case {
    scenario: String,
    id: String,
    steps: Vec<(String, u64)>,
    half_close: bool,
    until_close: bool,
    quiet_ms: u64,
    max_ms: u64,
    closed: bool,
    logged: bool,
    compare: String,
    head: bool,
    server: String,
}

fn load_case(scenario: &str, id: &str) -> Case {
    let cases = common::read_json(&format!("http/{scenario}/cases.json"));
    let list = cases
        .as_array()
        .unwrap_or_else(|| panic!("{scenario}: cases.json is not an array"));
    let row = list
        .iter()
        .find(|c| c.get("id").and_then(Value::as_str) == Some(id))
        .unwrap_or_else(|| panic!("{scenario}/{id}: missing case"));
    let read = row
        .get("read")
        .unwrap_or_else(|| panic!("{id}: missing read"));
    let steps = row
        .get("steps")
        .and_then(Value::as_array)
        .map(|steps| {
            steps
                .iter()
                .map(|s| {
                    (
                        common::str_field(s, "file").to_string(),
                        s.get("pause_before_ms")
                            .and_then(Value::as_i64)
                            .unwrap_or(0) as u64,
                    )
                })
                .collect()
        })
        .unwrap_or_default();
    let flag = |k: &str| row.get(k).and_then(Value::as_bool).unwrap_or(false);
    Case {
        scenario: scenario.to_string(),
        id: id.to_string(),
        steps,
        half_close: flag("half_close"),
        until_close: read.get("until").and_then(Value::as_str) == Some("close"),
        quiet_ms: read.get("quiet_ms").and_then(Value::as_i64).unwrap_or(800) as u64,
        max_ms: read.get("max_ms").and_then(Value::as_i64).unwrap_or(10000) as u64,
        closed: flag("closed"),
        logged: flag("logged"),
        compare: common::str_field(row, "compare").to_string(),
        head: flag("head"),
        server: row
            .get("server")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    }
}

fn read_response(
    conn: &mut TcpStream,
    until_close: bool,
    quiet_ms: u64,
    max_ms: u64,
) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(max_ms);
    let quiet = Duration::from_millis(if quiet_ms == 0 { 800 } else { quiet_ms });
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return (out, false);
        }
        let mut wait = deadline - now;
        if !until_close && quiet < wait {
            wait = quiet;
        }
        let _ = conn.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
        match conn.read(&mut buf) {
            Ok(0) => return (out, true),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if !until_close {
                    return (out, false);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return (out, true),
        }
    }
}

fn exchange(case: &Case, addr: SocketAddr) -> (TcpStream, Vec<u8>, bool) {
    let mut conn = TcpStream::connect(addr).unwrap_or_else(|e| panic!("{}: {e}", case.id));
    for (file, pause) in &case.steps {
        if *pause > 0 {
            std::thread::sleep(Duration::from_millis(*pause));
        }
        let data = common::read_golden(&format!("http/{}/{file}", case.scenario));
        let _ = conn.write_all(&data);
    }
    if case.half_close {
        let _ = conn.shutdown(Shutdown::Write);
    }
    let (raw, closed) = read_response(&mut conn, case.until_close, case.quiet_ms, case.max_ms);
    (conn, raw, closed)
}

fn body_of(raw: &[u8]) -> Vec<u8> {
    match raw.windows(4).position(|w| w == b"\r\n\r\n") {
        Some(i) => raw[i + 4..].to_vec(),
        None => Vec::new(),
    }
}

fn content_length(raw: &[u8]) -> Option<usize> {
    let head = String::from_utf8_lossy(raw).into_owned();
    head.split("\r\n")
        .find_map(|l| l.strip_prefix("Content-Length: "))
        .and_then(|v| v.parse().ok())
}

fn check(case: &Case, raw: &[u8], closed: bool, logs: &str) {
    let want = common::read_golden(&format!("http/{}/{}.resp", case.scenario, case.id));
    let mut norm =
        normalize::Normalizer::new("<no data folder>", &std::env::temp_dir().to_string_lossy());
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
    if let Some(reason) = d27::log_reason_for(&case.scenario, &case.id) {
        assert!(
            logs.contains(&format!("error=\"{reason}\"")),
            "{}/{}: {logs}",
            case.scenario,
            case.id
        );
        assert!(
            logs.contains("component=http"),
            "{}/{}: {logs}",
            case.scenario,
            case.id
        );
    }

    if case.compare == "d27" {
        check_d27(case, raw);
    }
}

fn check_d27(case: &Case, raw: &[u8]) {
    let msg = d27::message_for(&case.scenario, &case.id)
        .unwrap_or_else(|| panic!("{}/{}: no D27 message", case.scenario, case.id));
    let body = body_of(raw);
    assert_eq!(
        String::from_utf8_lossy(&body),
        msg,
        "{}/{}: D27 body",
        case.scenario,
        case.id
    );
    assert_eq!(
        content_length(raw),
        Some(msg.len()),
        "{}/{}: Content-Length",
        case.scenario,
        case.id
    );
}

#[test]
fn test_server_level_cases_replay_the_go_corpus() {
    let buf = LogBuffer::default();
    let _ = log::configure(&log::Options::default());
    log::set_output(Some(Box::new(buf.clone())), false);

    let main = Running::start(new_http_server_config(100_000));
    let limited = Running::start(new_http_server_config(1));

    let silent = load_case("timeouts", "004-silent-connection");
    let silent_addr = main.addr;
    let silent_run = std::thread::spawn(move || {
        let (_conn, raw, closed) = exchange(&silent, silent_addr);
        (silent, raw, closed)
    });

    let hold = load_case("limit", "001-hold-connection");
    let (held_conn, _, held_closed) = exchange(&hold, limited.addr);
    assert!(!held_closed);

    let cases: Vec<(&str, &str)> = routed::SERVER_LEVEL
        .iter()
        .copied()
        .filter(|(_, id)| *id != "001-hold-connection")
        .collect();
    let mut replayed = 0;
    for &(scenario, id) in &cases {
        if id == "004-silent-connection" {
            continue;
        }
        let case = load_case(scenario, id);
        let addr = if case.server == "limited" {
            limited.addr
        } else {
            main.addr
        };
        let mark = buf.len();
        let (_conn, raw, closed) = exchange(&case, addr);
        std::thread::sleep(Duration::from_millis(50));
        check(&case, &raw, closed, &buf.since(mark));
        replayed += 1;
    }
    drop(held_conn);

    let (case, raw, closed) = silent_run
        .join()
        .unwrap_or_else(|_| panic!("silent connection thread panicked"));
    check(&case, &raw, closed, "");
    replayed += 1;

    log::set_output(None, false);
    assert_eq!(replayed + 1, routed::SERVER_LEVEL.len());
}

#[test]
fn test_normalizer_matches_the_generator_rules() {
    let mut n = normalize::Normalizer::new("/data/x", "/tmp");
    let raw = b"HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: Mon, 23 Nov 2026 12:38:06 GMT\r\nContent-Type: application/json\r\nContent-Length: 157\r\nX-Request-Id: 0a1b2c3d-1z\r\nX-Elysian-Version: golden\r\nSet-Cookie: edb_session=deadbeef; path=/\r\n\r\n{\"id\":\"123e4567-e89b-12d3-a456-426614174000\",\"tx\":\"20261123133203.346387000\",\"at\":\"2026-11-23T12:38:06.123+01:00\",\"p\":\"/data/x/f\",\"version\":\"1.2\"}";
    let got = n.normalize(raw, false, "exact");
    let want = "HTTP/1.1 200 OK\r\nServer: ElysianDB\r\nDate: <DATE>\r\nContent-Type: application/json\r\nContent-Length: <LEN>\r\nX-Request-Id: <RID>\r\nX-Elysian-Version: <VER>\r\nSet-Cookie: edb_session=<SID>; path=/\r\n\r\n{\"id\":\"<UUID:1>\",\"tx\":\"<TXID:1>\",\"at\":\"<TIME>\",\"p\":\"<DATA>/f\",\"version\":\"<VER>\"}";
    assert_eq!(String::from_utf8_lossy(&got), want);

    let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n3\r\n{\"a\r\n4\r\n\":1}\r\n0\r\n\r\n";
    let got = n.normalize(chunked, false, "json-shape");
    assert_eq!(
        String::from_utf8_lossy(&got),
        "HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n<CHUNKED complete>\r\n{\"a\":\"\\u003cnumber\\u003e\"}"
    );
    assert!(normalize::is_request_id("0a1b2c3d-1z"));
    assert!(!normalize::is_request_id("client-supplied-1"));
}

const SCENARIOS: [&str; 10] = [
    "off",
    "user",
    "basic",
    "token",
    "strict",
    "cors",
    "disk-full",
    "deadline",
    "limit",
    "timeouts",
];

fn run_routed(scenario: &str) {
    let r = routed::replay(scenario);
    assert_eq!(r.replayed + r.skipped, r.total, "{scenario}");
    assert!(r.replayed > 0, "{scenario}: nothing replayed");
}

#[test]
fn test_every_case_is_replayed_or_listed() {
    let mut total = 0;
    let mut listed = 0;
    for scenario in SCENARIOS {
        let ids = routed::case_ids(scenario);
        total += ids.len();
        for id in &ids {
            if routed::excluded(scenario, id) || routed::server_level(scenario, id) {
                listed += 1;
            }
        }
    }
    for (scenario, id, _) in routed::EXCLUDED {
        assert!(
            routed::case_ids(scenario).iter().any(|i| i == id),
            "{scenario}/{id}: excluded case does not exist"
        );
    }
    assert_eq!(total, 268);
    assert_eq!(listed, routed::EXCLUDED.len() + routed::SERVER_LEVEL.len());
}

#[test]
fn test_routed_off_cases_replay_the_go_corpus() {
    run_routed("off");
}

#[test]
fn test_routed_basic_cases_replay_the_go_corpus() {
    run_routed("basic");
}

#[test]
fn test_routed_token_cases_replay_the_go_corpus() {
    run_routed("token");
}

#[test]
fn test_routed_user_cases_replay_the_go_corpus() {
    run_routed("user");
}

#[test]
fn test_routed_cors_cases_replay_the_go_corpus() {
    run_routed("cors");
}

#[test]
fn test_routed_deadline_cases_replay_the_go_corpus() {
    run_routed("deadline");
}

#[test]
fn test_routed_strict_cases_replay_the_go_corpus() {
    run_routed("strict");
}

#[test]
fn test_routed_timeouts_cases_replay_the_go_corpus() {
    run_routed("timeouts");
}

#[test]
fn test_streamed_restore_body_errors_use_the_d27_messages() {
    let head = "POST /api/restore HTTP/1.1\r\nHost: h\r\nContent-Type: application/gzip\r\n";
    let cut = format!("{head}Content-Length: 100\r\n\r\nshort");
    let bad_chunk = format!("{head}Transfer-Encoding: chunked\r\n\r\nzz\r\nabc\r\n0\r\n\r\n");
    let cut_chunk = format!("{head}Transfer-Encoding: chunked\r\n\r\n10\r\nabc");
    let replies = routed::send_raw(
        "off",
        &[cut.as_bytes(), bad_chunk.as_bytes(), cut_chunk.as_bytes()],
    );

    let want = [
        r#"{"error":"unexpected end of the request body"}"#,
        r#"{"error":"invalid chunked body"}"#,
        r#"{"error":"unexpected end of the request body"}"#,
    ];
    for (raw, want) in replies.iter().zip(want) {
        let text = String::from_utf8_lossy(raw).into_owned();
        assert!(
            text.starts_with("HTTP/1.1 500 Internal Server Error\r\n"),
            "{text}"
        );
        assert!(text.contains("\r\nConnection: close\r\n"), "{text}");
        assert_eq!(String::from_utf8_lossy(&body_of(raw)), want, "{text}");
    }
}

fn request(method: &str, path: &[u8], body: &str) -> Vec<u8> {
    let mut out = method.as_bytes().to_vec();
    out.push(b' ');
    out.extend_from_slice(path);
    out.extend_from_slice(b" HTTP/1.1\r\nHost: h\r\nConnection: close\r\n");
    if !body.is_empty() || method != "GET" {
        out.extend_from_slice(
            format!(
                "Content-Type: application/json\r\nContent-Length: {}\r\n",
                body.len()
            )
            .as_bytes(),
        );
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body.as_bytes());
    out
}

fn status_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw).into_owned();
    text.split("\r\n").next().unwrap_or_default().to_string()
}

#[test]
fn test_import_with_a_null_document_stops_like_go() {
    let requests = [
        request("POST", b"/api/b", r#"{"id":"b1"}"#),
        request("POST", b"/api/import", r#"{"a":[{"id":"x"}],"b":[null]}"#),
        request("GET", b"/api/a", ""),
        request("GET", b"/api/b", ""),
        request("POST", b"/api/c", r#"{"id":"c1"}"#),
        request("GET", b"/api/c/c1", ""),
    ];
    let refs: Vec<&[u8]> = requests.iter().map(Vec::as_slice).collect();
    let (replies, logs) = routed::send_raw_with_logs("off", &refs);

    assert_eq!(status_line(&replies[0]), "HTTP/1.1 200 OK");

    let import = String::from_utf8_lossy(&replies[1]).into_owned();
    assert_eq!(
        status_line(&replies[1]),
        "HTTP/1.1 500 Internal Server Error"
    );
    assert!(
        import.contains("\r\nContent-Type: application/json\r\n"),
        "{import}"
    );
    assert!(import.contains("\r\nContent-Length: 0\r\n"), "{import}");
    assert!(!import.contains("X-Elysian-Version"), "{import}");
    assert!(body_of(&replies[1]).is_empty(), "{import}");
    assert!(
        logs.contains("msg=\"handler panic\"")
            && logs.contains("panic=\"cannot write a null document\""),
        "{logs}"
    );

    assert_eq!(
        String::from_utf8_lossy(&body_of(&replies[2])),
        r#"[{"id":"x"}]"#
    );
    assert_eq!(String::from_utf8_lossy(&body_of(&replies[3])), "[]");
    assert_eq!(status_line(&replies[4]), "HTTP/1.1 200 OK");
    assert_eq!(
        String::from_utf8_lossy(&body_of(&replies[5])),
        r#"{"id":"c1"}"#
    );
}

#[test]
fn test_raw_entity_names_reach_the_wire_like_go() {
    let fields = r#"{"fields":{"t":{"type":"string"}}}"#;
    let requests = [
        request("POST", b"/api/\xfe/create", fields),
        request("POST", b"/api/\xfe/create", fields),
        request("PUT", b"/api/\xfe/schema", fields),
        request("GET", b"/api/entity/types/name", ""),
        request("POST", b"/api/\xfe/migrate", r#"[{"set":{"x":1}}]"#),
        request("POST", b"/api/\xfd/migrate", r#"[{"set":{"x":1}}]"#),
    ];
    let refs: Vec<&[u8]> = requests.iter().map(Vec::as_slice).collect();
    let replies = routed::send_raw("off", &refs);

    let schema = r#"{"_manual":true,"fields":{"t":{"name":"t","required":false,"type":"string"}},"id":"\ufffd"}"#;
    assert_eq!(status_line(&replies[0]), "HTTP/1.1 200 OK");
    assert_eq!(String::from_utf8_lossy(&body_of(&replies[0])), schema);

    assert_eq!(status_line(&replies[1]), "HTTP/1.1 400 Bad Request");
    assert_eq!(body_of(&replies[1]), b"entity type '\xfe' already exists");

    assert_eq!(status_line(&replies[2]), "HTTP/1.1 200 OK");
    assert_eq!(String::from_utf8_lossy(&body_of(&replies[2])), schema);

    assert_eq!(
        String::from_utf8_lossy(&body_of(&replies[3])),
        r#"{"entities":["\ufffd"]}"#
    );

    assert_eq!(
        body_of(&replies[4]),
        b"{\"message\" : \"Entity '\xfe' migrated successfully.\"}"
    );
    assert_eq!(
        body_of(&replies[5]),
        b"{\"error\" : \"Entity '\xfd' does not exist.\"}"
    );
}
