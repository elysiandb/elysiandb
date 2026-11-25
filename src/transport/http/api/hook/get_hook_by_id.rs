use crate::gocompat::json;
use crate::hook;
use crate::httpd::{
    RequestCtx, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_NOT_FOUND, STATUS_OK,
};
use crate::security;
use crate::transport::http::observe;

use super::{FORBIDDEN, HookRecord, INTERNAL_ERROR, NOT_FOUND};

pub fn get_hook_by_id_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(FORBIDDEN);

        return;
    }

    let id = String::from_utf8_lossy(ctx.param("id")).into_owned();

    let found = match hook::get_hook_by_id(&id) {
        Ok(h) => h,
        Err(_) => {
            ctx.set_status_code(STATUS_NOT_FOUND);
            ctx.set_body_string(NOT_FOUND);

            return;
        }
    };

    match json::marshal(&HookRecord::from(&found)) {
        Ok(response) => {
            ctx.set_status_code(STATUS_OK);
            ctx.set_body(response);
        }
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string(INTERNAL_ERROR);
        }
    }
}
