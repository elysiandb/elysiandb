use crate::gocompat::json;
use crate::hook;
use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::security;
use crate::transport::http::observe;

use super::{FORBIDDEN, HookRecord, INTERNAL_ERROR};

pub fn get_hooks_for_entity_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(FORBIDDEN);

        return;
    }

    let entity = String::from_utf8_lossy(ctx.param("entity")).into_owned();

    let hooks = match hook::get_hooks_for_entity(&entity) {
        Ok(list) => list,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string(INTERNAL_ERROR);

            return;
        }
    };

    let records: Vec<HookRecord> = hooks.iter().map(HookRecord::from).collect();
    match json::marshal(&records) {
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
