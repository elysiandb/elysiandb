use crate::gocompat::json;
use crate::httpd::{
    RequestCtx, STATUS_BAD_REQUEST, STATUS_INTERNAL_SERVER_ERROR, STATUS_NO_CONTENT,
    STATUS_TOO_MANY_REQUESTS, STATUS_UNAUTHORIZED,
};
use crate::json_struct;
use crate::security::{self, ByteString, Role, SESSION_COOKIE_NAME};
use crate::transport::http::observe;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct LoginRequest {
        pub username: String => "username",
        pub password: String => "password",
    }
}

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct UserResponse {
        pub username: ByteString => "username",
        pub role: Role => "role",
    }
}

pub fn login_controller(ctx: &mut RequestCtx) {
    let mut req = LoginRequest::default();
    if json::unmarshal(ctx.post_body(), &mut req).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        return;
    }

    let source = security::login_source_from_request(ctx);
    let (user, retry_after, ok) =
        security::authenticate_user_from(req.username.as_bytes(), req.password.as_bytes(), source);
    if !retry_after.is_zero() {
        ctx.response.header.set(
            "Retry-After",
            security::retry_after_seconds(retry_after).to_string(),
        );
        ctx.set_status_code(STATUS_TOO_MANY_REQUESTS);
        return;
    }

    if ok && user.as_ref().is_some_and(|u| !u.role.is_admin()) {
        security::log_failed_login(
            req.username.as_bytes(),
            source,
            security::LOGIN_FAILURE_NOT_ADMIN,
        );
    }

    let user = match user {
        Some(user) if ok && user.role.is_admin() => user,
        _ => {
            ctx.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }
    };

    let session = match security::create_session(
        user.username.as_bytes(),
        user.role.clone(),
        security::SESSION_TTL,
    ) {
        Ok(session) => session,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            return;
        }
    };

    security::set_session_cookie(ctx, &session);
    security::log_login(user.username.as_bytes(), source);

    let resp = UserResponse {
        username: ByteString::from(user.username.as_str()),
        role: user.role,
    };

    let body = json::marshal(&resp).unwrap_or_default();
    ctx.set_content_type("application/json");
    ctx.set_body(body);
}

pub fn logout_controller(ctx: &mut RequestCtx) {
    let id = ctx.request.header.cookie(SESSION_COOKIE_NAME).to_vec();
    if !id.is_empty() {
        let _ = security::delete_session(&id);
    }

    security::log_logout(&security::get_current_username(ctx));
    security::clear_session_cookie(ctx);

    ctx.set_status_code(STATUS_NO_CONTENT);
}

pub fn me_controller(ctx: &mut RequestCtx) {
    let id = ctx.request.header.cookie(SESSION_COOKIE_NAME).to_vec();
    if id.is_empty() {
        ctx.set_status_code(STATUS_UNAUTHORIZED);
        return;
    }

    let session = match security::get_session(&id) {
        Ok(Some(session)) => session,
        _ => {
            ctx.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }
    };

    let resp = UserResponse {
        username: session.username,
        role: session.role,
    };

    let body = json::marshal(&resp).unwrap_or_default();
    ctx.set_content_type("application/json");
    ctx.set_body(body);
}
