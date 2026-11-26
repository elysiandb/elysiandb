#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use elysiandb::gocompat::json::{self, Value};

use super::http::Client;

pub const JSON: Option<&str> = Some("application/json");

pub fn binary() -> String {
    match std::env::var("ELYSIANDB_BINARY") {
        Ok(path) if !path.is_empty() => path,
        _ => env!("CARGO_BIN_EXE_elysiandb").to_string(),
    }
}

pub fn previous_binary() -> Option<String> {
    std::env::var("ELYSIANDB_PREVIOUS_BINARY")
        .ok()
        .filter(|path| !path.is_empty())
}

pub fn skip(reason: &str) {
    let _ = std::io::stderr().write_all(format!("\nskipped: {reason}\n").as_bytes());
}

#[derive(Clone, Default)]
pub struct ServerLog(Arc<Mutex<Vec<u8>>>);

impl ServerLog {
    pub fn string(&self) -> String {
        String::from_utf8_lossy(&self.0.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
    }

    fn follow<R: Read + Send + 'static>(&self, mut from: R) -> JoinHandle<()> {
        let sink = self.0.clone();
        std::thread::spawn(move || {
            let mut buf = [0u8; 8192];
            loop {
                match from.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => sink
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend_from_slice(&buf[..n]),
                }
            }
        })
    }
}

struct Process {
    child: Child,
    readers: Vec<JoinHandle<()>>,
    done: bool,
}

pub struct Server {
    process: Mutex<Process>,
    pid: libc::pid_t,
    pub client: Client,
    pub folder: PathBuf,
    pub log: ServerLog,
}

impl Drop for Server {
    fn drop(&mut self) {
        let mut p = self.process.lock().unwrap_or_else(|e| e.into_inner());
        if !p.done {
            let _ = p.child.kill();
            let _ = p.child.wait();
        }
    }
}

pub fn free_port() -> u16 {
    let ln = TcpListener::bind("127.0.0.1:0").unwrap_or_else(|e| panic!("{e}"));
    ln.local_addr().unwrap_or_else(|e| panic!("{e}")).port()
}

pub fn write_private(path: &Path, data: &[u8]) {
    use std::os::unix::fs::OpenOptionsExt;

    let mut f = std::fs::OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .open(path)
        .unwrap_or_else(|e| panic!("{e}"));
    f.write_all(data).unwrap_or_else(|e| panic!("{e}"));
}

pub fn spawn(
    binary: &str,
    config: &Path,
    port: u16,
    folder: &Path,
    admin_password: Option<&str>,
) -> Arc<Server> {
    let output = ServerLog::default();

    let mut cmd = Command::new(binary);
    cmd.arg("--config")
        .arg(config)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    match admin_password {
        Some(password) => cmd.env("ELYSIANDB_ADMIN_PASSWORD", password),
        None => cmd.env_remove("ELYSIANDB_ADMIN_PASSWORD"),
    };
    let mut child = cmd.spawn().unwrap_or_else(|e| panic!("{binary}: {e}"));

    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        readers.push(output.follow(out));
    }
    if let Some(err) = child.stderr.take() {
        readers.push(output.follow(err));
    }

    let s = Arc::new(Server {
        pid: child.id() as libc::pid_t,
        process: Mutex::new(Process {
            child,
            readers,
            done: false,
        }),
        client: Client::new(
            SocketAddr::from(([127, 0, 0, 1], port)),
            Duration::from_secs(30),
        ),
        folder: folder.to_path_buf(),
        log: output,
    });

    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Ok(resp) = s.client.get("/health")
            && resp.status == 200
        {
            break;
        }
        if Instant::now() > deadline {
            s.kill();
            panic!("the server never became ready:\n{}", s.log.string());
        }
        std::thread::sleep(Duration::from_millis(20));
    }

    s
}

