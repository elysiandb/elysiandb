#[path = "support/http.rs"]
mod http;
#[path = "support/process.rs"]
mod process;
#[path = "internal/tempdir.rs"]
mod tempdir;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use elysiandb::gocompat::rand::{Rand, new_source};

use process::{
    Confirmed, Server, binary, free_port, listed_ids, previous_binary, skip, spawn,
    verify_confirmed, wait, write_load, write_private,
};
use tempdir::TempDir;

fn write_config(folder: &Path, port: u16, extra: &str) -> PathBuf {
    let config = format!(
        "store:
  folder: {}
  shards: 16
  flushIntervalSeconds: 1
  crashRecovery: {{ enabled: true, maxLogMB: 1 }}
{extra}
engine:
  name: internal
server:
  http: {{ enabled: true, host: 127.0.0.1, port: {port} }}
  tcp: {{ enabled: false, host: 127.0.0.1, port: 1 }}
security:
  authentication: {{ enabled: true, mode: user }}
api:
  schema: {{ enabled: false }}
  hooks: {{ enabled: true }}
monitoring: {{ prometheus: {{ enabled: true }} }}
log: {{ level: info, format: text }}
",
        folder.display()
    );

    let path = PathBuf::from(format!("{}.yaml", folder.display()));
    write_private(&path, config.as_bytes());

    path
}

fn start(folder: &Path, extra: &str) -> Arc<Server> {
    start_binary(&binary(), folder, extra)
}

fn start_binary(binary: &str, folder: &Path, extra: &str) -> Arc<Server> {
    let port = free_port();
    let config = write_config(folder, port, extra);
    let s = spawn(binary, &config, port, folder, Some("process-test-password"));
    s.login("admin", "process-test-password");

    s
}

fn kill_rounds(extra: &str) {
    let dir = TempDir::new();
    let folder = dir.path().join("data");
    let c = Confirmed::default();
    let mut r = Rand::new(new_source(i64::from(std::process::id())));

    for round in 0..6 {
        let s = start(&folder, extra);
        verify_confirmed(&s, &c, &format!("start of round {round}"));

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
    }

    let s = start(&folder, extra);
    verify_confirmed(&s, &c, "after the last kill");

    let took = s.terminate();
    if took > Duration::from_secs(9) {
        panic!("the server should stop before docker kills it, took {took:?}");
    }

    if !s.log.string().contains("shutdown complete") {
        panic!("no clean shutdown:\n{}", s.log.string());
    }

    let s = start(&folder, extra);
    verify_confirmed(&s, &c, "after the clean stop");
    s.terminate();

    let logs = s.log.string();
    if logs.contains("level=ERROR") {
        panic!("errors in the logs:\n{logs}");
    }
}

#[test]
fn the_way_back_to_the_previous_binary_keeps_the_data() {
    let Some(previous) = previous_binary() else {
        skip("needs a binary of the previous commit in ELYSIANDB_PREVIOUS_BINARY");
        return;
    };

    let current = binary();

    for crash in [false, true] {
        let dir = TempDir::new();
        let folder = dir.path().join("data");
        let s = start_binary(&current, &folder, "");

        for _ in 0..50 {
            match s.call("GET", "/api/way?filter[tag][eq]=t1&limit=1", "") {
                Ok((200, _)) => {}
                Ok((status, _)) => panic!("{status} <nil>"),
                Err(e) => panic!("0 {e}"),
            }
        }

        let mut ids = BTreeSet::new();
        for b in 0..8 {
            let mut docs = Vec::with_capacity(300);
            for i in 0..300 {
                let id = format!("d{b}-{i}");
                docs.push(format!(
                    r#"{{"id":"{id}","tag":"t{}","pad":"{}"}}"#,
                    i % 5,
                    "w".repeat(200)
                ));
                ids.insert(id);
            }
            let (status, body) = s.fetch("POST", "/api/way", &format!("[{}]", docs.join(",")), "");
            if status >= 300 {
                panic!("{status} {}", String::from_utf8_lossy(&body));
            }
            if b == 4 {
                std::thread::sleep(Duration::from_millis(1500));
            }
        }
        match s.call("PUT", "/kv/way", "back") {
            Ok((204, _)) => {}
            Ok((status, _)) => panic!("{status} <nil>"),
            Err(e) => panic!("0 {e}"),
        }

        s.stop(crash);

        let check = |s: &Server, label: &str, want: usize| {
            let (status, body) = s.fetch("GET", "/api/way?fields=id", "", label);
            if status != 200 {
                panic!("{label}: {status}");
            }
            let listed = listed_ids(&body, label, "list");
            if listed.len() != want {
                panic!("{label}: {} documents, {want} expected", listed.len());
            }

            let (status, body) = s.fetch("GET", "/api/way?fields=id&filter[tag][eq]=t1", "", label);
            if status != 200 {
                panic!("{label}: {status}");
            }
            let filtered = listed_ids(&body, label, "filtered list");
            if filtered.len() < want / 5 {
                panic!(
                    "{label}: the filter finds {} documents of {want}",
                    filtered.len()
                );
            }

            let (status, body) = s.fetch("GET", "/kv/way", "", label);
            let body = String::from_utf8_lossy(&body);
            if status != 200 || !body.contains("back") {
                panic!("{label}: key {status} {body}");
            }
        };

        let old = start_binary(&previous, &folder, "");
        check(&old, &format!("previous binary, crash {crash}"), ids.len());
        let (status, body) = old.fetch(
            "POST",
            "/api/way",
            r#"{"id":"written-by-the-previous-binary","tag":"t1"}"#,
            "",
        );
        if status >= 300 {
            panic!("{status} {}", String::from_utf8_lossy(&body));
        }
        old.terminate();

        let again = start_binary(&current, &folder, "");
        check(
            &again,
            &format!("this binary again, crash {crash}"),
            ids.len() + 1,
        );
        again.terminate();
    }
}

