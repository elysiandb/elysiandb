use std::io;
use std::net::{IpAddr, Ipv4Addr, TcpListener};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::datastore::{self, ERR_WRITE_FAILED};
use crate::gocompat::json::append_string;
use crate::httpd::{
    Handler, RequestCtx, STATUS_INSUFFICIENT_STORAGE, STATUS_SERVICE_UNAVAILABLE, Server,
    ServerConfig, bind_tcp4,
};
use crate::httprouter::Router;
use crate::kv;
use crate::log::{self, Logger};
use crate::routing;
use crate::security;
use crate::storage;
use crate::sync::Mutex;
use crate::transport::http::api as http_api;
use crate::transport::http::observe;

use super::init::config;
use super::tcp::{AddrError, go_addr, resolve_tcp_addr};

pub(crate) const HTTP_LOG: Logger = log::for_component("http");

pub(crate) const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(3);
const IDLE_TIMEOUT: Duration = Duration::from_secs(30);
const CONCURRENCY: usize = 100_000;
const BUFFER_SIZE: usize = 64 << 10;

pub(crate) static STOPPING: AtomicBool = AtomicBool::new(false);
pub(crate) static HTTP_SERVER: Mutex<Option<Server>> = Mutex::new(None);

pub fn new_router() -> Router {
    let mut r = Router::new();
    r.save_matched_route_path = true;
    r.panic_handler = Some(observe::panic_handler());
    routing::register_routes(&mut r);

    r
}

pub fn new_http_handler(r: Router) -> Handler {
    observe::middleware(refuse_writes_without_disk(security::protect(
        r.into_handler(),
    )))
}

pub fn changes_data(ctx: &RequestCtx) -> bool {
    if ctx.is_get() || ctx.is_head() || ctx.is_options() {
        return false;
    }

    let path = ctx.path();
    if path.starts_with(b"/kv/") {
        return true;
    }
    if !path.starts_with(b"/api/") {
        return false;
    }
    if path == b"/api/query" || path.starts_with(b"/api/security/log") {
        return false;
    }

    true
}

pub fn refuse_writes_without_disk(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        if !changes_data(ctx) {
            next(ctx);
            return;
        }

        if let Some(err) = storage::write_refusal()
            && !ctx.is_delete()
        {
            storage::note_refused_write();
            send_refused_write(ctx, err);
            return;
        }

        let before = storage::refused_writes();
        next(ctx);

        if storage::refused_writes() != before && !ctx.hijacked() {
            let err = storage::write_refusal().unwrap_or(ERR_WRITE_FAILED);
            send_refused_write(ctx, err);
        }
    })
}

fn send_refused_write(ctx: &mut RequestCtx, err: datastore::Error) {
    let status = if err == datastore::ERR_DISK_FULL {
        STATUS_INSUFFICIENT_STORAGE
    } else {
        STATUS_SERVICE_UNAVAILABLE
    };

    let message = err.to_string();
    ctx.response.reset();
    ctx.response.header.set("Content-Type", "application/json");
    ctx.response.header.set("Retry-After", "30");
    ctx.set_status_code(status);
    let mut body = b"{\"error\":".to_vec();
    append_string(&mut body, message.as_bytes(), true);
    body.push(b'}');
    ctx.set_body(body);
    observe::set_rejected(ctx, &message);
}

pub fn new_http_server() -> Server {
    let cfg = config();

    let mut sc = ServerConfig::new(http_api::close_after_unread_body(new_http_handler(
        new_router(),
    )));
    sc.header_received = Some(http_api::streaming_routes);
    sc.name = "ElysianDB".to_string();
    sc.concurrency = CONCURRENCY;
    sc.read_buffer_size = BUFFER_SIZE;
    sc.write_buffer_size = BUFFER_SIZE;
    sc.read_timeout = seconds_or(cfg.server.http.read_timeout_seconds, DEFAULT_HTTP_TIMEOUT);
    sc.write_timeout = seconds_or(cfg.server.http.write_timeout_seconds, DEFAULT_HTTP_TIMEOUT);
    sc.idle_timeout = IDLE_TIMEOUT;

    Server::new(sc)
}

fn copy_error(e: &io::Error) -> io::Error {
    match e.raw_os_error() {
        Some(n) => io::Error::from_raw_os_error(n),
        None => io::Error::new(e.kind(), e.to_string()),
    }
}

pub fn listen_tcp4(addr: &str) -> Result<TcpListener, AddrError> {
    let (ip, port) = resolve_tcp_addr(addr)?;
    let v4 = match ip.map(|ip| ip.to_canonical()) {
        None => Ipv4Addr::UNSPECIFIED,
        Some(IpAddr::V4(v4)) => v4,
        Some(IpAddr::V6(_)) => return Err(AddrError::NotIpv4(go_addr(ip, port))),
    };

    bind_tcp4(&format!("{v4}:{port}"))
        .map_err(|e| AddrError::Listen(go_addr(ip, port), copy_error(e.io_error())))
}

pub fn start_http() {
    let cfg = config();
    let addr = format!("{}:{}", cfg.server.http.host, cfg.server.http.port);

    let srv = new_http_server();

    let ln = match listen_tcp4(&addr) {
        Ok(ln) => ln,
        Err(err) => log::fatal(
            "cannot start the HTTP server",
            kv!["component" => "http", "addr" => addr, "err" => log::display(&err)],
        ),
    };

    *HTTP_SERVER.lock() = Some(srv.clone());
    HTTP_LOG.info("http listening", kv!["addr" => addr]);

    if let Err(err) = srv.serve(ln)
        && !STOPPING.load(Ordering::SeqCst)
    {
        log::fatal(
            "http server stopped",
            kv!["component" => "http", "addr" => addr, "err" => err],
        );
    }
}

pub(crate) fn seconds_or(seconds: i64, fallback: Duration) -> Duration {
    if seconds > 0 {
        return Duration::from_secs(seconds as u64);
    }

    fallback
}

pub fn stop_http(timeout: Duration) {
    let Some(srv) = HTTP_SERVER.lock().clone() else {
        return;
    };

    STOPPING.store(true, Ordering::SeqCst);
    if srv.shutdown_with_timeout(timeout).is_err() {
        let nanos = i64::try_from(timeout.as_nanos()).unwrap_or(i64::MAX);
        let err = format!("timed out after {}", crate::gocompat::time::Duration(nanos));
        HTTP_LOG.warn("http requests still running at shutdown", kv!["err" => err]);
    }
}

pub fn http_connections() -> i64 {
    match HTTP_SERVER.lock().as_ref() {
        Some(srv) => i64::from(srv.open_connections()),
        None => 0,
    }
}
