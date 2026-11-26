#[path = "compat/fixture.rs"]
mod fixture;
#[path = "compat/golden.rs"]
mod golden;
#[path = "support/http.rs"]
mod http;
#[path = "support/process.rs"]
mod process;
#[path = "internal/tempdir.rs"]
mod tempdir;

use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use elysiandb::gocompat::json::{self, Value};
use elysiandb::gocompat::rand::{Rand, new_source};
use elysiandb::security;

use fixture::{
    FixtureRequest, HOOK_QUERY_FIELDS, fixture_load_requests, hook_requests, kv_requests,
    ownership_requests, read_requests, transaction_requests, write_requests,
};
use golden::{
    Corpus, GoldenEntry, LEGACY_FIXTURES_DIR, differs, expected_body, golden_full, normalize_body,
    read_golden, read_legacy_golden, testdata, truncate,
};
use process::{
    Confirmed, Server, binary, free_port, previous_binary, skip, spawn, verify_confirmed, wait,
    write_load, write_private,
};
use tempdir::TempDir;

const NEEDS_PREVIOUS: &str =
    "needs a binary of the other implementation in ELYSIANDB_PREVIOUS_BINARY";

const LEGACY_VARIANTS: [&str; 6] = [
    "full",
    "pending",
    "kv-only",
    "empty",
    "inconsistent",
    "failed-log",
];

fn binaries() -> Option<(String, String)> {
    let previous = previous_binary()?;

    Some((binary(), previous))
}

fn speaks_go(binary: &str) -> bool {
    let rust = std::fs::canonicalize(env!("CARGO_BIN_EXE_elysiandb")).ok();
    let given = std::fs::canonicalize(binary).ok();

    rust.is_none() || rust != given
}

fn name_of(binary: &str) -> &'static str {
    if speaks_go(binary) { "go" } else { "rust" }
}

struct Settings<'a> {
    shards: usize,
    flush_seconds: usize,
    crash_recovery: bool,
    max_log_mb: usize,
    tcp: Option<u16>,
    authentication: bool,
    strict_schema: bool,
    cache: bool,
    http: bool,
    level: &'a str,
    extra: &'a str,
}

impl Settings<'_> {
    fn corpus(crash_recovery: bool) -> Settings<'static> {
        Settings {
            shards: 8,
            flush_seconds: 0,
            crash_recovery,
            max_log_mb: 100,
            tcp: None,
            authentication: true,
            strict_schema: true,
            cache: true,
            http: true,
            level: "error",
            extra: "",
        }
    }

    fn open() -> Settings<'static> {
        Settings {
            shards: 16,
            flush_seconds: 3600,
            crash_recovery: true,
            max_log_mb: 100,
            tcp: None,
            authentication: false,
            strict_schema: false,
            cache: false,
            http: true,
            level: "info",
            extra: "",
        }
    }

    fn load() -> Settings<'static> {
        Settings {
            shards: 16,
            flush_seconds: 1,
            crash_recovery: true,
            max_log_mb: 1,
            tcp: None,
            authentication: true,
            strict_schema: false,
            cache: false,
            http: true,
            level: "info",
            extra: "monitoring: { prometheus: { enabled: true } }\n",
        }
    }
}

fn write_config(folder: &Path, port: u16, s: &Settings<'_>) -> PathBuf {
    let authentication = if s.authentication {
        "{ enabled: true, mode: user }"
    } else {
        "{ enabled: false }"
    };
    let schema = if s.strict_schema {
        "{ enabled: true, strict: true }"
    } else {
        "{ enabled: false }"
    };
    let cache = if s.cache {
        "  cache: { enabled: true, cleanupIntervalSeconds: 10 }\n"
    } else {
        ""
    };
    let (tcp_enabled, tcp_port) = match s.tcp {
        Some(p) => (true, p),
        None => (false, 1),
    };
    let config = format!(
        "store:
  folder: {}
  shards: {}
  flushIntervalSeconds: {}
  crashRecovery: {{ enabled: {}, maxLogMB: {} }}
engine:
  name: internal
server:
  http: {{ enabled: {}, host: 127.0.0.1, port: {port} }}
  tcp: {{ enabled: {tcp_enabled}, host: 127.0.0.1, port: {tcp_port} }}
security:
  authentication: {authentication}
api:
  schema: {schema}
{cache}  hooks: {{ enabled: true }}
  index: {{ workers: 1 }}
{}log: {{ level: {}, format: text }}
",
        folder.display(),
        s.shards,
        s.flush_seconds,
        s.crash_recovery,
        s.max_log_mb,
        s.http,
        s.extra,
        s.level,
    );

    let path = PathBuf::from(format!("{}.yaml", folder.display()));
    write_private(&path, config.as_bytes());

    path
}

