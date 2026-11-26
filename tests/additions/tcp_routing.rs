use std::cell::Cell;
use std::net::SocketAddr;
use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::strings;
use elysiandb::transport::tcp::tcp_routing::{self, Conn, Security};

use crate::serial;

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

#[test]
fn test_unregistered_security_refuses_credentials() {
    let _serial = serial();
    globals::set_config(Some(Arc::new(Config::default())));
    tcp_routing::set_security(Security::default());

    let c = FakeConn::default();
    let mut session = tcp_routing::new_session();
    if session.is_authenticated() {
        panic!("without a registered security module a session must not be authenticated");
    }
    if tcp_routing::route_line(b"AUTH anything", &c, &mut session) != b"ERR invalid credentials"
        || session.is_authenticated()
    {
        panic!("without a registered security module AUTH must fail");
    }
    if tcp_routing::route_line(b"GET k", &c, &mut session) != b"ERR auth required" {
        panic!("without a registered security module commands must be refused");
    }

    tcp_routing::set_security(config_security());
}
