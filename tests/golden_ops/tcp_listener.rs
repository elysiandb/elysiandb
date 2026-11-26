use std::io::{ErrorKind, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::{Duration, Instant};

use elysiandb::boot;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::log::{self, Options};
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::transport::tcp::tcp_routing;

use super::common::{self, str_field};
use super::tempdir::TempDir;
use super::{Capture, Restore, log_state};

const ROWS: usize = 46;
const GO_TCP_IO: &str = "err=\u{27e8}go:tcp-io\u{27e9}";
const TIMEOUT_CASE: (&str, &str) = ("token", "unauthenticated timeout");

fn err_text(class: &str) -> &'static str {
    match class {
        "eof" => "connection closed by the client",
        "buffer-full" => "line longer than 131072 bytes",
        "closed" => "connection closed",
        "timeout" => "read timed out",
        other => panic!("unknown error class {other}"),
    }
}

fn start_mode(mode: &str, capture: &Capture) -> (TempDir, SocketAddr) {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().join("data").to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.server.http.enabled = true;
    cfg.server.http.host = "127.0.0.1".to_string();
    cfg.server.http.port = 1;
    cfg.server.tcp.enabled = true;
    cfg.server.tcp.host = if mode == "dual" {
        "0.0.0.0"
    } else {
        "127.0.0.1"
    }
    .to_string();
    cfg.server.tcp.port = 0;
    cfg.log.level = "debug".to_string();
    cfg.log.format = "text".to_string();
    let auth = &mut cfg.security.authentication;
    match mode {
        "off" => {}
        "token" | "dual" => {
            auth.enabled = true;
            auth.mode = "token".to_string();
            auth.token = "tcp-token".to_string();
        }
        "basic" | "user" => {
            auth.enabled = true;
            auth.mode = mode.to_string();
        }
        other => panic!("unknown mode {other}"),
    }
    globals::set_config(Some(Arc::new(cfg)));

    boot::init_db();
    if mode == "basic" {
        unsafe { std::env::set_var(security::ADMIN_PASSWORD_ENV_VAR, "ad min") };
    }
    let created = security::init_admin_user_if_not_exists();
    unsafe { std::env::remove_var(security::ADMIN_PASSWORD_ENV_VAR) };
    created.expect("default admin");
    security::protect_sessions_file();
    if mode == "user" {
        security::create_basic_user(&BasicUser {
            username: b"ann".to_vec(),
            password: b"secret-1".to_vec(),
            role: Role::user(),
        })
        .expect("user mode ann");
    }
    tcp_routing::reset_throttles();
    security::reset_failed_login_throttle();
    security::reset_login_attempts();

    let before = boot::tcp_listener_addr();
    std::thread::spawn(boot::init_tcp);
    let deadline = Instant::now() + Duration::from_secs(10);
    let addr = loop {
        if let Some(addr) = boot::tcp_listener_addr()
            && Some(addr) != before
        {
            break addr;
        }
        assert!(Instant::now() < deadline, "the TCP listener did not start");
        std::thread::sleep(Duration::from_millis(10));
    };

    wait_for(capture, "msg=\"tcp listening\"");
    capture.take();

    (dir, addr)
}