fn start(binary: &str, folder: &Path, settings: &Settings<'_>) -> Arc<Server> {
    let port = free_port();
    let config = write_config(folder, port, settings);

    spawn(binary, &config, port, folder, None)
}

struct Line {
    conn: Option<TcpStream>,
    addr: Option<SocketAddr>,
}

impl Line {
    fn exchange(conn: &mut TcpStream, line: &str) -> Vec<u8> {
        conn.write_all(format!("{line}\nPING\n").as_bytes())
            .unwrap_or_else(|e| panic!("tcp {line}: {e}"));
        let ends = if line == "PING" { 2 } else { 1 };
        let mut got = Vec::new();
        let mut buf = [0u8; 4096];
        loop {
            let text = String::from_utf8_lossy(&got).into_owned();
            let complete = text.ends_with("PONG\n")
                && (text == "PONG\n" || text.ends_with("\nPONG\n"))
                && text.lines().filter(|l| *l == "PONG").count() >= ends;
            if complete {
                break;
            }
            match conn.read(&mut buf) {
                Ok(0) => panic!("tcp {line}: the connection was closed after {text:?}"),
                Ok(n) => got.extend_from_slice(&buf[..n]),
                Err(e) => panic!("tcp {line}: {e} after {text:?}"),
            }
        }
        got.truncate(got.len() - "PONG\n".len());
        if got.last() == Some(&b'\n') {
            got.pop();
        }

        got
    }

    fn send(&mut self, line: &str) -> Vec<u8> {
        if self.conn.is_none() {
            let addr = self
                .addr
                .unwrap_or_else(|| panic!("tcp {line}: the TCP server is off"));
            let mut conn = TcpStream::connect_timeout(&addr, Duration::from_secs(5))
                .unwrap_or_else(|e| panic!("tcp dial: {e}"));
            let _ = conn.set_read_timeout(Some(Duration::from_secs(10)));
            let _ = conn.set_write_timeout(Some(Duration::from_secs(10)));
            let auth = Line::exchange(
                &mut conn,
                &format!(
                    "AUTH {} {}",
                    security::DEFAULT_ADMIN_USERNAME,
                    security::DEFAULT_ADMIN_PASSWORD
                ),
            );
            if auth != b"OK" {
                panic!("tcp auth failed: {}", String::from_utf8_lossy(&auth));
            }
            self.conn = Some(conn);
        }

        match self.conn.as_mut() {
            Some(conn) => Line::exchange(conn, line),
            None => Vec::new(),
        }
    }
}

fn corpus_on(s: &Server, tcp: Option<u16>) -> Corpus {
    let addr = SocketAddr::from(([127, 0, 0, 1], s.port));
    let mut line = Line {
        conn: None,
        addr: tcp.map(|p| SocketAddr::from(([127, 0, 0, 1], p))),
    };

    let mut corpus = Corpus::new(
        addr,
        Box::new(|r: &FixtureRequest| panic!("no route opens a session for {}", r.path)),
        Box::new(move |command: &str| line.send(command)),
    );
    corpus.login(
        "admin",
        security::DEFAULT_ADMIN_USERNAME,
        security::DEFAULT_ADMIN_PASSWORD,
    );

    corpus
}

fn unbuilt_hook_indexes(corpus: &mut Corpus) -> Vec<String> {
    let answer = corpus.http("GET", "/monitoring/details", "", "");
    let details = json::unmarshal_value(&answer.body).ok();
    let indexes = details
        .as_ref()
        .and_then(|d| d.get("indexes"))
        .and_then(Value::as_array);

    HOOK_QUERY_FIELDS
        .iter()
        .filter(|(entity, field)| {
            !indexes.is_some_and(|list| {
                list.iter().any(|i| {
                    let text = |key: &str| i.get(key).and_then(Value::as_str).unwrap_or("");
                    text("entity") == *entity
                        && (text("index") == format!("f:{field}")
                            || text("index") == format!("b:{field}"))
                        && text("state") == "ready"
                })
            })
        })
        .map(|(entity, field)| format!("{entity}.{field}"))
        .collect()
}

