use std::sync::Arc;

use crate::crypto::subtle;
use crate::globals;
use crate::gocompat::json::{self, Encode};
use crate::gocompat::strconv;
use crate::gocompat::strings;
use crate::gocompat::time::{Duration, Time};
use crate::httpd::{
    HEADER_AUTHORIZATION, Handler, RequestCtx, STATUS_BAD_REQUEST, STATUS_INTERNAL_SERVER_ERROR,
    STATUS_NOT_FOUND, STATUS_OK, STATUS_UNAUTHORIZED,
};
use crate::json_struct;
use crate::kv;
use crate::log::{self, Level, Query};
use crate::metrics;
use crate::transport::http::observe;

pub const MAX_LEVEL_MINUTES: i64 = 24 * 60;

const BEARER_PREFIX: &[u8] = b"Bearer ";
const LOG_BUFFER_DISABLED: &str = "the log buffer is disabled: set log.bufferLines above 0";

fn write_json<T: Encode + ?Sized>(ctx: &mut RequestCtx, status: u16, body: &T) {
    let data = match json::marshal(body) {
        Ok(data) => data,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            return;
        }
    };

    ctx.set_content_type("application/json");
    ctx.set_status_code(status);
    ctx.set_body(data);
}

fn write_error(ctx: &mut RequestCtx, status: u16, message: &str) {
    let mut data = b"{\"error\":".to_vec();
    json::append_string(&mut data, message.as_bytes(), true);
    data.push(b'}');

    ctx.set_content_type("application/json");
    ctx.set_status_code(status);
    ctx.set_body(data);
}

pub fn overview_controller(ctx: &mut RequestCtx) {
    write_json(ctx, STATUS_OK, &metrics::current_overview());
}

pub fn details_controller(ctx: &mut RequestCtx) {
    write_json(ctx, STATUS_OK, &metrics::current_details());
}

pub fn series_controller(ctx: &mut RequestCtx) {
    let mut range_name = String::from_utf8_lossy(ctx.query_args().peek("range")).into_owned();
    if range_name.is_empty() {
        range_name = "15m".to_string();
    }

    let Ok(since) = optional_int(ctx, "since") else {
        write_error(
            ctx,
            STATUS_BAD_REQUEST,
            "since must be a unix time in milliseconds",
        );
        return;
    };

    let (mut series, ok) = metrics::series(&range_name, since);
    if !ok {
        write_error(ctx, STATUS_BAD_REQUEST, "range must be 15m, 1h or 24h");
        return;
    }

    if let Some(cfg) = globals::config() {
        series.slow_request_ms = cfg.log.slow_request_ms;
    }

    write_json(ctx, STATUS_OK, &series);
}

pub fn logs_controller(ctx: &mut RequestCtx) {
    if !log::buffer_enabled() {
        write_error(ctx, STATUS_NOT_FOUND, LOG_BUFFER_DISABLED);
        return;
    }

    let query = match log_query(ctx) {
        Ok(query) => query,
        Err(message) => {
            write_error(ctx, STATUS_BAD_REQUEST, &message);
            return;
        }
    };

    write_json(ctx, STATUS_OK, &log::logs(&query));
}

pub fn logs_download_controller(ctx: &mut RequestCtx) {
    if !log::buffer_enabled() {
        write_error(ctx, STATUS_NOT_FOUND, LOG_BUFFER_DISABLED);
        return;
    }

    let mut query = match log_query(ctx) {
        Ok(query) => query,
        Err(message) => {
            write_error(ctx, STATUS_BAD_REQUEST, &message);
            return;
        }
    };

    query.limit = log::MAX_QUERY_LIMIT;
    let mut b: Vec<u8> = Vec::new();
    let mut before = query.before;

    loop {
        query.before = before;
        let page = log::logs(&query);
        for entry in &page.entries {
            let line = json::marshal(entry.as_ref()).unwrap_or_default();
            b.extend_from_slice(&line);
            b.push(b'\n');
        }

        let Some(last) = page.entries.last() else {
            break;
        };
        if !page.has_more {
            break;
        }

        before = last.seq;
    }

    let name = format!(
        "elysiandb-logs-{}.ndjson",
        Time::now().utc().format("20060102-150405")
    );
    ctx.response.header.set(
        "Content-Disposition",
        format!("attachment; filename=\"{name}\""),
    );
    ctx.set_content_type("application/x-ndjson");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(b);
}

