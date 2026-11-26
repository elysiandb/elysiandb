use std::sync::Arc;

use crate::gocompat::strings;
use crate::httpd::{Handler, METHOD_OPTIONS, RequestCtx, STATUS_NO_CONTENT};

use super::config;

pub fn cors(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        let origin = ctx.request.header.peek("Origin").to_vec();

        if !origin.is_empty() && origin_is_allowed(&origin) {
            let header = &mut ctx.response.header;
            header.set("Access-Control-Allow-Origin", &origin);
            header.set("Vary", "Origin");
            header.set("Access-Control-Allow-Credentials", "true");
            header.set(
                "Access-Control-Allow-Headers",
                "Content-Type, Authorization",
            );
            header.set(
                "Access-Control-Allow-Methods",
                "GET, POST, PUT, DELETE, OPTIONS",
            );
        }

        if ctx.method() == METHOD_OPTIONS.as_bytes() {
            ctx.set_status_code(STATUS_NO_CONTENT);
            return;
        }

        next(ctx);
    })
}

pub fn origin_is_allowed(origin: &[u8]) -> bool {
    let cfg = config();
    for allowed in cfg.server.http.cors.allowed_origins.0.iter().flatten() {
        let allowed = strings::trim_space(allowed.as_bytes());
        let allowed = allowed.strip_suffix(b"/").unwrap_or(allowed);
        if allowed == b"*" || strings::equal_fold(allowed, origin) {
            return true;
        }
    }

    false
}