impl Server {
    pub fn login(&self, username: &str, password: &str) {
        let resp = self
            .client
            .send(
                "POST",
                "/api/security/login",
                JSON,
                format!(r#"{{"username":"{username}","password":"{password}"}}"#).as_bytes(),
            )
            .unwrap_or_else(|e| panic!("{e}"));
        if resp.status != 200 {
            panic!(
                "login: {} {}\n{}",
                resp.status,
                resp.reason,
                self.log.string()
            );
        }
    }

    fn reap(&self) {
        let mut p = self.process.lock().unwrap_or_else(|e| e.into_inner());
        let _ = p.child.wait();
        p.done = true;
        for r in p.readers.drain(..) {
            let _ = r.join();
        }
    }

    pub fn kill(&self) {
        unsafe {
            libc::kill(self.pid, libc::SIGKILL);
        }
        self.reap();
    }

    pub fn terminate(&self) -> Duration {
        let begin = Instant::now();
        unsafe {
            libc::kill(self.pid, libc::SIGTERM);
        }

        loop {
            let exited = {
                let mut p = self.process.lock().unwrap_or_else(|e| e.into_inner());
                matches!(p.child.try_wait(), Ok(Some(_)) | Err(_))
            };
            if exited {
                break;
            }
            if begin.elapsed() > Duration::from_secs(30) {
                self.kill();
                panic!("the server does not stop:\n{}", self.log.string());
            }
            std::thread::sleep(Duration::from_millis(2));
        }

        let took = begin.elapsed();
        self.reap();

        took
    }

    pub fn stop(&self, crash: bool) {
        if crash {
            self.kill();
        } else {
            self.terminate();
        }
    }

    pub fn call(&self, method: &str, path: &str, body: &str) -> std::io::Result<(u16, Vec<u8>)> {
        let resp = self.client.send(method, path, JSON, body.as_bytes())?;

        Ok((resp.status, resp.body))
    }

    pub fn fetch(&self, method: &str, path: &str, body: &str, label: &str) -> (u16, Vec<u8>) {
        self.call(method, path, body)
            .unwrap_or_else(|e| panic!("{label}: {method} {path}: {e}"))
    }
}

#[derive(Default)]
pub struct Sets {
    pub docs: BTreeMap<String, i64>,
    pub deleted: BTreeSet<String>,
    pub kv: BTreeMap<String, i64>,
    pub bulk: BTreeSet<String>,
}

#[derive(Clone, Default)]
pub struct Confirmed(Arc<Mutex<Sets>>);

impl Confirmed {
    pub fn lock(&self) -> std::sync::MutexGuard<'_, Sets> {
        self.0.lock().unwrap_or_else(|e| e.into_inner())
    }
}

fn is_written(status: u16) -> bool {
    status == 200 || status == 201
}

