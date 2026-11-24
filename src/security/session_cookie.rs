use crate::gocompat::strings;
use crate::gocompat::time::Time;
use crate::httpd::{Cookie, CookieSameSite, RequestCtx};

use super::session::Session;
use super::user_auth::SESSION_COOKIE_NAME;

pub fn request_is_secure(ctx: &RequestCtx) -> bool {
    if ctx.is_tls() {
        return true;
    }

    let header = ctx.request.header.peek("X-Forwarded-Proto");
    let proto = match header.iter().position(|&b| b == b',') {
        Some(i) => &header[..i],
        None => header,
    };

    strings::equal_fold(strings::trim_space(proto), b"https")
}

pub fn set_session_cookie(ctx: &mut RequestCtx, session: &Session) {
    write_session_cookie(ctx, session.id.as_bytes(), None);
}

pub fn clear_session_cookie(ctx: &mut RequestCtx) {
    write_session_cookie(ctx, b"", Some(Time::from_unix(0, 0)));
}

fn write_session_cookie(ctx: &mut RequestCtx, value: &[u8], expire: Option<Time>) {
    let mut cookie = Cookie::new();

    cookie.set_key(SESSION_COOKIE_NAME);
    cookie.set_value(value);
    cookie.set_http_only(true);
    cookie.set_same_site(CookieSameSite::StrictMode);
    cookie.set_path("/");
    cookie.set_secure(request_is_secure(ctx));
    if expire.is_some() {
        cookie.set_expire(expire);
    }

    ctx.response.header.set_cookie(&cookie);
}
