use std::cell::Cell;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::gocompat::strings;
use elysiandb::log::{self, Options};
use elysiandb::storage;
use elysiandb::transport::tcp::tcp_routing::{self, Conn, Security};

use super::common;
use super::tempdir::TempDir;
use super::{Capture, Restore, log_state};

const LINE_LIMIT: usize = 131_072;

const SKIPPED: [(&str, &str, &str); 13] = [
    ("off", "line 131073", "line longer than the listener buffer"),
    ("off", "line 200000", "line longer than the listener buffer"),
    ("token", "unauthenticated timeout", "listener read deadline"),
    ("basic", "auth wrong", "user store of the security module"),
    ("basic", "auth one arg", "user store of the security module"),
    ("basic", "auth admin", "user store of the security module"),
    ("basic", "auth trimmed", "user store of the security module"),
    (
        "basic",
        "auth non-admin user",
        "user store of the security module",
    ),
    ("user", "auth wrong", "user store of the security module"),
    ("user", "auth one arg", "user store of the security module"),
    ("user", "auth admin", "user store of the security module"),
    ("user", "auth trimmed", "user store of the security module"),
    (
        "user",
        "auth non-admin user",
        "user store of the security module",
    ),
];

const PARTIAL: [(&str, &str, usize, &str); 2] = [
    (
        "off",
        "partial last line",
        1,
        "a final line without newline is dropped by the listener",
    ),
    (
        "off",
        "internal keys",
        2,
        "the core entity types are created at boot",
    ),
];

const CONNECTION_LINES: [&str; 4] = [
    "msg=\"connection opened\"",
    "msg=\"connection ended\"",
    "msg=\"connection closed\"",
    "msg=\"connection error\"",
];

struct FakeConn {
    closed: Cell<bool>,
    addr: SocketAddr,
}

impl Conn for FakeConn {
    fn close(&self) {
        self.closed.set(true);
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        Some(self.addr)
    }
}

fn config() -> Arc<Config> {
    match globals::config() {
        Some(cfg) => cfg,
        None => panic!("the configuration is not set"),
    }
}

fn token_mode() -> bool {
    let cfg = config();
    cfg.security.authentication.enabled && cfg.security.authentication.mode == "token"
}

fn config_security() -> Security {
    Security {
        credentials_are_required: Some(Arc::new(|| config().security.authentication.enabled)),
        token_authentication_is_enabled: Some(Arc::new(token_mode)),
        authenticate_credentials: Some(Arc::new(|credentials, _source| {
            let cfg = config();
            if !cfg.security.authentication.enabled {
                return true;
            }
            let credentials = strings::trim_space(credentials);
            if token_mode() {
                let expected = cfg.security.authentication.token.as_bytes();
                return !expected.is_empty() && credentials == expected;
            }
            false
        })),
    }
}

fn start_mode(mode: &str) -> TempDir {
    let _ = storage::close();
    let dir = TempDir::new();

    let mut cfg = Config::default();
    cfg.store.folder = dir.path().join("data").to_string_lossy().into_owned();
    cfg.store.shards = 4;
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
        other => panic!("unknown mode {other:?}"),
    }
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    tcp_routing::reset_throttles();
    tcp_routing::set_security(config_security());

    dir
}

fn str_of<'a>(v: &'a Value, key: &str) -> Option<&'a str> {
    v.get(key).and_then(Value::as_str)
}