pub fn write_load(
    s: &Arc<Server>,
    c: &Confirmed,
    round: usize,
    stop: &Arc<AtomicBool>,
    wg: &mut Vec<JoinHandle<()>>,
) {
    for w in 0..4 {
        let (s, c, stop) = (s.clone(), c.clone(), stop.clone());
        wg.push(std::thread::spawn(move || {
            let mut n = 0i64;
            while !stop.load(Ordering::SeqCst) {
                let id = format!("r{round}-w{w}-{}", n % 40);
                let body = format!(
                    r#"{{"id":"{id}","round":{round},"n":{n},"pad":"{}"}}"#,
                    "p".repeat(300)
                );
                let Ok((mut status, _)) = s.call("PUT", &format!("/api/single/{id}"), &body) else {
                    return;
                };
                if status == 404 {
                    let Ok((created, _)) = s.call("POST", "/api/single", &body) else {
                        return;
                    };
                    status = created;
                }
                if is_written(status) {
                    c.lock().docs.insert(id, n);
                }
                n += 1;
            }
        }));
    }

    let (sv, cf, st) = (s.clone(), c.clone(), stop.clone());
    wg.push(std::thread::spawn(move || {
        let mut b = 0;
        while !st.load(Ordering::SeqCst) {
            let mut docs = Vec::with_capacity(200);
            let mut ids = Vec::with_capacity(200);
            for i in 0..200 {
                let id = format!("bulk-r{round}-{b}-{i}");
                docs.push(format!(
                    r#"{{"id":"{id}","round":{round},"pad":"{}"}}"#,
                    "b".repeat(200)
                ));
                ids.push(id);
            }
            let Ok((status, _)) = sv.call("POST", "/api/bulk", &format!("[{}]", docs.join(",")))
            else {
                return;
            };
            if is_written(status) {
                cf.lock().bulk.extend(ids);
            }
            b += 1;
        }
    }));

    let (sv, st) = (s.clone(), stop.clone());
    wg.push(std::thread::spawn(move || {
        while !st.load(Ordering::SeqCst) {
            if sv
                .call(
                    "GET",
                    &format!("/api/bulk?filter[round][eq]={round}&limit=1"),
                    "",
                )
                .is_err()
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }));

    let (sv, cf, st) = (s.clone(), c.clone(), stop.clone());
    wg.push(std::thread::spawn(move || {
        let mut n = 0i64;
        while !st.load(Ordering::SeqCst) {
            let key = format!("k{round}-{}", n % 10);
            let Ok((status, _)) = sv.call("PUT", &format!("/kv/{key}"), &n.to_string()) else {
                return;
            };
            if status == 204 {
                cf.lock().kv.insert(key, n);
            }
            n += 1;
        }
    }));

    let (sv, cf, st) = (s.clone(), c.clone(), stop.clone());
    wg.push(std::thread::spawn(move || {
        let mut n = 0;
        while !st.load(Ordering::SeqCst) {
            let id = format!("gone-r{round}-{n}");
            match sv.call("POST", "/api/gone", &format!(r#"{{"id":"{id}"}}"#)) {
                Ok((status, _)) if is_written(status) => {}
                _ => return,
            }
            let Ok((status, _)) = sv.call("DELETE", &format!("/api/gone/{id}"), "") else {
                return;
            };
            if status < 300 {
                cf.lock().deleted.insert(id);
            }
            n += 1;
        }
    }));

    let (sv, st) = (s.clone(), stop.clone());
    wg.push(std::thread::spawn(move || {
        let mut n = 0;
        while !st.load(Ordering::SeqCst) {
            let body = format!(r#"{{"id":"j{n}","pad":"{}"}}"#, "j".repeat(400));
            if sv.call("POST", "/api/junk", &body).is_err() {
                return;
            }
            if n % 50 == 49 && sv.call("DELETE", "/api/junk", "").is_err() {
                return;
            }
            n += 1;
        }
    }));
}

pub fn wait(wg: Vec<JoinHandle<()>>) {
    for t in wg {
        t.join().unwrap_or_else(|_| panic!("a writer panicked"));
    }
}

pub fn listed_ids(body: &[u8], label: &str, what: &str) -> Vec<String> {
    let listed = json::unmarshal_value(body).unwrap_or_else(|e| panic!("{label}: {what}: {e}"));
    let Some(items) = listed.as_array() else {
        panic!(
            "{label}: {what}: not a list: {}",
            String::from_utf8_lossy(body)
        );
    };

    items
        .iter()
        .map(|doc| {
            doc.get("id")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string()
        })
        .collect()
}

fn leading_int(s: &str) -> i64 {
    let s = s.trim_start();
    let end = s
        .char_indices()
        .take_while(|(i, ch)| ch.is_ascii_digit() || (*i == 0 && (*ch == '-' || *ch == '+')))
        .map(|(i, ch)| i + ch.len_utf8())
        .last()
        .unwrap_or(0);

    s[..end].parse().unwrap_or(0)
}

fn status_of(answer: std::io::Result<(u16, Vec<u8>)>) -> String {
    match answer {
        Ok((status, _)) => format!("{status} <nil>"),
        Err(e) => format!("0 {e}"),
    }
}

pub fn verify_confirmed(s: &Server, c: &Confirmed, label: &str) {
    let c = c.lock();

    for (id, n) in &c.docs {
        let body = match s.call("GET", &format!("/api/single/{id}"), "") {
            Ok((200, body)) => body,
            other => panic!(
                "{label}: confirmed document {id} is lost: {}\n{}",
                status_of(other),
                s.log.string()
            ),
        };

        let doc = json::unmarshal_value(&body)
            .unwrap_or_else(|e| panic!("{label}: document {id} is corrupt: {e}"));
        let got = doc.get("n").and_then(Value::as_f64).unwrap_or(0.0);
        if (got as i64) < *n {
            panic!("{label}: document {id} went back to version {got}, {n} was confirmed");
        }
    }

    if !c.bulk.is_empty() {
        let body = match s.call("GET", "/api/bulk?fields=id", "") {
            Ok((200, body)) => body,
            other => panic!(
                "{label}: cannot list the bulk documents: {}",
                status_of(other)
            ),
        };

        let present: BTreeSet<String> = listed_ids(&body, label, "the list is corrupt")
            .into_iter()
            .collect();

        for id in &c.bulk {
            if !present.contains(id) {
                panic!("{label}: confirmed bulk document {id} is lost");
            }
        }

        let mut by_round: BTreeMap<String, usize> = BTreeMap::new();
        for id in &c.bulk {
            let round = id.split('-').nth(1).unwrap_or("r");
            *by_round.entry(round[1..].to_string()).or_default() += 1;
        }

        for _ in 0..3 {
            for (round, want) in &by_round {
                let body = match s.call(
                    "GET",
                    &format!("/api/bulk?fields=id&filter[round][eq]={round}"),
                    "",
                ) {
                    Ok((200, body)) => body,
                    other => panic!(
                        "{label}: cannot filter the bulk documents: {}",
                        status_of(other)
                    ),
                };

                let found = listed_ids(&body, label, "the filtered list is corrupt");
                if found.len() < *want {
                    panic!(
                        "{label}: the filter on round {round} finds {} documents, {want} were confirmed",
                        found.len()
                    );
                }
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }

    for id in &c.deleted {
        match s.call("GET", &format!("/api/gone/{id}"), "") {
            Ok((404, _)) => {}
            other => panic!(
                "{label}: confirmed delete of {id} is lost: {}",
                status_of(other)
            ),
        }
    }

    for (key, n) in &c.kv {
        let body = match s.call("GET", &format!("/kv/{key}"), "") {
            Ok((200, body)) => body,
            other => panic!("{label}: confirmed key {key} is lost: {}", status_of(other)),
        };

        let got = json::unmarshal_value(&body)
            .ok()
            .and_then(|kv| kv.get("value").and_then(Value::as_str).map(leading_int))
            .unwrap_or(0);
        if got < *n {
            panic!("{label}: key {key} went back to {got}, {n} was confirmed");
        }
    }
}
