use std::cell::Cell;
use std::net::SocketAddr;
use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::strings;
use elysiandb::metrics;
use elysiandb::sync::{Mutex, MutexGuard};
use elysiandb::transport::tcp::tcp_routing::{self, Conn, Security, Session};

static SERIAL: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

#[derive(Default)]
struct FakeConn {
    closed: Cell<bool>,
}

impl Conn for FakeConn {
    fn close(&self) {
        self.closed.set(true);
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        None
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

fn set_config(cfg: Config) {
    globals::set_config(Some(Arc::new(cfg)));
    tcp_routing::set_security(config_security());
}

#[test]
fn test_route_line_ping() {
    let _serial = serial();
    let c = FakeConn::default();
    let out = tcp_routing::route_line(b"PING", &c, &mut Session::default());
    if out != b"PONG" {
        panic!("bad PING");
    }
}

#[test]
fn test_route_line_exit() {
    let _serial = serial();
    let c = FakeConn::default();
    let out = tcp_routing::route_line(b"EXIT", &c, &mut Session::default());
    if out != b"Goodbye!" {
        panic!("bad EXIT");
    }
    if !c.closed.get() {
        panic!("conn not closed");
    }
}

#[test]
fn test_route_line_unknown() {
    let _serial = serial();
    set_config(Config::default());
    let c = FakeConn::default();
    let out = tcp_routing::route_line(b"WHAT", &c, &mut tcp_routing::new_session());
    if out != b"ERR" {
        panic!("bad unknown");
    }
}

#[test]
fn test_route_line_requires_auth_when_authentication_is_enabled() {
    let _serial = serial();
    let mut cfg = Config::default();
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "token".to_string();
    cfg.security.authentication.token = "tcp-token".to_string();
    set_config(cfg);

    let c = FakeConn::default();
    let mut session = tcp_routing::new_session();

    if session.is_authenticated() {
        panic!("a new session must not be authenticated when credentials are required");
    }

    let out = tcp_routing::route_line(b"PING\r\n", &c, &mut session);
    if out != b"PONG" {
        panic!(
            "PING must stay available, got {:?}",
            String::from_utf8_lossy(&out)
        );
    }

    for line in [
        "GET foo",
        "MGET foo",
        "SET foo bar",
        "DEL foo",
        "RESET",
        "SAVE",
        "WHAT",
    ] {
        let out = tcp_routing::route_line(line.as_bytes(), &c, &mut session);
        if out != b"ERR auth required" {
            panic!(
                "{line} before AUTH: got {:?}",
                String::from_utf8_lossy(&out)
            );
        }
    }

    let out = tcp_routing::route_line(b"AUTH nope", &c, &mut session);
    if out != b"ERR invalid credentials" || session.is_authenticated() {
        panic!(
            "a wrong token must be refused, got {:?}",
            String::from_utf8_lossy(&out)
        );
    }

    let out = tcp_routing::route_line(b"auth tcp-token\r\n", &c, &mut session);
    if out != b"OK" || !session.is_authenticated() {
        panic!(
            "the token must authenticate the session, got {:?}",
            String::from_utf8_lossy(&out)
        );
    }

    let out = tcp_routing::route_line(b"WHAT", &c, &mut session);
    if out != b"ERR" {
        panic!(
            "an authenticated session must reach the router, got {:?}",
            String::from_utf8_lossy(&out)
        );
    }
}

#[test]
fn test_new_session_is_authenticated_without_authentication() {
    let _serial = serial();
    set_config(Config::default());

    if !tcp_routing::new_session().is_authenticated() {
        panic!("a session must be authenticated when authentication is disabled");
    }
}

#[test]
fn test_command_name() {
    let cases = [
        ("GET k\n", metrics::TCP_GET),
        ("get k\n", metrics::TCP_GET),
        ("MGET a b\n", metrics::TCP_MGET),
        ("SET k v\n", metrics::TCP_SET),
        ("DEL k\n", metrics::TCP_DEL),
        ("PING\n", metrics::TCP_OTHER),
        ("AUTH secret\n", metrics::TCP_OTHER),
        ("FOO\n", metrics::TCP_OTHER),
    ];

    for (line, want) in cases {
        let got = tcp_routing::command_name(line.as_bytes());
        if got != want {
            panic!("command_name({line:?}) = {got:?}, want {want:?}");
        }
    }
}
