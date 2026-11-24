use std::any::Any;
use std::backtrace::Backtrace;
use std::cell::RefCell;
use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Once, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::api::Guard;
use crate::crypto::rand::os_random;
use crate::globals;
use crate::gocompat::strconv;
use crate::httpd::{
    HEADER_AUTHORIZATION, Handler, METHOD_DELETE, METHOD_GET, METHOD_PUT, RequestCtx,
    STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_UNAUTHORIZED,
};
use crate::httprouter::{self, PanicHandler};
use crate::log::{self, Attr, Level, Logger, Throttle, ToValue, Value};
use crate::metrics;
use crate::security::{self, SESSION_COOKIE_NAME};
use crate::sync::RwLock;

pub const REQUEST_ID_HEADER: &str = "X-Request-Id";
pub const ERROR_KEY: &str = "elysiandb.error";
pub const LOGGED_KEY: &str = "elysiandb.error_logged";
pub const REJECTED_KEY: &str = "elysiandb.rejected";
pub const REQUEST_ID_KEY: &str = "elysiandb.request_id";
pub const GUARD_KEY: &str = "elysian.guard";
pub const DEFERRED_KEY: &str = "observeDeferred";
pub const UNKNOWN_PANIC: &str = "unknown panic";

const MAX_REQUEST_ID_LEN: usize = 64;
const UNMATCHED_ROUTE: &str = "unmatched";
const QUIET_UNAUTHORIZED_ROUTES: [&str; 2] = ["/api/security/login", "/api/security/me"];
const THROTTLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(60);

const HTTP_LOG: Logger = log::for_component("http");
const SECURITY_LOG: Logger = log::for_component("security");

struct Throttles {
    unauthorized: Throttle,
    forbidden: Throttle,
    slow: Throttle,
    rejected: Throttle,
}

impl Throttles {
    const fn new() -> Throttles {
        Throttles {
            unauthorized: Throttle::new(THROTTLE_INTERVAL),
            forbidden: Throttle::new(THROTTLE_INTERVAL),
            slow: Throttle::new(THROTTLE_INTERVAL),
            rejected: Throttle::new(THROTTLE_INTERVAL),
        }
    }
}

static THROTTLES: RwLock<Option<Arc<Throttles>>> = RwLock::new(None);
static ID_PREFIX: OnceLock<String> = OnceLock::new();
static ID_COUNTER: AtomicU64 = AtomicU64::new(0);
static PANIC_HOOK: Once = Once::new();

thread_local! {
    static CAUGHT_STACK: RefCell<Option<String>> = const { RefCell::new(None) };
}

fn throttles() -> Arc<Throttles> {
    if let Some(t) = THROTTLES.read().as_ref() {
        return t.clone();
    }

    THROTTLES
        .write()
        .get_or_insert_with(|| Arc::new(Throttles::new()))
        .clone()
}

fn new_id_prefix() -> String {
    let mut b = [0u8; 4];
    if os_random(&mut b).is_err() {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| i64::try_from(d.as_nanos()).unwrap_or(i64::MAX))
            .unwrap_or(0);
        return strconv::format_int(nanos, 36);
    }

    b.iter().map(|c| format!("{c:02x}")).collect()
}

fn id_prefix() -> &'static str {
    ID_PREFIX.get_or_init(new_id_prefix)
}

pub fn reset_throttles() {
    *THROTTLES.write() = Some(Arc::new(Throttles::new()));
}

pub fn set_error(ctx: &mut RequestCtx, err: &dyn fmt::Display) {
    ctx.set_user_value(ERROR_KEY, err.to_string().into_bytes());
}

pub fn set_error_bytes(ctx: &mut RequestCtx, message: impl Into<Vec<u8>>) {
    ctx.set_user_value(ERROR_KEY, message.into());
}

pub fn error(ctx: &RequestCtx) -> Option<&[u8]> {
    ctx.user_value::<Vec<u8>>(ERROR_KEY).map(Vec::as_slice)
}

