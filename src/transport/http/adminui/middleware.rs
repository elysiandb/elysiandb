use std::sync::Arc;

use crate::httpd::{Handler, RequestCtx, STATUS_FORBIDDEN, STATUS_UNAUTHORIZED};
use crate::security::{self, ROLE_KEY, SESSION_COOKIE_NAME, USERNAME_KEY};

pub fn admin_auth(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        if !security::authentication_is_enabled() {
            next(ctx);
            return;
        }

        if security::basic_authentication_is_enabled() {
            if !security::check_basic_authentication(ctx) {
                ctx.set_status_code(STATUS_UNAUTHORIZED);
                return;
            }

            next(ctx);
            return;
        }

        if security::token_authentication_is_enabled() {
            if !security::check_token_authentication(ctx) {
                ctx.set_status_code(STATUS_UNAUTHORIZED);
                return;
            }

            next(ctx);
            return;
        }

        if !security::user_authentication_is_enabled() {
            ctx.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }

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

        if !session.role.is_admin() {
            ctx.set_status_code(STATUS_FORBIDDEN);
            return;
        }

        ctx.set_user_value(USERNAME_KEY, session.username);
        ctx.set_user_value(ROLE_KEY, session.role);

        next(ctx);
    })
}
