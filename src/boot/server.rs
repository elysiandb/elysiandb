use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use crate::datastore::{self, ERR_WRITE_FAILED};
use crate::gocompat::json::append_string;
use crate::httpd::{
    Handler, RequestCtx, STATUS_INSUFFICIENT_STORAGE, STATUS_SERVICE_UNAVAILABLE, Server,
};
use crate::httprouter::Router;
use crate::kv;
use crate::log::{self, Logger};
use crate::security;
use crate::storage;
use crate::sync::Mutex;
use crate::transport::http::observe;

pub(crate) const HTTP_LOG: Logger = log::for_component("http");

pub(crate) static STOPPING: AtomicBool = AtomicBool::new(false);
pub(crate) static HTTP_SERVER: Mutex<Option<Server>> = Mutex::new(None);

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