fn wait_for_hook_indexes(s: &Server, corpus: &mut Corpus) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let unbuilt = unbuilt_hook_indexes(corpus);
        if unbuilt.is_empty() {
            return;
        }
        if Instant::now() > deadline {
            panic!(
                "the indexes of the hooks are never built: {unbuilt:?}\n{}",
                s.log.string()
            );
        }
        std::thread::sleep(Duration::from_millis(2));
    }
}

fn copy_tree(src: &Path, dst: &Path) -> std::io::Result<()> {
    let mut entries: Vec<_> = std::fs::read_dir(src)?.collect::<Result<_, _>>()?;
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let target = dst.join(e.file_name());
        if e.file_type()?.is_dir() {
            std::fs::DirBuilder::new()
                .recursive(true)
                .mode(0o755)
                .create(&target)?;
            copy_tree(&e.path(), &target)?;
            continue;
        }

        let data = std::fs::read(e.path())?;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(&target)?;
        f.write_all(&data)?;
    }

    Ok(())
}

fn copy_fixture(variant: &str, dst: &Path) {
    let src = testdata(LEGACY_FIXTURES_DIR).join(variant);
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(dst)
        .unwrap_or_else(|e| panic!("{e}"));
    if let Err(e) = copy_tree(&src, dst) {
        panic!("cannot copy {}: {e}", src.display());
    }
}

fn legacy_read_requests() -> Vec<FixtureRequest> {
    let mut reqs: Vec<FixtureRequest> = [
        "/kv/session:1",
        "/kv/derived:total",
        "/kv/long",
        "/kv/pending:key",
        "/kv/mget?keys=session:1,long,derived:total,pending:key",
        "/api/hook/budget_month",
        "/api/acl/reader/note",
        "/api/note",
        "/api/pending",
        "/api/pending/count",
    ]
    .iter()
    .map(|p| FixtureRequest::new("GET", p, ""))
    .collect();
    reqs.extend(read_requests());

    reqs
}

fn compare_reads(
    errors: &mut Vec<String>,
    label: &str,
    binary: &str,
    s: &Server,
    want: &[GoldenEntry],
) {
    let go = speaks_go(binary);
    let mut corpus = corpus_on(s, None);
    let reqs = legacy_read_requests();
    if reqs.len() != want.len() {
        panic!(
            "{label}: {} requests, golden has {}",
            reqs.len(),
            want.len()
        );
    }

    let mut failures = 0;
    for (i, r) in reqs.iter().enumerate() {
        let got = corpus.run(r);
        if differs(&got, &want[i], go) {
            failures += 1;
            if failures <= 10 {
                errors.push(format!(
                    "{label} #{i} {} {}\nwant {} {}\ngot  {} {}",
                    r.method,
                    r.path,
                    want[i].status,
                    truncate(expected_body(&want[i], go)),
                    got.status,
                    truncate(&got.body)
                ));
            }
        }
    }
}

fn files_of(folder: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let entries = std::fs::read_dir(dir).unwrap_or_else(|e| panic!("{}: {e}", dir.display()));
        for e in entries.filter_map(|e| e.ok()) {
            let path = e.path();
            if e.file_type().is_ok_and(|t| t.is_dir()) {
                walk(root, &path, out);
                continue;
            }
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .into_owned();
            out.insert(
                rel,
                std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display())),
            );
        }
    }

    let mut out = BTreeMap::new();
    walk(folder, folder, &mut out);

    out
}

fn without_creation_time(manifest: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    for line in manifest.split_inclusive(|&c| c == b'\n') {
        if line.starts_with(b"  \"created_at\": \"") {
            out.extend_from_slice(b"  \"created_at\": \"<time>\",\n");
        } else {
            out.extend_from_slice(line);
        }
    }

    out
}

fn without_password_hashes(payload: &[u8]) -> Vec<u8> {
    let mut out = payload.to_vec();
    let mut i = 0;
    while i + 60 <= out.len() {
        let head = &out[i..i + 7];
        if head.starts_with(b"$2a$")
            && head[4].is_ascii_digit()
            && head[5].is_ascii_digit()
            && head[6] == b'$'
        {
            out[i + 7..i + 60].fill(b'.');
            i += 60;
        } else {
            i += 1;
        }
    }

    out
}

fn u32_at(b: &[u8], at: usize) -> usize {
    u32::from_le_bytes([b[at], b[at + 1], b[at + 2], b[at + 3]]) as usize
}