fn flag(v: &Value, key: &str) -> bool {
    v.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn long_line(n: usize) -> Vec<u8> {
    let mut b = b"SET long ".to_vec();
    b.extend(std::iter::repeat_n(b'x', n - b.len() - 2));
    b.extend_from_slice(b"\r\n");
    b
}

fn step_send(step: &Value) -> Vec<u8> {
    let len = step
        .get("send_len")
        .and_then(Value::as_i64)
        .expect("send_len") as usize;
    let send = match (str_of(step, "send"), str_of(step, "send_prefix")) {
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

fn mask_time(line: &str) -> String {
    match line.strip_prefix("time=").and_then(|rest| rest.find(' ')) {
        Some(end) => format!("time=<T>{}", &line[5 + end..]),
        None => line.to_string(),
    }
}

fn routing_lines(lines: impl Iterator<Item = String>) -> Vec<String> {
    lines
        .filter(|l| {
            l.contains("component=tcp")
                || l.contains("component=audit")
                || l.contains("component=security")
        })
        .filter(|l| !CONNECTION_LINES.iter().any(|c| l.contains(c)))
        .map(|l| mask_time(&l))
        .collect()
}

#[test]
fn tcp_golden_matches_go() {
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
    let mut mode_dir: Option<(String, TempDir)> = None;
    let (mut full, mut partial, mut skipped) = (0usize, Vec::new(), Vec::new());

    for row in &rows {
        let mode = str_of(row, "mode").expect("mode");
        let name = str_of(row, "name").expect("name");
        if mode_dir.as_ref().is_none_or(|(m, _)| m != mode) {
            drop(mode_dir.take());
            mode_dir = Some((mode.to_string(), start_mode(mode)));
        }

        let steps = row.get("steps").and_then(Value::as_array).expect("steps");

        if let Some((_, _, reason)) = SKIPPED.iter().find(|(m, n, _)| *m == mode && *n == name) {
            let first = &steps[0];
            let listener_level = match name {
                "line 131073" | "line 200000" => {
                    flag(first, "reset") && step_send(first).len() > LINE_LIMIT
                }
                "unauthenticated timeout" => {
                    flag(first, "eof") && str_of(first, "send") == Some("")
                }
                _ => steps.iter().any(|s| {
                    str_of(s, "send").is_some_and(|t| t.to_ascii_uppercase().starts_with("AUTH"))
                }),
            };
            assert!(listener_level, "{mode}/{name} is not what the skip says");
            skipped.push(format!("{mode}/{name}: {reason}"));
            continue;
        }
        let stop_at = PARTIAL
            .iter()
            .find(|(m, n, _, _)| *m == mode && *n == name)
            .map(|(_, _, at, _)| *at);
        if let Some(at) = stop_at {
            let rest = &steps[at];
            let expected = match name {
                "partial last line" => flag(rest, "half_close") && flag(rest, "eof"),
                _ => str_of(rest, "send") == Some("GET api:entity:types:list\r\n"),
            };
            assert!(
                expected,
                "{mode}/{name} is not what the partial replay says"
            );
            assert_eq!(
                steps.len(),
                at + 1,
                "{mode}/{name} skips only its last step"
            );
        }

        let port = 40_000;
        let addr: SocketAddr = match str_of(row, "client") {
            Some("tcp6") => SocketAddr::from(([0, 0, 0, 0, 0, 0, 0, 1], port)),
            _ => SocketAddr::from(([127, 0, 0, 1], port)),
        };
        let conn = FakeConn {
            closed: Cell::new(false),
            addr,
        };
        let mut session = tcp_routing::new_session();
        capture.take();

        for (i, step) in steps.iter().enumerate() {
            if stop_at == Some(i) {
                break;
            }
            let send = step_send(step);
            assert!(
                send.ends_with(b"\n") && send.len() <= LINE_LIMIT,
                "{mode}/{name} step {i} must be one complete line"
            );
            assert!(!flag(step, "reset") && !flag(step, "half_close"));

            let resp = tcp_routing::route_line(&send, &conn, &mut session);
            let (recv, eof) = if conn.closed.get() {
                (Vec::new(), true)
            } else {
                let mut out = resp;
                out.push(b'\n');
                (out, false)
            };

            let want = str_of(step, "recv").expect("recv").as_bytes();
            assert!(
                recv == want,
                "{mode}/{name} step {i}: got {:?}, want {:?}",
                String::from_utf8_lossy(&recv),
                String::from_utf8_lossy(want)
            );
            assert_eq!(eof, flag(step, "eof"), "{mode}/{name} step {i} eof");

            if mode == "off" && name == "ttl expiry" && i == 2 {
                std::thread::sleep(Duration::from_millis(1600));
            }
        }

        let want_log = routing_lines(
            row.get("log")
                .and_then(Value::as_array)
                .expect("log")
                .iter()
                .map(|l| l.as_str().expect("log line").to_string()),
        );
        let got_log = routing_lines(capture.take().lines().map(str::to_string));
        assert_eq!(got_log, want_log, "{mode}/{name} log");

        match stop_at {
            Some(at) => partial.push(format!("{mode}/{name}: steps {at}..")),
            None => full += 1,
        }
    }

    let _ = storage::close();
    drop(mode_dir);

    assert_eq!(skipped.len(), SKIPPED.len(), "skipped cases {skipped:?}");
    assert_eq!(partial.len(), PARTIAL.len(), "partial cases {partial:?}");
    assert_eq!(full + partial.len() + skipped.len(), rows.len());
    assert_eq!(rows.len(), 46);
    assert_eq!(full, 31);
}
