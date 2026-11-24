use std::fmt;

use crate::httpd::RequestCtx;

pub const REQUEST_ID_HEADER: &str = "X-Request-Id";
pub const ERROR_KEY: &str = "elysiandb.error";
pub const REQUEST_ID_KEY: &str = "elysiandb.request_id";

pub fn set_error(ctx: &mut RequestCtx, err: &dyn fmt::Display) {
    ctx.set_user_value(ERROR_KEY, err.to_string());
}

pub fn error(ctx: &RequestCtx) -> Option<&str> {
    ctx.user_value::<String>(ERROR_KEY).map(String::as_str)
}

pub fn request_id(ctx: &RequestCtx) -> String {
    ctx.user_value::<String>(REQUEST_ID_KEY)
        .cloned()
        .unwrap_or_default()
}
