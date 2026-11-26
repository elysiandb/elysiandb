use std::net::IpAddr;
use std::sync::Arc;

use crate::gocompat::strings;
use crate::httpd::{Handler, RequestCtx, STATUS_FORBIDDEN, STATUS_UNAUTHORIZED};
use crate::transport::tcp::tcp_routing::{self, Security};

use super::basic_user::{Role, check_basic_authentication};
use super::config;
use super::login::authenticate_user_from;
use super::login_source::{LoginSource, login_source_from_ip};
use super::token::{check_token_authentication, token_matches};
use super::user_auth::{ROLE_KEY, user_auth};

pub fn authentication_is_enabled() -> bool {
    config().security.authentication.enabled
}

fn mode_is_enabled(mode: &str) -> bool {
    let cfg = config();
    cfg.security.authentication.enabled && cfg.security.authentication.mode == mode
}

pub fn basic_authentication_is_enabled() -> bool {
    mode_is_enabled("basic")
}

pub fn token_authentication_is_enabled() -> bool {
    mode_is_enabled("token")
}

pub fn user_authentication_is_enabled() -> bool {
    mode_is_enabled("user")
}

pub fn credentials_are_required() -> bool {
    authentication_is_enabled()
}

pub fn authenticate_credentials(credentials: &[u8], source: LoginSource) -> bool {
    if !credentials_are_required() {
        return true;
    }

    let credentials = strings::trim_space(credentials);

    if token_authentication_is_enabled() {
        return token_matches(credentials);
    }

    if !basic_authentication_is_enabled() && !user_authentication_is_enabled() {
        return false;
    }

    let Some(i) = credentials.iter().position(|&b| b == b' ') else {
        return false;
    };

    let (user, _, ok) = authenticate_user_from(&credentials[..i], &credentials[i + 1..], source);
    if !ok {
        return false;
    }

    !user_authentication_is_enabled() || user.is_some_and(|u| u.role.is_admin())
}

pub fn authenticate(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        if !authentication_is_enabled() {
            next(ctx);
            return;
        }

        if basic_authentication_is_enabled() && !check_basic_authentication(ctx) {
            ctx.response.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }

        if token_authentication_is_enabled() && !check_token_authentication(ctx) {
            ctx.response.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }

        if user_authentication_is_enabled() {
            user_auth(next.clone())(ctx);
            return;
        }

        if !basic_authentication_is_enabled() && !token_authentication_is_enabled() {
            ctx.response.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }

        next(ctx);
    })
}

pub fn authenticate_admin(next: Handler) -> Handler {
    authenticate(Arc::new(move |ctx: &mut RequestCtx| {
        if user_authentication_is_enabled()
            && !ctx.user_value::<Role>(ROLE_KEY).is_some_and(Role::is_admin)
        {
            ctx.response.header.set("Content-Type", "application/json");
            ctx.set_status_code(STATUS_FORBIDDEN);
            ctx.set_body_string(r#"{"error":"admin role required"}"#);

            return;
        }

        next(ctx);
    }))
}

fn tcp_authenticate_credentials(credentials: &[u8], ip: Option<IpAddr>) -> bool {
    authenticate_credentials(credentials, login_source_from_ip(ip))
}

pub fn register() {
    tcp_routing::set_security(Security {
        credentials_are_required: Some(Arc::new(credentials_are_required)),
        token_authentication_is_enabled: Some(Arc::new(token_authentication_is_enabled)),
        authenticate_credentials: Some(Arc::new(tcp_authenticate_credentials)),
    });
}