fn log_query(ctx: &RequestCtx) -> Result<Query, String> {
    let args = ctx.query_args();

    let mut level = log::parse_level(args.peek("level")).map_err(|e| e.to_string())?;

    if args.peek("level").is_empty() {
        level = Level::DEBUG;
    }

    let mut query = Query {
        min_level: level,
        component: strings::trim_space(args.peek("component")).to_vec(),
        text: args.peek("q").to_vec(),
        user: strings::trim_space(args.peek("user")).to_vec(),
        request_id: strings::trim_space(args.peek("request_id")).to_vec(),
        ..Query::default()
    };

    for (name, is_since) in [("since", true), ("until", false)] {
        let v = optional_int(ctx, name)
            .map_err(|_| format!("{name} must be a unix time in milliseconds"))?;
        if v > 0 {
            let t = Time::from_unix_milli(v);
            if is_since {
                query.since = t;
            } else {
                query.until = t;
            }
        }
    }

    for (name, is_before) in [("before", true), ("after", false)] {
        let v = match optional_int(ctx, name) {
            Ok(v) if v >= 0 => v as u64,
            _ => return Err(format!("{name} must be a sequence number")),
        };
        if is_before {
            query.before = v;
        } else {
            query.after = v;
        }
    }

    query.limit = match optional_int(ctx, "limit") {
        Ok(limit) if limit >= 0 => limit,
        _ => {
            return Err(format!(
                "limit must be between 1 and {}",
                log::MAX_QUERY_LIMIT
            ));
        }
    };

    Ok(query)
}

fn optional_int(ctx: &RequestCtx, name: &str) -> Result<i64, ()> {
    let raw = ctx.query_args().peek(name);
    if raw.is_empty() {
        return Ok(0);
    }

    strconv::parse_int(raw, 10, 64).map_err(|_| ())
}

pub fn get_log_level_controller(ctx: &mut RequestCtx) {
    write_json(ctx, STATUS_OK, &log::current_level());
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct LevelRequest {
        pub level: String => "level",
        pub minutes: i64 => "minutes",
    }
}

pub fn put_log_level_controller(ctx: &mut RequestCtx) {
    let mut req = LevelRequest::default();
    if json::unmarshal(ctx.post_body(), &mut req).is_err() {
        write_error(ctx, STATUS_BAD_REQUEST, "invalid request body");
        return;
    }

    let level = match log::parse_level(req.level.as_bytes()) {
        Ok(level) if !strings::trim_space(req.level.as_bytes()).is_empty() => level,
        _ => {
            write_error(
                ctx,
                STATUS_BAD_REQUEST,
                "level must be debug, info, warn or error",
            );
            return;
        }
    };

    if level != log::configured_level() && (req.minutes < 1 || req.minutes > MAX_LEVEL_MINUTES) {
        write_error(
            ctx,
            STATUS_BAD_REQUEST,
            &format!("minutes must be between 1 and {MAX_LEVEL_MINUTES}"),
        );
        return;
    }

    let info = log::set_level_for(
        level,
        Duration(req.minutes.wrapping_mul(Duration::MINUTE.0)),
    );
    observe::audit(
        ctx,
        "log level changed",
        kv![
            "level" => info.level,
            "configured" => info.configured,
            "minutes" => req.minutes,
        ],
    );

    write_json(ctx, STATUS_OK, &info);
}

pub fn prometheus_controller(ctx: &mut RequestCtx) {
    ctx.set_content_type("text/plain; version=0.0.4; charset=utf-8");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(metrics::prometheus());
}

pub fn prometheus_auth(admin_auth: impl Fn(Handler) -> Handler, next: Handler) -> Handler {
    let guarded = admin_auth(next.clone());

    Arc::new(move |ctx: &mut RequestCtx| {
        let token = globals::config()
            .map(|cfg| cfg.monitoring.prometheus.token.clone())
            .unwrap_or_default();
        if token.is_empty() {
            guarded(ctx);
            return;
        }

        let header = ctx.request.header.peek(HEADER_AUTHORIZATION);
        let presented = header.strip_prefix(BEARER_PREFIX);
        match presented {
            Some(p) if subtle::constant_time_compare(p, token.as_bytes()) == 1 => {}
            _ => {
                ctx.set_status_code(STATUS_UNAUTHORIZED);
                return;
            }
        }

        next(ctx);
    })
}
