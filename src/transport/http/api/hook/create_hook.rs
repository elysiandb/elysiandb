use crate::gocompat::json;
use crate::hook::{self, Hook};
use crate::httpd::{
    RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK,
};
use crate::log;
use crate::security;
use crate::transport::http::observe;

use super::{FORBIDDEN, HookRecord, INTERNAL_ERROR, INVALID_BODY};

pub fn create_hook_for_entity_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(FORBIDDEN);

        return;
    }

    let entity = String::from_utf8_lossy(ctx.param("entity")).into_owned();
    let mut record = HookRecord::default();
    if json::unmarshal(ctx.post_body(), &mut record).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(INVALID_BODY);

        return;
    }

    record.entity = entity.clone();
    let mut hook_data: Hook = record.into();

    if let Err(err) = hook::create_hook(&mut hook_data) {
        observe::set_error(ctx, &err);
        ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
        ctx.set_body_string(INTERNAL_ERROR);

        return;
    }

    let response = match json::marshal(&HookRecord::from(&hook_data)) {
        Ok(r) => r,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string(INTERNAL_ERROR);

            return;
        }
    };

    observe::audit(
        ctx,
        "hook created",
        &[
            ("entity", log::Value::Str(entity.as_bytes())),
            ("hook_id", log::Value::Str(hook_data.id.as_bytes())),
            ("event", log::Value::Str(hook_data.event.as_bytes())),
            ("bypass_acl", log::Value::Bool(hook_data.bypass_acl)),
            ("enabled", log::Value::Bool(hook_data.enabled)),
        ],
    );
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response);
}