pub fn set_rejected(ctx: &mut RequestCtx, reason: &str) {
    ctx.set_user_value(REJECTED_KEY, reason.to_string());
}

pub fn request_id(ctx: &RequestCtx) -> String {
    ctx.user_value::<String>(REQUEST_ID_KEY)
        .cloned()
        .unwrap_or_default()
}

pub fn install_panic_hook() {
    PANIC_HOOK.call_once(|| {
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if httprouter::in_recovery_point() {
                let stack = Backtrace::force_capture().to_string();
                CAUGHT_STACK.with(|c| *c.borrow_mut() = Some(stack));
                return;
            }

            previous(info);
        }));
    });
}

pub fn panic_message(recovered: &(dyn Any + Send)) -> String {
    if let Some(s) = recovered.downcast_ref::<&str>() {
        return (*s).to_string();
    }

    if let Some(s) = recovered.downcast_ref::<String>() {
        return s.clone();
    }

    UNKNOWN_PANIC.to_string()
}

pub fn panic(ctx: &mut RequestCtx, recovered: Box<dyn Any + Send>) {
    let message = panic_message(recovered.as_ref());
    let stack = CAUGHT_STACK
        .with(|c| c.borrow_mut().take())
        .unwrap_or_else(|| Backtrace::force_capture().to_string());
    let route = route(ctx);
    let request_id = request_id(ctx);

    HTTP_LOG.error(
        "handler panic",
        &[
            ("method", Value::Str(ctx.method())),
            ("route", route.to_value()),
            ("request_id", request_id.to_value()),
            ("panic", message.to_value()),
            ("stack", stack.to_value()),
        ],
    );

    ctx.set_user_value(LOGGED_KEY, true);
    ctx.reset_body();
    ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
}

pub fn panic_handler() -> PanicHandler {
    Arc::new(panic)
}

pub fn route(ctx: &RequestCtx) -> String {
    match ctx.matched_route_path() {
        Some(route) if !route.is_empty() => route.to_string(),
        _ => UNMATCHED_ROUTE.to_string(),
    }
}

pub fn excluded(path: &[u8]) -> bool {
    path == b"/health"
        || path == b"/metrics"
        || path == b"/admin"
        || path.starts_with(b"/admin/")
        || path == b"/monitoring"
        || path.starts_with(b"/monitoring/")
}

pub fn kind(method: impl AsRef<[u8]>, route: impl AsRef<[u8]>) -> &'static str {
    let method = method.as_ref();
    let route = route.as_ref();

    if route.starts_with(b"/kv/") {
        if method == METHOD_GET.as_bytes() {
            return metrics::KIND_KV_GET;
        }
        if method == METHOD_PUT.as_bytes() {
            return metrics::KIND_KV_SET;
        }
        if method == METHOD_DELETE.as_bytes() {
            return metrics::KIND_KV_DELETE;
        }
        return "";
    }

    if route == b"/api/query" {
        return metrics::KIND_DOC_QUERY;
    }

    if route == b"/api/tx/{txId}/commit" {
        return metrics::KIND_DOC_WRITE;
    }

    if route.starts_with(b"/api/{entity}") {
        if method == METHOD_GET.as_bytes() {
            return metrics::KIND_DOC_READ;
        }
        return metrics::KIND_DOC_WRITE;
    }

    ""
}

pub fn middleware(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        let start = Instant::now();
        let request_id = request_id_for(ctx);
        ctx.set_user_value(REQUEST_ID_KEY, request_id.clone());

        next(ctx);

        if let Some(guard) = ctx.user_value::<Arc<Guard>>(GUARD_KEY) {
            guard.release_memory();
        }

        ctx.response.header.set(REQUEST_ID_HEADER, &request_id);
        finish(ctx, start, &request_id);
    })
}

