use std::sync::Arc;

use crate::gocompat::{strings, url};
use crate::httpd::{
    Handler, METHOD_GET, METHOD_HEAD, METHOD_OPTIONS, RequestCtx, STATUS_FORBIDDEN,
};

use super::cors::{cors, origin_is_allowed};

pub const ADMIN_UI_CONTENT_SECURITY_POLICY: &str = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; worker-src 'self' blob:; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'";
pub const API_CONTENT_SECURITY_POLICY: &str = "default-src 'none'; frame-ancestors 'none'";

const ADMIN_UI_PATH_PREFIX: &[u8] = b"/admin";

pub fn protect(next: Handler) -> Handler {
    cors(reject_cross_origin_writes(security_headers(next)))
}

pub fn security_headers(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        next(ctx);

        let admin_ui = is_admin_ui_path(ctx.path());
        let header = &mut ctx.response.header;
        header.set("X-Content-Type-Options", "nosniff");
        header.set("X-Frame-Options", "DENY");
        header.set("Referrer-Policy", "no-referrer");

        if admin_ui {
            header.set("Content-Security-Policy", ADMIN_UI_CONTENT_SECURITY_POLICY);
            return;
        }

        header.set("Content-Security-Policy", API_CONTENT_SECURITY_POLICY);
        if header.peek("Cache-Control").is_empty() {
            header.set("Cache-Control", "no-store");
        }
    })
}

fn is_admin_ui_path(path: &[u8]) -> bool {
    if !path.starts_with(ADMIN_UI_PATH_PREFIX) {
        return false;
    }

    path.len() == ADMIN_UI_PATH_PREFIX.len() || path[ADMIN_UI_PATH_PREFIX.len()] == b'/'
}

pub fn reject_cross_origin_writes(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        if cross_origin_write_is_rejected(ctx) {
            ctx.response.header.set("Content-Type", "application/json");
            ctx.set_status_code(STATUS_FORBIDDEN);
            ctx.set_body_string(r#"{"error":"cross-origin request rejected"}"#);
            return;
        }

        next(ctx);
    })
}

pub fn cross_origin_write_is_rejected(ctx: &RequestCtx) -> bool {
    let method = ctx.method();
    if method == METHOD_GET.as_bytes()
        || method == METHOD_HEAD.as_bytes()
        || method == METHOD_OPTIONS.as_bytes()
    {
        return false;
    }

    let origin = ctx.request.header.peek("Origin");
    if !origin.is_empty() && origin_is_allowed(origin) {
        return false;
    }

    match ctx.request.header.peek("Sec-Fetch-Site") {
        b"" => {}
        b"same-origin" | b"none" => return false,
        _ => return true,
    }

    if origin.is_empty() {
        return false;
    }

    match url::parse(origin) {
        Ok(parsed) if !parsed.host.is_empty() => !strings::equal_fold(&parsed.host, ctx.host()),
        _ => true,
    }
}