fn batches(log: &[u8]) -> (Vec<&[u8]>, &[u8]) {
    let mut out = Vec::new();
    let mut at = 16;
    while at + 16 <= log.len() {
        let end = at + 16 + u32_at(log, at + 4);
        if &log[at..at + 4] != b"ELB1" || end > log.len() {
            break;
        }
        out.push(&log[at..end]);
        at = end;
    }

    (out, &log[at..])
}

fn same_batch(a: &[u8], b: &[u8]) -> bool {
    a == b
        || (a.len() == b.len()
            && a[..8] == b[..8]
            && a[12..16] == b[12..16]
            && without_password_hashes(&a[16..]) == without_password_hashes(&b[16..]))
}

fn key_writes(batches: &[&[u8]]) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::new();
    for batch in batches {
        if u32_at(batch, 12) != 1 || batch.len() < 25 || batch[16] != 7 {
            return Err("a batch that is not a key write differs".to_string());
        }
        let mut record = batch[16..].to_vec();
        record[1..9].fill(0);
        out.push(record);
    }
    out.sort();

    Ok(out)
}

fn same_log(a: &[u8], b: &[u8], keys_in_any_order: bool) -> Result<(), String> {
    if a.len() != b.len() {
        return Err(format!("{} bytes and {} bytes", a.len(), b.len()));
    }
    if a.len() < 16 || a[..16] != b[..16] {
        return Err("the segment headers differ".to_string());
    }

    let ((ba, rest_a), (bb, rest_b)) = (batches(a), batches(b));
    if ba.len() != bb.len() || rest_a != rest_b {
        return Err(format!(
            "{} batches then {} bytes, and {} batches then {} bytes",
            ba.len(),
            rest_a.len(),
            bb.len(),
            rest_b.len()
        ));
    }

    let same = ba
        .iter()
        .zip(bb.iter())
        .take_while(|(x, y)| same_batch(x, y))
        .count();
    if same == ba.len() {
        return Ok(());
    }
    if !keys_in_any_order {
        return Err(format!("batch {same} of {} differs", ba.len()));
    }
    if key_writes(&ba[same..])? != key_writes(&bb[same..])? {
        return Err(format!("the key writes after batch {same} differ"));
    }

    Ok(())
}

fn compare_folders(
    errors: &mut Vec<String>,
    label: &str,
    a: (&str, &Path),
    b: (&str, &Path),
    keys_in_any_order: bool,
) {
    let (fa, fb) = (files_of(a.1), files_of(b.1));

    let names_a: Vec<&String> = fa.keys().collect();
    let names_b: Vec<&String> = fb.keys().collect();
    if names_a != names_b {
        errors.push(format!(
            "{label}: the files differ\n{}: {names_a:?}\n{}: {names_b:?}",
            a.0, b.0
        ));
        return;
    }

    for (name, da) in &fa {
        let db = &fb[name];
        let base = name.rsplit('/').next().unwrap_or(name);
        let same = if base == "users.key" || (keys_in_any_order && base.starts_with("kv-")) {
            if da.len() == db.len() {
                Ok(())
            } else {
                Err(format!("{} bytes and {} bytes", da.len(), db.len()))
            }
        } else if base == "sessions.json" {
            if session_owners(da) == session_owners(db) {
                Ok(())
            } else {
                Err("the sessions differ".to_string())
            }
        } else if base == "MANIFEST" || base == "MANIFEST.prev" {
            if without_creation_time(da) == without_creation_time(db) {
                Ok(())
            } else {
                Err(format!(
                    "\n{}\n{}",
                    String::from_utf8_lossy(da),
                    String::from_utf8_lossy(db)
                ))
            }
        } else if name.starts_with("log/") && base.ends_with(".seg") {
            same_log(da, db, keys_in_any_order)
        } else if da == db {
            Ok(())
        } else {
            let first = da
                .iter()
                .zip(db.iter())
                .position(|(x, y)| x != y)
                .unwrap_or(da.len().min(db.len()));
            Err(format!(
                "{} bytes and {} bytes, first difference at {first}",
                da.len(),
                db.len()
            ))
        };
        if let Err(why) = same {
            errors.push(format!(
                "{label}: {name} written by {} and by {} differ: {why}",
                a.0, b.0
            ));
        }
    }
}

