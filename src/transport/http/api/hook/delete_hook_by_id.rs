use crate::hook;
use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK};
use crate::log;
use crate::security;
use crate::transport::http::observe;

use super::{FORBIDDEN, NOT_FOUND};

pub fn delete_hook_by_id_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(FORBIDDEN);

        return;
    }

    let id = String::from_utf8_lossy(ctx.param("id")).into_owned();

    if hook::delete_hook(&id).is_err() {
        ctx.set_status_code(STATUS_NOT_FOUND);
        ctx.set_body_string(NOT_FOUND);

        return;
    }

    observe::audit(
        ctx,
        "hook deleted",
        &[("hook_id", log::Value::Str(id.as_bytes()))],
    );
    ctx.set_status_code(STATUS_OK);
}
