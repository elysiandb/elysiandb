use std::sync::Arc;

use crate::api::{self, Guard, GuardError};
use crate::globals;
use crate::gocompat::json::Map;
use crate::httpd::{RequestCtx, STATUS_SERVICE_UNAVAILABLE};
use crate::metrics;
use crate::query::{Filters, Query};
use crate::transport::http::observe::{self, GUARD_KEY};

use super::disconnect::disconnect_probe;
use super::response::send_deadline_exceeded;

pub const STATUS_CLIENT_CLOSED_REQUEST: u16 = 499;

pub fn request_guard(ctx: &mut RequestCtx) -> Arc<Guard> {
    if let Some(g) = ctx.user_value::<Arc<Guard>>(GUARD_KEY) {
        return g.clone();
    }

    let mut abandoned = None;
    if globals::config().is_none_or(|cfg| cfg.api.query.cancels_abandoned_requests()) {
        abandoned = disconnect_probe(ctx.conn());
    }

    let g = Arc::new(Guard::new(api::deadline_from_config(), abandoned));
    ctx.set_user_value(GUARD_KEY, g.clone());

    g
}

pub fn send_guard_error(ctx: &mut RequestCtx, err: GuardError) {
    ctx.response.header.set("Content-Type", "application/json");

    match err {
        GuardError::Busy => {
            observe::set_rejected(ctx, "server busy");
            ctx.response.header.set("Retry-After", "1");
            ctx.set_status_code(STATUS_SERVICE_UNAVAILABLE);
            ctx.set_body_string(r#"{"error":"server busy"}"#);
        }
        GuardError::Abandoned => {
            observe::set_rejected(ctx, "client gone");
            metrics::request_abandoned();
            ctx.set_status_code(STATUS_CLIENT_CLOSED_REQUEST);
            ctx.set_body_string(r#"{"error":"request abandoned by the client"}"#);
        }
        GuardError::Deadline => send_deadline_exceeded(ctx),
    }
}

pub fn guard_stopped(ctx: &mut RequestCtx, guard: &Guard) -> bool {
    if let Some(err) = guard.err() {
        send_guard_error(ctx, err);
        return true;
    }

    false
}

#[allow(clippy::too_many_arguments)]
pub fn list_entities(
    guard: &Guard,
    entity: &[u8],
    limit: i64,
    offset: i64,
    sort_field: &[u8],
    sort_ascending: bool,
    filters: &Filters,
    search: &[u8],
    includes: &[u8],
) -> Vec<Map> {
    api::list_entities_guarded(
        entity,
        limit,
        offset,
        sort_field,
        sort_ascending,
        Some(filters),
        search,
        includes,
        Some(guard),
    )
    .0
}

pub fn execute_query(guard: &Guard, q: &Query) -> (Vec<Map>, Option<GuardError>) {
    api::execute_query_guarded(q, Some(guard))
}