fn session_owners(sessions: &[u8]) -> Vec<(String, String)> {
    let file = json::unmarshal_value(sessions).unwrap_or_else(|e| panic!("sessions.json: {e}"));
    file.get("sessions")
        .and_then(Value::as_array)
        .map(|sessions| {
            sessions
                .iter()
                .map(|s| {
                    let text =
                        |key: &str| s.get(key).and_then(Value::as_str).unwrap_or("").to_string();
                    (text("username"), text("role"))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn type_names(body: &str) -> Vec<String> {
    let mut names: Vec<String> = json::unmarshal_value(body.as_bytes())
        .ok()
        .and_then(|v| v.get("value").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_default()
        .split('\n')
        .map(str::to_string)
        .collect();
    names.sort();

    names
}

fn fail_on(errors: Vec<String>) {
    if !errors.is_empty() {
        panic!("{}", errors.join("\n"));
    }
}

fn compact(docs: &[String]) -> String {
    format!("[{}]", docs.join(","))
}

fn deterministic_phase(s: &Server, phase: usize) -> Vec<(u16, String)> {
    let mut out = Vec::new();
    let mut tx = String::new();
    let mut run = |method: &str, path: &str, body: &str| {
        let path = path.replace("{tx}", &tx);
        let (status, answer) = s.fetch(method, &path, body, "deterministic scenario");
        if path == "/api/tx/begin" {
            tx = json::unmarshal_value(&answer)
                .ok()
                .and_then(|v| {
                    v.get("transaction_id")
                        .and_then(Value::as_str)
                        .map(str::to_string)
                })
                .unwrap_or_default();
        }
        out.push((status, normalize_body(&FixtureRequest::default(), &answer)));
    };

    if phase == 0 {
        let docs: Vec<String> = (0..400)
            .map(|i| {
                format!(
                    r#"{{"id":"a{i:03}","tag":"t{}","n":{i},"pad":"{}"}}"#,
                    i % 5,
                    "x".repeat(50 + i % 7)
                )
            })
            .collect();
        run("POST", "/api/alpha", &compact(&docs));
        for i in 0..30 {
            run(
                "POST",
                "/api/beta",
                &format!(
                    r#"{{"id":"b{i}","ref":{{"@entity":"alpha","id":"a{i:03}"}},"v":{}.5}}"#,
                    i
                ),
            );
        }
        for i in (0..60).step_by(3) {
            run(
                "PUT",
                &format!("/api/alpha/a{i:03}"),
                &format!(r#"{{"tag":"changed","n":-{i}}}"#),
            );
        }
        for i in (1..60).step_by(7) {
            run("DELETE", &format!("/api/alpha/a{i:03}"), "");
        }
        for i in 0..20 {
            run("PUT", &format!("/kv/key:{i}"), &format!("value {i}"));
        }
        run("DELETE", "/kv/key:3", "");
        run(
            "GET",
            "/api/alpha?filter[tag][eq]=t1&sort[n]=desc&limit=5",
            "",
        );
        run("GET", "/api/beta?filter[ref.tag][eq]=t2&includes=ref", "");
        run("POST", "/save", "");
        return out;
    }

    let docs: Vec<String> = (0..150)
        .map(|i| format!(r#"{{"id":"c{phase}-{i:03}","phase":{phase},"n":{i}}}"#))
        .collect();
    run("POST", "/api/gamma", &compact(&docs));
    for i in (0..100).step_by(9) {
        run(
            "PUT",
            &format!("/api/alpha/a{:03}", 100 + i),
            &format!(r#"{{"tag":"p{phase}"}}"#),
        );
    }
    run("DELETE", &format!("/api/beta/b{}", 3 + phase), "");
    run("PUT", "/kv/phase", &phase.to_string());
    run("POST", "/api/tx/begin", "");
    run(
        "POST",
        "/api/tx/{tx}/entity/delta",
        &format!(r#"{{"id":"d{phase}","phase":{phase}}}"#),
    );
    run(
        "PUT",
        "/api/tx/{tx}/entity/alpha/a200",
        &format!(r#"{{"tag":"tx{phase}"}}"#),
    );
    run(
        "DELETE",
        &format!("/api/tx/{{tx}}/entity/alpha/a{}", 300 + phase),
        "",
    );
    run("POST", "/api/tx/{tx}/commit", "");
    if phase == 2 {
        run("DELETE", "/api/gamma", "");
        run(
            "POST",
            "/api/gamma",
            r#"[{"id":"again","n":1},{"id":"more","n":2}]"#,
        );
    }
    run(
        "GET",
        "/api/alpha?filter[tag][eq]=changed&countOnly=true",
        "",
    );
    run("GET", "/api/gamma?sort[n]=desc&limit=3", "");
    if phase % 2 == 1 {
        run("POST", "/save", "");
    }

    out
}

#[test]
fn a_deterministic_scenario_writes_the_same_bytes_with_both_binaries() {
    let Some((current, previous)) = binaries() else {
        skip(NEEDS_PREVIOUS);
        return;
    };

    let mut errors = Vec::new();
    for stops in [
        [false, false, false],
        [true, true, false],
        [true, false, true],
    ] {
        let label = format!("stops {stops:?}");
        let dir = TempDir::new();
        let mut written = Vec::new();
        for (side, binary) in [&current, &previous].into_iter().enumerate() {
            let folder = dir.path().join(format!("{side}-{}", name_of(binary)));
            let mut answers = Vec::new();
            for (phase, crash) in stops.iter().enumerate() {
                let s = start(binary, &folder, &Settings::open());
                answers.push(deterministic_phase(&s, phase));
                s.stop(*crash);
            }
            written.push((name_of(binary), folder, answers));
        }

        if written[0].2 != written[1].2 {
            errors.push(format!("{label}: the answers differ"));
        }
        compare_folders(
            &mut errors,
            &label,
            (written[0].0, &written[0].1),
            (written[1].0, &written[1].1),
            false,
        );
    }

    fail_on(errors);
}

fn corpus_sections() -> Vec<Vec<FixtureRequest>> {
    let mut load = fixture_load_requests();
    load.extend(hook_requests());

    vec![
        load,
        read_requests(),
        write_requests(),
        transaction_requests(),
        kv_requests(),
    ]
}

#[test]
fn the_golden_corpus_survives_a_change_of_binary_at_every_step() {
    let Some((current, previous)) = binaries() else {
        skip(NEEDS_PREVIOUS);
        return;
    };

    let golden = read_golden();
    let sections = corpus_sections();
    let total = sections.iter().map(Vec::len).sum::<usize>() + ownership_requests().len();
    if total != golden.len() {
        panic!(
            "corpus has {total} requests, golden file has {}",
            golden.len()
        );
    }

    let mut errors = Vec::new();
    for (first, crash) in [
        (&current, false),
        (&current, true),
        (&previous, false),
        (&previous, true),
    ] {
        let second = if first == &current {
            &previous
        } else {
            &current
        };
        let dir = TempDir::new();
        let folder = dir.path().join("data");
        let mut at = 0;
        let mut failures = 0;
        for (n, section) in sections.iter().enumerate() {
            let binary = if n % 2 == 0 { first } else { second };
            let tcp = free_port();
            let mut settings = Settings::corpus(true);
            settings.tcp = Some(tcp);
            let s = start(binary, &folder, &settings);
            let mut corpus = corpus_on(&s, Some(tcp));
            let go = speaks_go(binary);

            for r in section {
                let got = corpus.run(r);
                let want = &golden[at];
                let boot_order = r.path == "/kv/api:entity:types:list"
                    && got.status == want.status
                    && type_names(&got.body) == type_names(&want.body);
                if !boot_order && differs(&got, want, go) {
                    failures += 1;
                    if failures <= 10 {
                        errors.push(format!(
                            "{} first, crash {crash}: #{at} {} {} answered by {}\nwant {} {:?} {}\ngot  {} {:?} {}",
                            name_of(first),
                            r.method,
                            r.path,
                            name_of(binary),
                            want.status,
                            want.content_type,
                            truncate(expected_body(want, go)),
                            got.status,
                            got.content_type,
                            truncate(&got.body)
                        ));
                    }
                }
                at += 1;
            }

            if !crash {
                wait_for_hook_indexes(&s, &mut corpus);
            }
            drop(corpus);
            s.stop(crash);
        }
    }

    fail_on(errors);
}

#[test]
fn folders_of_version_024_are_migrated_alike_and_read_by_the_other_binary() {
    let Some((current, previous)) = binaries() else {
        skip(NEEDS_PREVIOUS);
        return;
    };

    let golden = read_legacy_golden();
    let mut errors = Vec::new();
    for variant in LEGACY_VARIANTS {
        let want = golden
            .get(variant)
            .unwrap_or_else(|| panic!("{variant}: no golden answers"));
        let settings = Settings::corpus(variant == "pending");
        let dir = TempDir::new();

        let mut migrated = Vec::new();
        for (side, binary) in [&current, &previous].into_iter().enumerate() {
            let folder = dir.path().join(format!("{side}-{}", name_of(binary)));
            copy_fixture(variant, &folder);
            let s = start(binary, &folder, &settings);
            compare_reads(
                &mut errors,
                &format!("{variant} migrated and read by {}", name_of(binary)),
                binary,
                &s,
                want,
            );
            s.terminate();
            migrated.push((name_of(binary), folder));
        }

        compare_folders(
            &mut errors,
            &format!("{variant} after the migration"),
            (migrated[0].0, &migrated[0].1),
            (migrated[1].0, &migrated[1].1),
            true,
        );

        for (reader, (writer, folder)) in [(&previous, &migrated[0]), (&current, &migrated[1])] {
            for round in 0..2 {
                let s = start(reader, folder, &settings);
                compare_reads(
                    &mut errors,
                    &format!(
                        "{variant} migrated by {writer}, read by {} round {round}",
                        name_of(reader)
                    ),
                    reader,
                    &s,
                    want,
                );
                s.terminate();
            }
        }
    }

    fail_on(errors);
}

fn restore_by_command(binary: &str, archive: &Path, folder: &Path) {
    let mut settings = Settings::corpus(true);
    settings.http = false;
    std::fs::DirBuilder::new()
        .recursive(true)
        .mode(0o755)
        .create(folder)
        .unwrap_or_else(|e| panic!("{e}"));
    let config = write_config(folder, 1, &settings);

    let out = Command::new(binary)
        .arg("--config")
        .arg(&config)
        .arg("restore")
        .arg(archive)
        .arg("--force")
        .env_remove("ELYSIANDB_ADMIN_PASSWORD")
        .output()
        .unwrap_or_else(|e| panic!("{binary}: {e}"));
    if !out.status.success() {
        panic!(
            "{} refuses the archive: {}\n{}{}",
            name_of(binary),
            out.status,
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn archives_are_restored_by_the_other_binary() {
    let Some((current, previous)) = binaries() else {
        skip(NEEDS_PREVIOUS);
        return;
    };

    let want = golden_full();
    let settings = Settings::corpus(true);
    let mut errors = Vec::new();
    for (writer, reader) in [(&current, &previous), (&previous, &current)] {
        let label = format!(
            "archive of {} restored by {}",
            name_of(writer),
            name_of(reader)
        );
        let dir = TempDir::new();

        let source = dir.path().join("source");
        copy_fixture("full", &source);
        let s = start(writer, &source, &settings);
        s.login(
            security::DEFAULT_ADMIN_USERNAME,
            security::DEFAULT_ADMIN_PASSWORD,
        );
        let archive = match s.client.get("/api/backup") {
            Ok(resp) if resp.status == 200 => resp.body,
            Ok(resp) => panic!("{label}: backup: {} {}", resp.status, resp.text()),
            Err(e) => panic!("{label}: backup: {e}"),
        };
        s.terminate();
        let file = dir.path().join("archive.tar.gz");
        write_private(&file, &archive);

        let by_command = dir.path().join("command");
        restore_by_command(reader, &file, &by_command);
        for round in 0..2 {
            let s = start(reader, &by_command, &settings);
            compare_reads(
                &mut errors,
                &format!("{label} with the command, round {round}"),
                reader,
                &s,
                &want,
            );
            s.terminate();
        }

        let by_route = dir.path().join("route");
        let s = start(reader, &by_route, &settings);
        s.login(
            security::DEFAULT_ADMIN_USERNAME,
            security::DEFAULT_ADMIN_PASSWORD,
        );
        match s
            .client
            .send("POST", "/api/restore", Some("application/gzip"), &archive)
        {
            Ok(resp) if resp.status == 200 => {}
            Ok(resp) => panic!("{label}: restore: {} {}", resp.status, resp.text()),
            Err(e) => panic!("{label}: restore: {e}"),
        }
        compare_reads(
            &mut errors,
            &format!("{label} with the route"),
            reader,
            &s,
            &want,
        );
        s.terminate();

        let s = start(reader, &by_route, &settings);
        compare_reads(
            &mut errors,
            &format!("{label} with the route, after a restart"),
            reader,
            &s,
            &want,
        );
        s.terminate();

        let back = dir.path().join("back");
        restore_by_command(writer, &file, &back);
        let s = start(writer, &back, &settings);
        compare_reads(
            &mut errors,
            &format!("archive of {} restored by itself", name_of(writer)),
            writer,
            &s,
            &want,
        );
        s.terminate();
    }

    fail_on(errors);
}

#[test]
fn sessions_and_accounts_of_one_binary_are_accepted_by_the_other() {
    let Some((current, previous)) = binaries() else {
        skip(NEEDS_PREVIOUS);
        return;
    };

    let settings = Settings::corpus(true);
    for (first, second) in [(&current, &previous), (&previous, &current)] {
        let label = format!("{} then {}", name_of(first), name_of(second));
        let dir = TempDir::new();
        let folder = dir.path().join("data");

        let s = start(first, &folder, &settings);
        s.login(
            security::DEFAULT_ADMIN_USERNAME,
            security::DEFAULT_ADMIN_PASSWORD,
        );
        let cookie = s
            .client
            .cookie(security::SESSION_COOKIE_NAME)
            .unwrap_or_else(|| panic!("{label}: the login sets no session cookie"));
        let (status, body) = s.fetch(
            "POST",
            "/api/security/user",
            r#"{"username":"second","password":"second-password-1","role":"admin"}"#,
            &label,
        );
        if status != 200 {
            panic!(
                "{label}: cannot create the account: {status} {}",
                String::from_utf8_lossy(&body)
            );
        }
        s.terminate();

        let s = start(second, &folder, &settings);
        s.client.set_cookie(security::SESSION_COOKIE_NAME, &cookie);
        let (status, body) = s.fetch("GET", "/api/security/me", "", &label);
        let body = String::from_utf8_lossy(&body);
        if status != 200 || body != r#"{"username":"admin","role":"admin"}"# {
            panic!("{label}: the session is refused: {status} {body}");
        }
        s.login("second", "second-password-1");
        let (status, body) = s.fetch("GET", "/api/security/me", "", &label);
        let body = String::from_utf8_lossy(&body);
        if status != 200 || body != r#"{"username":"second","role":"admin"}"# {
            panic!("{label}: the account is refused: {status} {body}");
        }
        s.terminate();
    }
}

fn start_loaded(binary: &str, folder: &Path) -> Arc<Server> {
    let port = free_port();
    let config = write_config(folder, port, &Settings::load());
    let s = spawn(binary, &config, port, folder, Some("cross-binary-password"));
    s.login("admin", "cross-binary-password");

    s
}

#[test]
fn kills_at_any_moment_across_binaries_lose_no_confirmed_write() {
    let Some((current, previous)) = binaries() else {
        skip(NEEDS_PREVIOUS);
        return;
    };

    let mut r = Rand::new(new_source(i64::from(std::process::id())));
    for first in [&current, &previous] {
        let second = if first == &current {
            &previous
        } else {
            &current
        };
        let dir = TempDir::new();
        let folder = dir.path().join("data");
        let c = Confirmed::default();
        let mut logs = String::new();

        for round in 0..6 {
            let binary = if round % 2 == 0 { first } else { second };
            let s = start_loaded(binary, &folder);
            verify_confirmed(
                &s,
                &c,
                &format!("start of round {round} with {}", name_of(binary)),
            );

            let stop = Arc::new(AtomicBool::new(false));
            let mut wg = Vec::new();
            write_load(&s, &c, round, &stop, &mut wg);

            if round % 2 == 1 {
                let sv = s.clone();
                wg.push(std::thread::spawn(move || {
                    let _ = sv.client.get("/api/backup");
                }));
            }

            std::thread::sleep(Duration::from_millis(300 + r.intn(1200) as u64));
            s.kill();
            stop.store(true, Ordering::SeqCst);
            wait(wg);
            logs.push_str(&s.log.string());
        }

        for binary in [first, second, first] {
            let s = start_loaded(binary, &folder);
            verify_confirmed(
                &s,
                &c,
                &format!("after the last kill with {}", name_of(binary)),
            );
            let took = s.terminate();
            if took > Duration::from_secs(9) {
                panic!("the server should stop before docker kills it, took {took:?}");
            }
            if !s.log.string().contains("shutdown complete") {
                panic!("no clean shutdown:\n{}", s.log.string());
            }
            logs.push_str(&s.log.string());
        }

        if logs.contains("level=ERROR") {
            panic!("errors in the logs:\n{logs}");
        }
    }
}