fn request_id_for(ctx: &RequestCtx) -> String {
    let id = ctx.request.header.peek(REQUEST_ID_HEADER);
    if valid_request_id(id) {
        return String::from_utf8_lossy(id).into_owned();
    }

    let n = ID_COUNTER.fetch_add(1, Ordering::SeqCst) + 1;
    format!("{}-{}", id_prefix(), strconv::format_uint(n, 36))
}

fn valid_request_id(id: &[u8]) -> bool {
    if id.is_empty() || id.len() > MAX_REQUEST_ID_LEN {
        return false;
    }

    id.iter()
        .all(|&c| c.is_ascii_alphanumeric() || c == b'-' || c == b'_' || c == b'.')
}

pub fn defer(ctx: &mut RequestCtx) {
    ctx.set_user_value(DEFERRED_KEY, true);
}

pub fn finish_deferred(ctx: &mut RequestCtx) {
    ctx.remove_user_value(DEFERRED_KEY);
    let request_id = request_id(ctx);
    finish(ctx, ctx.time(), &request_id);
}

fn joined(parts: &[&[u8]]) -> Vec<u8> {
    let mut out = Vec::new();
    for (i, p) in parts.iter().enumerate() {
        if i > 0 {
            out.push(b' ');
        }
        out.extend_from_slice(p);
    }
    out
}

fn finish(ctx: &RequestCtx, start: Instant, request_id: &str) {
    if ctx.has_user_value(DEFERRED_KEY) {
        return;
    }

    let d = start.elapsed();
    let status = i64::from(ctx.response.status_code());
    let method = ctx.method();
    let route = route(ctx);
    let excluded = excluded(ctx.path());
    let user = security::get_current_username(ctx);
    let duration_ms = log::since(start);

    if !excluded {
        metrics::observe_http(method, &route, status, d, kind(method, &route));
    }

    if let Some(reason) = ctx.user_value::<String>(REJECTED_KEY) {
        let key = joined(&[method, route.as_bytes(), reason.as_bytes()]);
        let (suppressed, ok) = throttles().rejected.allow(&key);
        if ok {
            let mut args: Vec<Attr<'_>> = vec![
                ("method", Value::Str(method)),
                ("route", route.to_value()),
                ("status", status.to_value()),
                ("reason", reason.to_value()),
                ("request_id", request_id.to_value()),
            ];
            if !user.is_empty() {
                args.push(("user", user.to_value()));
            }
            HTTP_LOG.warn("request rejected", &log::with_suppressed(&args, suppressed));
        }
    } else if status >= i64::from(STATUS_INTERNAL_SERVER_ERROR) {
        if !ctx.has_user_value(LOGGED_KEY) {
            let mut args: Vec<Attr<'_>> = vec![
                ("method", Value::Str(method)),
                ("route", route.to_value()),
                ("status", status.to_value()),
                ("request_id", request_id.to_value()),
                ("duration_ms", duration_ms.to_value()),
            ];
            if !user.is_empty() {
                args.push(("user", user.to_value()));
            }
            if let Some(err) = error(ctx) {
                args.push(("err", Value::Str(err)));
            }
            HTTP_LOG.error("request failed", &args);
        }
    } else if status == i64::from(STATUS_UNAUTHORIZED) {
        unauthorized(ctx, method, &route, request_id);
    } else if status == i64::from(STATUS_FORBIDDEN) {
        forbidden(ctx, method, &route, &user, request_id);
    } else if status >= 400 && log::enabled(Level::DEBUG) {
        HTTP_LOG.debug(
            "client error",
            &[
                ("method", Value::Str(method)),
                ("route", route.to_value()),
                ("status", status.to_value()),
                ("request_id", request_id.to_value()),
            ],
        );
    }

    let Some(cfg) = globals::config() else {
        return;
    };
    if excluded {
        return;
    }

    let slow_ms = cfg.log.slow_request_ms;
    let elapsed = i64::try_from(d.as_nanos()).unwrap_or(i64::MAX);
    if slow_ms > 0 && elapsed >= slow_ms.wrapping_mul(1_000_000) {
        metrics::slow_request();
        let key = joined(&[method, route.as_bytes()]);
        let (suppressed, ok) = throttles().slow.allow(&key);
        if ok {
            let entity = ctx.param("entity");
            let mut args: Vec<Attr<'_>> = vec![
                ("method", Value::Str(method)),
                ("route", route.to_value()),
                ("status", status.to_value()),
                ("duration_ms", duration_ms.to_value()),
                ("request_id", request_id.to_value()),
            ];
            if !entity.is_empty() {
                args.push(("entity", Value::Str(entity)));
            }
            HTTP_LOG.warn("slow request", &log::with_suppressed(&args, suppressed));
        }
    }

    if cfg.log.access_log {
        let mut args: Vec<Attr<'_>> = vec![
            ("method", Value::Str(method)),
            ("route", route.to_value()),
            ("status", status.to_value()),
            ("duration_ms", duration_ms.to_value()),
        ];
        if ctx.response.is_body_stream() {
            args.push(("streamed", true.to_value()));
            let n = ctx.response.header.content_length();
            if n >= 0 {
                args.push(("bytes", Value::Int(n)));
            }
        } else {
            let n = i64::try_from(ctx.response.body_bytes().len()).unwrap_or(i64::MAX);
            args.push(("bytes", Value::Int(n)));
        }
        let remote_addr = security::login_source_from_request(ctx).to_string();
        args.push(("remote_addr", remote_addr.to_value()));
        args.push(("request_id", request_id.to_value()));
        if !user.is_empty() {
            args.push(("user", user.to_value()));
        }
        HTTP_LOG.info("request", &args);
    }
}

