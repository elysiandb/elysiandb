use crate::gocompat::json;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK};
use crate::json_struct;
use crate::kv;
use crate::security;
use crate::transport::http::observe;

use super::response::write_error;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct PasswordDto {
        pub password: Option<String> => "password",
    }
}

pub fn change_user_password_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    let username = ctx.param("user_name").to_vec();

    if !matches!(
        security::current_user_can_manage_user(ctx, &username),
        Ok(true)
    ) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        return;
    }

    let mut body = PasswordDto::default();
    let decoded = json::unmarshal(ctx.post_body(), &mut body);
    let password = match (decoded, body.password) {
        (Ok(()), Some(password)) => password,
        _ => {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"invalid request body"}"#);
            return;
        }
    };

    if let Err(err) = security::validate_password(password.as_bytes()) {
        write_error(ctx, STATUS_BAD_REQUEST, &err);
        return;
    }

    let kept_session_id = match security::current_session(ctx) {
        Ok(Some(current)) if current.username.as_bytes() == username.as_slice() => current.id,
        _ => String::new(),
    };

    if let Err(err) = security::change_user_password_keeping_session(
        &username,
        password.as_bytes(),
        kept_session_id.as_bytes(),
    ) {
        write_error(ctx, STATUS_NOT_FOUND, &err);
        return;
    }

    observe::audit(ctx, "user password changed", kv!["user" => username]);
    ctx.set_status_code(STATUS_OK);
}