fn metric(body: &str, name: &str) -> Option<f64> {
    let prefix = format!("{name} ");
    body.lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(|rest| {
            rest.split_whitespace()
                .next()
                .and_then(|v| v.parse::<f64>().ok())
                .unwrap_or_else(|| panic!("{name}: {rest}"))
        })
}

fn metric_value(s: &Server, name: &str) -> f64 {
    let (status, body) = s.fetch("GET", "/metrics", "", "metrics");
    if status != 200 {
        panic!("metrics: {status}");
    }
    let body = String::from_utf8_lossy(&body);

    metric(&body, name).unwrap_or_else(|| panic!("{name} is missing:\n{body}"))
}

fn threads_metric(s: &Server) -> &'static str {
    let (_, body) = s.fetch("GET", "/metrics", "", "metrics");
    let body = String::from_utf8_lossy(&body);
    if metric(&body, "elysiandb_threads").is_none()
        && metric(&body, "elysiandb_goroutines").is_some()
    {
        return "elysiandb_goroutines";
    }

    "elysiandb_threads"
}

struct Terminate(Arc<Server>);

impl Drop for Terminate {
    fn drop(&mut self) {
        if !std::thread::panicking() {
            self.0.terminate();
        }
    }
}

#[test]
fn backups_leave_nothing_behind_in_a_server() {
    let dir = TempDir::new();
    let s = start(&dir.path().join("data"), "");
    let _stop = Terminate(s.clone());

    let docs: Vec<String> = (0..2000)
        .map(|i| format!(r#"{{"id":"d{i}","pad":"{}"}}"#, "p".repeat(500)))
        .collect();
    let (status, body) = s.fetch("POST", "/api/steady", &format!("[{}]", docs.join(",")), "");
    if status >= 300 {
        panic!("{status} {}", String::from_utf8_lossy(&body));
    }

    let round = |n: usize| {
        for _ in 0..n {
            s.client
                .get("/api/backup")
                .unwrap_or_else(|e| panic!("{e}"));
            s.client
                .send_reading("GET", "/api/backup", None, b"", 16)
                .unwrap_or_else(|e| panic!("{e}"));
        }
        s.client.close_idle_connections();
        std::thread::sleep(Duration::from_secs(11));
    };

    round(3);
    let threads_name = threads_metric(&s);
    let threads = metric_value(&s, threads_name);
    let files = metric_value(&s, "elysiandb_open_fds");

    round(10);
    let got = metric_value(&s, threads_name);
    if got > threads + 8.0 {
        panic!("threads grow with backups: {threads} then {got}");
    }
    let got = metric_value(&s, "elysiandb_open_fds");
    if got > files + 8.0 {
        panic!("file descriptors grow with backups: {files} then {got}");
    }

    let completed = metric_value(&s, r#"elysiandb_backups_total{result="ok"}"#);
    if completed < 13.0 {
        panic!("completed backups: {completed}");
    }
}

#[test]
fn a_start_after_a_crash_announces_its_duration() {
    let dir = TempDir::new();
    let folder = dir.path().join("data");
    let s = start(&folder, "");
    for b in 0..3 {
        let docs: Vec<String> = (0..200)
            .map(|i| format!(r#"{{"id":"d{b}-{i}","pad":"{}"}}"#, "p".repeat(1000)))
            .collect();
        let (status, body) = s.fetch(
            "POST",
            "/api/announced",
            &format!("[{}]", docs.join(",")),
            "",
        );
        if status >= 300 {
            panic!("{status} {}", String::from_utf8_lossy(&body));
        }
    }
    s.kill();

    let s = start(&folder, "");
    s.terminate();

    let logs = s.log.string();
    for want in [
        "replaying the log",
        "estimated_seconds",
        "log replayed",
        "ready to serve",
        "startup_ms",
    ] {
        if !logs.contains(want) {
            panic!("{want:?} is missing from the logs:\n{logs}");
        }
    }
}

#[test]
fn kill_at_any_moment_loses_no_confirmed_write() {
    kill_rounds("");
}

#[test]
fn kill_with_synced_writes_loses_no_confirmed_write() {
    kill_rounds("  durability: fsync");
}

#[test]
fn a_stopped_server_under_load_leaves_in_time() {
    let dir = TempDir::new();
    let folder = dir.path().join("data");
    let s = start(&folder, "");
    let c = Confirmed::default();

    let stop = Arc::new(AtomicBool::new(false));
    let mut wg = Vec::new();
    write_load(&s, &c, 0, &stop, &mut wg);
    std::thread::sleep(Duration::from_millis(1500));

    let took = s.terminate();
    stop.store(true, Ordering::SeqCst);
    wait(wg);

    if took > Duration::from_secs(9) {
        panic!("docker kills after 10 seconds, the server took {took:?}");
    }

    let s = start(&folder, "");
    verify_confirmed(&s, &c, "after the stop");
    s.terminate();
}

fn uploads_left(folder: &Path) -> Vec<String> {
    std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|e| e.ok())
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .filter(|name| name.starts_with(".restore-upload-"))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn an_interrupted_restore_is_reported_and_can_be_run_again() {
    let dir = TempDir::new();
    let folder = dir.path().join("data");
    let s = start(&folder, "");
    for b in 0..20 {
        let docs: Vec<String> = (0..500)
            .map(|i| format!(r#"{{"id":"d{b}-{i}","pad":"{}"}}"#, "r".repeat(500)))
            .collect();
        let (status, body) = s.fetch(
            "POST",
            "/api/many",
            &format!("[{}]", docs.join(",")),
            "fill",
        );
        if status >= 300 {
            panic!("fill: {status} {}", String::from_utf8_lossy(&body));
        }
    }

    let archive = match s.client.get("/api/backup") {
        Ok(resp) if resp.status == 200 => Arc::new(resp.body),
        Ok(resp) => panic!("backup: {} <nil>", resp.status),
        Err(e) => panic!("backup: 0 {e}"),
    };

    let (sv, sent) = (s.clone(), archive.clone());
    let upload = std::thread::spawn(move || {
        let _ = sv
            .client
            .send("POST", "/api/restore", Some("application/gzip"), &sent);
    });

    let mark = s.folder.join("RESTORING");
    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if mark.exists() {
            break;
        }
        std::thread::sleep(Duration::from_millis(1));
    }
    s.kill();
    let _ = upload.join();

    let interrupted = mark.exists();

    let s = start(&folder, "");
    if interrupted && !s.log.string().contains("a restore was interrupted") {
        panic!("an interrupted restore is reported:\n{}", s.log.string());
    }

    let left = uploads_left(&folder);
    if !left.is_empty() {
        panic!("abandoned uploads are removed: {left:?}");
    }

    let resp = s
        .client
        .send("POST", "/api/restore", Some("application/gzip"), &archive)
        .unwrap_or_else(|e| panic!("{e}"));
    let report = resp.text();
    if resp.status != 200 || !report.contains(r#""many":10000"#) {
        panic!("restore: {} {} {report}", resp.status, resp.reason);
    }

    if mark.exists() {
        panic!("a finished restore leaves no mark");
    }
    s.terminate();
}
