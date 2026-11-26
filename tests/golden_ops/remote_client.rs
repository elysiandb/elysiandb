use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;

use super::archives::archive;
use super::cli::{cli_config, run_cli};
use super::tempdir::TempDir;

const RED: &str = "\x1b[38;5;196m";
const GOLD: &str = "\x1b[38;5;220m";
const RESET: &str = "\x1b[0m";

#[derive(Clone, Debug, Default)]
struct Seen {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

type Reply = Box<dyn Fn(usize, &Seen) -> Vec<u8> + Send + Sync>;

struct Fake {
    port: u16,
    seen: Arc<Mutex<Vec<Seen>>>,
    thread: Option<JoinHandle<()>>,
    listener: TcpListener,
}

fn read_request(stream: &mut TcpStream) -> Option<Seen> {
    let mut reader = BufReader::new(stream.try_clone().ok()?);
    let mut line = String::new();
    if reader.read_line(&mut line).ok()? == 0 {
        return None;
    }
    let mut parts = line.trim_end().splitn(3, ' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut h = String::new();
        reader.read_line(&mut h).ok()?;
        let h = h.trim_end().to_string();
        if h.is_empty() {
            break;
        }
        let (n, v) = h.split_once(':')?;
        headers.push((n.trim().to_string(), v.trim().to_string()));
    }
    let len = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    let mut body = vec![0u8; len];
    reader.read_exact(&mut body).ok()?;
    Some(Seen {
        method,
        target,
        headers,
        body,
    })
}

impl Fake {
    fn start(reply: Reply) -> Fake {
        let listener = TcpListener::bind("127.0.0.1:0").expect("fake server");
        let port = listener.local_addr().expect("address").port();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let log = seen.clone();
        let accept = listener.try_clone().expect("listener copy");
        let thread = std::thread::spawn(move || {
            let mut n = 0;
            for stream in accept.incoming() {
                let Ok(mut stream) = stream else {
                    return;
                };
                let Some(req) = read_request(&mut stream) else {
                    continue;
                };
                if req.method == "STOP" {
                    return;
                }
                let answer = reply(n, &req);
                log.lock().unwrap_or_else(|e| e.into_inner()).push(req);
                let _ = stream.write_all(&answer);
                n += 1;
            }
        });
        Fake {
            port,
            seen,
            thread: Some(thread),
            listener,
        }
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    fn requests(&self) -> Vec<Seen> {
        self.seen.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Drop for Fake {
    fn drop(&mut self) {
        if let Ok(mut s) = TcpStream::connect(("127.0.0.1", self.port)) {
            let _ = s.write_all(b"STOP / HTTP/1.1\r\n\r\n");
        }
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
        let _ = &self.listener;
    }
}

fn response(status: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\n").into_bytes();
    for (n, v) in headers {
        out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    out.extend_from_slice(body);
    out
}

fn chunked(status: &str, headers: &[(&str, &str)], body: &[u8], finish: bool) -> Vec<u8> {
    let mut out = format!("HTTP/1.1 {status}\r\nTransfer-Encoding: chunked\r\n").into_bytes();
    for (n, v) in headers {
        out.extend_from_slice(format!("{n}: {v}\r\n").as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    for part in body.chunks(1000) {
        out.extend_from_slice(format!("{:x}\r\n", part.len()).as_bytes());
        out.extend_from_slice(part);
        out.extend_from_slice(b"\r\n");
    }
    if finish {
        out.extend_from_slice(b"0\r\n\r\n");
    }
    out
}

fn backup(fake: &Fake, extra: &[&str], env: &[(String, String)]) -> (String, i64, TempDir) {
    let dir = TempDir::new();
    std::fs::create_dir(dir.path().join("out")).expect("out");
    std::fs::write(dir.path().join("s.yaml"), cli_config("", 1)).expect("config");
    let mut args: Vec<String> = ["-config", "s.yaml", "backup", "--url", &fake.url()]
        .iter()
        .map(|s| s.to_string())
        .collect();
    args.extend(extra.iter().map(|s| s.to_string()));
    args.extend(["--output".to_string(), "out".to_string()]);
    let run = run_cli(dir.path(), &args, "", env);
    (run.stdout, run.exit, dir)
}

fn failure(out: &str, want: &str) {
    let line = format!("{RED}Backup failed: {want}{RESET}");
    assert!(out.contains(&line), "{out}");
}

fn files(dir: &TempDir) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir.path().join("out"))
        .expect("out folder")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}

fn env(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

#[test]
fn backup_url_reads_chunked_bodies_and_the_file_name() {
    let data = archive("seed/seed.tar.gz");
    let body = data.clone();
    let fake = Fake::start(Box::new(move |_, _| {
        chunked(
            "200 OK",
            &[(
                "Content-Disposition",
                "attachment; filename=\"from-server.tar.gz\"",
            )],
            &body,
            true,
        )
    }));
    let (out, exit, dir) = backup(&fake, &["--token", "t"], &[]);

    assert_eq!(exit, 0, "{out}");
    assert!(
        out.contains(&format!(
            "{GOLD}Backup written to out/from-server.tar.gz{RESET}"
        )),
        "{out}"
    );
    assert_eq!(files(&dir), ["from-server.tar.gz"]);
    assert!(std::fs::read(dir.path().join("out/from-server.tar.gz")).expect("archive") == data);
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        (reqs[0].method.as_str(), reqs[0].target.as_str()),
        ("GET", "/api/backup")
    );
    assert_eq!(reqs[0].header("Authorization"), Some("Bearer t"));
}

#[test]
fn backup_url_reports_a_truncated_body() {
    let data = archive("seed/seed.tar.gz");
    for chunks in [false, true] {
        let body = data.clone();
        let fake = Fake::start(Box::new(move |_, _| {
            if chunks {
                return chunked("200 OK", &[], &body, false);
            }
            let mut out = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                body.len() + 100
            )
            .into_bytes();
            out.extend_from_slice(&body);
            out
        }));
        let (out, exit, dir) = backup(&fake, &[], &[]);
        assert_eq!(exit, 1, "{out}");
        failure(
            &out,
            "the backup was interrupted: unexpected end of the response body",
        );
        assert!(files(&dir).is_empty(), "{:?}", files(&dir));
    }
}

#[test]
fn backup_url_stops_after_ten_redirects() {
    let fake = Fake::start(Box::new(|_, _| {
        response("302 Found", &[("Location", "/api/backup")], b"")
    }));
    let (out, exit, _dir) = backup(&fake, &[], &[]);
    assert_eq!(exit, 1, "{out}");
    failure(&out, "request to /api/backup failed: too many redirects");
    assert_eq!(fake.requests().len(), 10);
}

#[test]
fn backup_url_reports_the_status_and_the_body() {
    let fake = Fake::start(Box::new(|_, _| {
        response("401 Unauthorized", &[], b"  nope \n")
    }));
    let (out, exit, _dir) = backup(&fake, &[], &[]);
    assert_eq!(exit, 1, "{out}");
    failure(&out, "the server answered 401 Unauthorized: nope");
}

#[test]
fn backup_url_logs_in_and_sends_the_session_cookie() {
    let data = archive("seed/seed.tar.gz");
    let body = data.clone();
    let fake = Fake::start(Box::new(move |n, req| {
        if n == 0 {
            return response(
                "200 OK",
                &[(
                    "Set-Cookie",
                    "edb_session=abc; Path=/; HttpOnly; SameSite=Strict",
                )],
                b"{}",
            );
        }
        if req.header("Cookie") == Some("edb_session=abc") {
            return response("200 OK", &[], &body);
        }
        response("403 Forbidden", &[], b"")
    }));
    let (out, exit, _dir) = backup(
        &fake,
        &["--user", "ann"],
        &env(&[("ELYSIANDB_PASSWORD", "p<w")]),
    );

    assert_eq!(exit, 0, "{out}");
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 2);
    assert_eq!(
        (reqs[0].method.as_str(), reqs[0].target.as_str()),
        ("POST", "/api/security/login")
    );
    assert_eq!(reqs[0].header("Content-Type"), Some("application/json"));
    assert_eq!(reqs[0].body, br#"{"password":"p\u003cw","username":"ann"}"#);
    assert_eq!(reqs[1].header("Authorization"), None);
}

#[test]
fn backup_url_falls_back_to_basic_or_refuses_the_login() {
    let data = archive("seed/seed.tar.gz");
    let body = data.clone();
    let fake = Fake::start(Box::new(move |n, _| match n {
        0 => response("401 Unauthorized", &[], b""),
        1 => response("200 OK", &[], b""),
        _ => response("200 OK", &[], &body),
    }));
    let (out, exit, _dir) = backup(
        &fake,
        &["--user", "ann"],
        &env(&[("ELYSIANDB_PASSWORD", "pw")]),
    );
    assert_eq!(exit, 0, "{out}");
    let reqs = fake.requests();
    assert_eq!(reqs.len(), 3);
    assert_eq!(reqs[1].method, "HEAD");
    assert_eq!(reqs[1].header("Authorization"), Some("Basic YW5uOnB3"));
    assert_eq!(reqs[2].header("Authorization"), Some("Basic YW5uOnB3"));

    let fake = Fake::start(Box::new(|_, _| response("401 Unauthorized", &[], b"")));
    let (out, exit, _dir) = backup(
        &fake,
        &["--user", "ann"],
        &env(&[("ELYSIANDB_PASSWORD", "pw")]),
    );
    assert_eq!(exit, 1, "{out}");
    failure(&out, "login refused: 401 Unauthorized");
    assert_eq!(fake.requests().len(), 2);
}

#[test]
fn backup_url_verifies_the_archive_unless_told_not_to() {
    let fake = Fake::start(Box::new(|_, _| {
        response("200 OK", &[], b"not a gzip stream")
    }));
    let (out, exit, dir) = backup(&fake, &[], &[]);
    assert_eq!(exit, 1, "{out}");
    failure(
        &out,
        "invalid backup archive: not a gzip file: invalid gzip header",
    );
    assert!(files(&dir).is_empty());

    let (out, exit, dir) = backup(&fake, &["--no-verify"], &[]);
    assert_eq!(exit, 0, "{out}");
    let names = files(&dir);
    assert_eq!(names.len(), 1, "{names:?}");
    assert!(names[0].starts_with("elysiandb-backup-") && names[0].ends_with(".tar.gz"));
}

fn backup_to(url: &str, env: &[(String, String)]) -> (String, i64) {
    let dir = TempDir::new();
    std::fs::create_dir(dir.path().join("out")).expect("out");
    std::fs::write(dir.path().join("s.yaml"), cli_config("", 1)).expect("config");
    let args: Vec<String> = [
        "-config", "s.yaml", "backup", "--url", url, "--output", "out",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    let run = run_cli(dir.path(), &args, "", env);
    (run.stdout, run.exit)
}

#[test]
fn backup_url_goes_through_the_proxy_of_the_environment() {
    let data = archive("seed/seed.tar.gz");
    let body = data.clone();
    let proxy = Fake::start(Box::new(move |_, _| response("200 OK", &[], &body)));
    let proxy_url = format!("http://u:p@127.0.0.1:{}", proxy.port);

    let (out, exit) = backup_to(
        "http://backup.test",
        &env(&[("HTTP_PROXY", proxy_url.as_str())]),
    );
    assert_eq!(exit, 0, "{out}");
    let reqs = proxy.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(reqs[0].target, "http://backup.test/api/backup");
    assert_eq!(reqs[0].header("Host"), Some("backup.test"));
    assert_eq!(reqs[0].header("Proxy-Authorization"), Some("Basic dTpw"));

    let (out, exit) = backup_to(
        "http://backup.test",
        &env(&[
            ("HTTP_PROXY", proxy_url.as_str()),
            ("NO_PROXY", "backup.test"),
        ]),
    );
    assert_eq!(exit, 1, "{out}");
    failure_prefix(&out, "request to http://backup.test/api/backup failed: ");
    assert_eq!(proxy.requests().len(), 1);

    let refusing = Fake::start(Box::new(|_, _| response("403 Forbidden", &[], b"")));
    let refusing_url = format!("http://127.0.0.1:{}", refusing.port);
    let (out, exit) = backup_to(
        "https://backup.test",
        &env(&[("HTTPS_PROXY", refusing_url.as_str())]),
    );
    assert_eq!(exit, 1, "{out}");
    failure(
        &out,
        "request to https://backup.test/api/backup failed: unexpected status 403",
    );
    let reqs = refusing.requests();
    assert_eq!(reqs.len(), 1);
    assert_eq!(
        (reqs[0].method.as_str(), reqs[0].target.as_str()),
        ("CONNECT", "backup.test:443")
    );
}

fn failure_prefix(out: &str, prefix: &str) {
    let line = format!("{RED}Backup failed: {prefix}");
    assert!(out.contains(&line), "{out}");
}

#[test]
fn backup_url_reports_tls_errors() {
    let listener = TcpListener::bind("127.0.0.1:0").expect("plain server");
    let port = listener.local_addr().expect("address").port();
    let server = std::thread::spawn(move || {
        if let Ok((mut stream, _)) = listener.accept() {
            let _ = stream.set_read_timeout(Some(std::time::Duration::from_millis(300)));
            let mut buf = [0u8; 4096];
            while matches!(stream.read(&mut buf), Ok(n) if n > 0) {}
            let _ = stream.write_all(b"HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\n\r\n");
            std::thread::sleep(std::time::Duration::from_millis(300));
        }
    });

    let (out, exit) = backup_to(&format!("https://127.0.0.1:{port}"), &[]);
    let _ = server.join();
    assert_eq!(exit, 1, "{out}");
    failure_prefix(
        &out,
        "request to https://127.0.0.1:<PORT>/api/backup failed: TLS error: ",
    );
}
