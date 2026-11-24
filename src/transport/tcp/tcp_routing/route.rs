use std::net::{IpAddr, Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::time::Duration;

use crate::globals;
use crate::gocompat::strings;
use crate::kv;
use crate::log::{self, Level, Logger, Throttle};
use crate::metrics;
use crate::stat;
use crate::sync::RwLock;
use crate::transport::tcp::handler;
use crate::transport::tcp::parsing;

const TCP_LOG: Logger = log::for_component("tcp");
const SECURITY_LOG: Logger = log::for_component("security");
const AUDIT_LOG: Logger = log::for_component("audit");
const MAX_LOGGED_COMMAND_BYTES: usize = 64;
const AUTH_THROTTLE_INTERVAL: Duration = Duration::from_secs(60);

static TCP_AUTH_THROTTLE: RwLock<Throttle> = RwLock::new(Throttle::new(AUTH_THROTTLE_INTERVAL));

pub type Predicate = Arc<dyn Fn() -> bool + Send + Sync>;
pub type Authenticator = Arc<dyn Fn(&[u8], Option<IpAddr>) -> bool + Send + Sync>;

#[derive(Clone, Default)]
pub struct Security {
    pub credentials_are_required: Option<Predicate>,
    pub token_authentication_is_enabled: Option<Predicate>,
    pub authenticate_credentials: Option<Authenticator>,
}

static SECURITY: RwLock<Security> = RwLock::new(Security {
    credentials_are_required: None,
    token_authentication_is_enabled: None,
    authenticate_credentials: None,
});

pub fn set_security(s: Security) {
    *SECURITY.write() = s;
}

pub fn security() -> Security {
    SECURITY.read().clone()
}

fn credentials_are_required() -> bool {
    let check = SECURITY.read().credentials_are_required.clone();
    check.is_none_or(|f| f())
}

fn token_authentication_is_enabled() -> bool {
    let check = SECURITY.read().token_authentication_is_enabled.clone();
    check.is_some_and(|f| f())
}

fn authenticate_credentials(credentials: &[u8], source: Option<IpAddr>) -> bool {
    let check = SECURITY.read().authenticate_credentials.clone();
    check.is_some_and(|f| f(credentials, source))
}

pub trait Conn {
    fn close(&self);
    fn remote_addr(&self) -> Option<SocketAddr>;
}

impl Conn for TcpStream {
    fn close(&self) {
        let _ = self.shutdown(Shutdown::Both);
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        self.peer_addr().ok()
    }
}

pub fn login_source(addr: Option<SocketAddr>) -> Option<IpAddr> {
    addr.map(|a| a.ip().to_canonical())
}

pub fn source_name(ip: Option<IpAddr>) -> String {
    match ip {
        Some(ip) => ip.to_string(),
        None => "unknown".to_string(),
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Session {
    authenticated: bool,
    actor: Vec<u8>,
}

pub fn new_session() -> Session {
    Session {
        authenticated: !credentials_are_required(),
        actor: Vec::new(),
    }
}

impl Session {
    pub fn is_authenticated(&self) -> bool {
        self.authenticated
    }

    fn actor_name(&self) -> &[u8] {
        if !self.actor.is_empty() {
            return &self.actor;
        }

        if !credentials_are_required() {
            return b"anonymous";
        }

        b"unknown"
    }
}

pub fn route_line(line: &[u8], c: &dyn Conn, session: &mut Session) -> Vec<u8> {
    let (cmd, mut query) = parsing::first_word_bytes(line);
    if let Some(cfg) = globals::config()
        && cfg.stats.enabled
    {
        stat::stats().increment_total_requests();
    }

    if parsing::eq_ascii(cmd, b"PING") {
        return b"PONG".to_vec();
    }

    if parsing::eq_ascii(cmd, b"EXIT") {
        c.close();
        return b"Goodbye!".to_vec();
    }

    if parsing::eq_ascii(cmd, b"AUTH") {
        let source = login_source(c.remote_addr());
        if !authenticate_credentials(query, source) {
            if token_authentication_is_enabled() {
                log_token_auth_failure(&source_name(source));
            }
            return b"ERR invalid credentials".to_vec();
        }

        session.authenticated = true;
        session.actor = actor_from_credentials(query);

        return b"OK".to_vec();
    }

    if !session.authenticated {
        return b"ERR auth required".to_vec();
    }

    if parsing::eq_ascii(cmd, b"GET") {
        return handler::handle_get(query);
    }

    if parsing::eq_ascii(cmd, b"MGET") {
        return handler::handle_multi_get(query);
    }

    if parsing::eq_ascii(cmd, b"SET") {
        let ttl = extract_ttl_from_query(&mut query);
        return handler::handle_set(query, ttl);
    }

    if parsing::eq_ascii(cmd, b"DEL") {
        return handler::handle_delete(query);
    }

    if parsing::eq_ascii(cmd, b"RESET") {
        let resp = handler::handle_reset();
        AUDIT_LOG.info(
            "store reset",
            kv![
                "actor" => session.actor_name(),
                "source" => "tcp",
                "remote_addr" => remote_addr(c),
            ],
        );
        return resp;
    }

    if parsing::eq_ascii(cmd, b"SAVE") {
        return handler::handle_save();
    }

    if log::enabled(Level::DEBUG) {
        let command = &cmd[..cmd.len().min(MAX_LOGGED_COMMAND_BYTES)];
        TCP_LOG.debug(
            "unknown command",
            kv!["remote_addr" => remote_addr(c), "command" => command],
        );
    }

    b"ERR".to_vec()
}

pub fn reset_throttles() {
    *TCP_AUTH_THROTTLE.write() = Throttle::new(AUTH_THROTTLE_INTERVAL);
}

fn log_token_auth_failure(addr: &str) {
    metrics::failed_login(addr);
    let (suppressed, ok) = TCP_AUTH_THROTTLE.read().allow(addr);
    if ok {
        SECURITY_LOG.warn(
            "tcp auth failed",
            &log::with_suppressed(
                kv!["remote_addr" => addr, "auth_mode" => "token"],
                suppressed,
            ),
        );
    }
}

fn actor_from_credentials(credentials: &[u8]) -> Vec<u8> {
    if token_authentication_is_enabled() {
        return b"token".to_vec();
    }

    let trimmed = strings::trim_space(credentials);
    match trimmed.iter().position(|&b| b == b' ') {
        Some(i) => trimmed[..i].to_vec(),
        None => Vec::new(),
    }
}

fn remote_addr(c: &dyn Conn) -> String {
    source_name(login_source(c.remote_addr()))
}

fn extract_ttl_from_query(query: &mut &[u8]) -> i64 {
    let (ttl_param, rest) = parsing::first_word_bytes(query);

    if ttl_param.len() < 4 {
        return 0;
    }

    if parsing::eq_ascii(&ttl_param[..4], b"TTL=") {
        return match parsing::parse_decimal_bytes(&ttl_param[4..]) {
            Ok(ttl) if ttl >= 0 => {
                *query = rest;
                ttl
            }
            _ => 0,
        };
    }

    0
}

pub fn command_name(line: &[u8]) -> &'static str {
    let (cmd, _) = parsing::first_word_bytes(line);

    if parsing::eq_ascii(cmd, b"GET") {
        return metrics::TCP_GET;
    }
    if parsing::eq_ascii(cmd, b"MGET") {
        return metrics::TCP_MGET;
    }
    if parsing::eq_ascii(cmd, b"SET") {
        return metrics::TCP_SET;
    }
    if parsing::eq_ascii(cmd, b"DEL") {
        return metrics::TCP_DEL;
    }

    metrics::TCP_OTHER
}
