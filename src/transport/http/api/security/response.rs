use crate::gocompat::json::{self, Encode, Map, Value};
use crate::httpd::{RequestCtx, STATUS_INTERNAL_SERVER_ERROR};
use crate::json_struct;
use crate::security::SecurityError;
use crate::transport::http::observe;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct UserView {
        pub username: String => "username",
        pub role: String => "role",
    }
}

pub(crate) fn write_json<T: Encode + ?Sized>(ctx: &mut RequestCtx, status: u16, body: &T) {
    let data = json::marshal(body).unwrap_or_default();
    ctx.set_status_code(status);
    ctx.set_body(data);
}

pub(crate) fn error_body(message: &[u8]) -> Vec<u8> {
    let mut out = b"{\"error\":".to_vec();
    json::append_string(&mut out, message, true);
    out.push(b'}');
    out
}

pub(crate) fn write_error(ctx: &mut RequestCtx, status: u16, err: &SecurityError) {
    if status >= STATUS_INTERNAL_SERVER_ERROR {
        observe::set_error(ctx, err);
    }

    ctx.set_status_code(status);
    ctx.set_body(error_body(&err.message()));
}

fn string_of(user: &Map, key: &str) -> String {
    match user.get(key) {
        Some(Value::String(s)) => s.clone(),
        _ => String::new(),
    }
}

pub(crate) fn user_view_from(user: &Map) -> UserView {
    UserView {
        username: string_of(user, "username"),
        role: string_of(user, "role"),
    }
}