fn credentials_presented(ctx: &RequestCtx) -> bool {
    !ctx.request.header.peek(HEADER_AUTHORIZATION).is_empty()
        || !ctx.request.header.cookie(SESSION_COOKIE_NAME).is_empty()
}

fn unauthorized(ctx: &RequestCtx, method: &[u8], route: &str, request_id: &str) {
    if QUIET_UNAUTHORIZED_ROUTES.contains(&route) || !credentials_presented(ctx) {
        if log::enabled(Level::DEBUG) {
            SECURITY_LOG.debug(
                "unauthenticated request",
                &[
                    ("method", Value::Str(method)),
                    ("route", route.to_value()),
                    ("request_id", request_id.to_value()),
                ],
            );
        }
        return;
    }

    let addr = security::login_source_from_request(ctx).to_string();
    metrics::auth_failure(&addr);

    let (suppressed, ok) = throttles().unauthorized.allow(&addr);
    if ok {
        let auth_mode = metrics::auth_mode();
        let args: [Attr<'_>; 5] = [
            ("method", Value::Str(method)),
            ("route", route.to_value()),
            ("auth_mode", auth_mode.to_value()),
            ("remote_addr", addr.to_value()),
            ("request_id", request_id.to_value()),
        ];
        SECURITY_LOG.warn(
            "unauthorized request",
            &log::with_suppressed(&args, suppressed),
        );
    }
}

fn forbidden(ctx: &RequestCtx, method: &[u8], route: &str, user: &[u8], request_id: &str) {
    let addr = security::login_source_from_request(ctx).to_string();
    let key = joined(&[addr.as_bytes(), route.as_bytes()]);
    let (suppressed, ok) = throttles().forbidden.allow(&key);
    if ok {
        let mut args: Vec<Attr<'_>> = vec![
            ("method", Value::Str(method)),
            ("route", route.to_value()),
            ("remote_addr", addr.to_value()),
            ("request_id", request_id.to_value()),
        ];
        if !user.is_empty() {
            args.push(("user", Value::Str(user)));
        }
        SECURITY_LOG.warn(
            "forbidden request",
            &log::with_suppressed(&args, suppressed),
        );
    }
}