fn wait_for(capture: &Capture, needle: &str) {
    let deadline = Instant::now() + Duration::from_secs(40);
    while !capture.peek().contains(needle) {
        assert!(Instant::now() < deadline, "no log line with {needle}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

fn long_line(n: usize) -> Vec<u8> {
    let mut b = b"SET long ".to_vec();
    b.extend(std::iter::repeat_n(b'x', n - b.len() - 2));
    b.extend_from_slice(b"\r\n");
    b
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn step_send(step: &Value) -> Vec<u8> {
    let len = step
        .get("send_len")
        .and_then(Value::as_i64)
        .expect("send_len") as usize;
    let send = match (
        step.get("send").and_then(Value::as_str),
        step.get("send_prefix").and_then(Value::as_str),
    ) {
        (Some(s), None) => s.as_bytes().to_vec(),
        (None, Some(prefix)) => {
            let b = long_line(len);
            assert_eq!(&b[..16], prefix.as_bytes(), "send prefix");
            b
        }
        _ => panic!("step without send"),
    };
    assert_eq!(send.len(), len, "send_len");
    send
}

#[derive(Debug, PartialEq, Eq)]
enum End {
    Open,
    Eof,
    Reset,
}

fn read_answer(conn: &mut TcpStream, want: usize, wait: Duration) -> (Vec<u8>, End) {
    let mut got = Vec::new();
    let deadline = Instant::now() + wait;
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let now = Instant::now();
        if got.len() >= want && want > 0 {
            let _ = conn.set_read_timeout(Some(Duration::from_millis(20)));
        } else if now >= deadline {
            return (got, End::Open);
        } else {
            let _ = conn.set_read_timeout(Some(deadline - now));
        }
        match conn.read(&mut buf) {
            Ok(0) => return (got, End::Eof),
            Ok(n) => got.extend_from_slice(&buf[..n]),
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut) => {
                if got.len() >= want {
                    return (got, End::Open);
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => return (got, End::Reset),
        }
    }
}

fn routing_lines(text: &str) -> Vec<String> {
    text.lines()
        .filter(|l| {
            l.contains("component=tcp")
                || l.contains("component=audit")
                || l.contains("component=security")
        })
        .map(|l| {
            let l = match l.strip_prefix("time=").and_then(|rest| rest.find(' ')) {
                Some(end) => format!("time=<T>{}", &l[5 + end..]),
                None => l.to_string(),
            };
            match l.find("duration_ms=") {
                Some(at) => {
                    let start = at + "duration_ms=".len();
                    let end = start
                        + l[start..]
                            .bytes()
                            .take_while(|c| c.is_ascii_digit() || *c == b'.')
                            .count();
                    format!("{}<N>{}", &l[..start], &l[end..])
                }
                None => l,
            }
        })
        .collect()
}

fn expected_log(row: &Value) -> Vec<String> {
    let lines = row.get("log").and_then(Value::as_array).expect("log");
    let classes = row
        .get("log_err")
        .and_then(Value::as_array)
        .expect("log_err");
    assert_eq!(lines.len(), classes.len());
    lines
        .iter()
        .zip(classes)
        .map(|(line, class)| {
            let line = line.as_str().expect("log line");
            match class.as_str() {
                Some(class) => {
                    let text = log_value(err_text(class));
                    line.replace(GO_TCP_IO, &format!("err={text}"))
                }
                None => {
                    assert!(!line.contains(GO_TCP_IO));
                    line.to_string()
                }
            }
        })
        .collect()
}

fn log_value(s: &str) -> String {
    if s.bytes().any(|c| c <= b' ' || c == b'=' || c == b'"') {
        return format!("\"{s}\"");
    }
    s.to_string()
}

fn connect(row: &Value, addr: SocketAddr) -> TcpStream {
    let target = match row.get("client").and_then(Value::as_str) {
        Some("tcp6") => SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], addr.port())),
        _ => SocketAddr::from(([127, 0, 0, 1], addr.port())),
    };
    TcpStream::connect(target).unwrap_or_else(|e| panic!("connect {target}: {e}"))
}

fn play(row: &Value, conn: &mut TcpStream) {
    let mode = str_field(row, "mode");
    let name = str_field(row, "name");
    let steps = row.get("steps").and_then(Value::as_array).expect("steps");
    for (i, step) in steps.iter().enumerate() {
        let send = step_send(step);
        if !send.is_empty() {
            let _ = conn.write_all(&send);
        }
        if flag(step, "half_close") {
            let _ = conn.shutdown(Shutdown::Write);
        }

        let want = str_field(step, "recv").as_bytes();
        let wait = if (mode, name) == TIMEOUT_CASE {
            Duration::from_secs(40)
        } else {
            Duration::from_secs(5)
        };
        let (got, end) = read_answer(conn, want.len(), wait);
        assert!(
            got == want,
            "{mode}/{name} step {i}: got {:?}, want {:?}",
            String::from_utf8_lossy(&got),
            String::from_utf8_lossy(want)
        );
        let want_end = if flag(step, "eof") {
            End::Eof
        } else if flag(step, "reset") {
            End::Reset
        } else {
            End::Open
        };
        assert_eq!(end, want_end, "{mode}/{name} step {i}");
        if end != End::Open {
            return;
        }
        if (mode, name) == ("off", "ttl expiry") && i == 2 {
            std::thread::sleep(Duration::from_millis(1600));
        }
    }
}

#[test]
fn tcp_listener_golden_matches_go() {
    let _state = log_state();
    let _restore = Restore;
    log::configure(&Options {
        level: "debug".to_string(),
        format: "text".to_string(),
        ..Options::default()
    })
    .expect("configure");
    let capture = Capture::default();
    log::set_output(Some(Box::new(capture.clone())), false);

    let rows = common::read_jsonl("ops/tcp/cases.jsonl");
    assert_eq!(rows.len(), ROWS);

    let mut modes: Vec<&str> = Vec::new();
    for row in &rows {
        let mode = str_field(row, "mode");
        if modes.last() != Some(&mode) {
            modes.push(mode);
        }
    }

    let mut played = 0;
    for mode in modes {
        let (_dir, addr) = start_mode(mode, &capture);
        let cases: Vec<&Value> = rows
            .iter()
            .filter(|r| str_field(r, "mode") == mode)
            .collect();

        let mut slow = None;
        if let Some(row) = cases
            .iter()
            .find(|r| (str_field(r, "mode"), str_field(r, "name")) == TIMEOUT_CASE)
        {
            let conn = connect(row, addr);
            wait_for(&capture, "msg=\"connection opened\"");
            slow = Some((*row, conn, capture.take()));
        }

        for row in &cases {
            if (str_field(row, "mode"), str_field(row, "name")) == TIMEOUT_CASE {
                continue;
            }
            let mut conn = connect(row, addr);
            play(row, &mut conn);
            drop(conn);
            wait_for(&capture, "msg=\"connection closed\"");
            let got = routing_lines(&capture.take());
            assert_eq!(
                got,
                expected_log(row),
                "{}/{} log",
                str_field(row, "mode"),
                str_field(row, "name")
            );
            played += 1;
        }

        if let Some((row, mut conn, opened)) = slow {
            play(row, &mut conn);
            drop(conn);
            wait_for(&capture, "msg=\"connection closed\"");
            let mut text = opened;
            text.push_str(&capture.take());
            assert_eq!(routing_lines(&text), expected_log(row), "timeout log");
            played += 1;
        }
    }

    assert_eq!(played, ROWS);
}
