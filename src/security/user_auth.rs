use std::sync::Arc;

use crate::httpd::{Handler, RequestCtx, STATUS_UNAUTHORIZED};
use crate::sync::RwLock;

use super::session::get_session;

pub const SESSION_COOKIE_NAME: &str = "edb_session";
pub const USERNAME_KEY: &str = "username";
pub const ROLE_KEY: &str = "role";

pub type Middleware = Arc<dyn Fn(Handler) -> Handler + Send + Sync>;

static USER_AUTH: RwLock<Option<Middleware>> = RwLock::new(None);

pub fn user_auth(next: Handler) -> Handler {
    let replaced = USER_AUTH.read().clone();
    match replaced {
        Some(f) => f(next),
        None => default_user_auth(next),
    }
}

pub fn set_user_auth(f: Option<Middleware>) -> Option<Middleware> {
    std::mem::replace(&mut *USER_AUTH.write(), f)
}

fn default_user_auth(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        let id = ctx.request.header.cookie(SESSION_COOKIE_NAME).to_vec();
        if id.is_empty() {
            ctx.set_status_code(STATUS_UNAUTHORIZED);
            return;
        }

        let session = match get_session(&id) {
            Ok(Some(session)) => session,
            _ => {
                ctx.set_status_code(STATUS_UNAUTHORIZED);
                return;
            }
        };

        ctx.set_user_value(USERNAME_KEY, session.username);
        ctx.set_user_value(ROLE_KEY, session.role);

        next(ctx);
    })
}
