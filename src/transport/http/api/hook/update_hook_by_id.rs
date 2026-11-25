use std::collections::BTreeMap;

use crate::gocompat::json::{self, Map, Value};
use crate::hook;
use crate::httpd::{
    RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR,
    STATUS_NOT_FOUND, STATUS_OK,
};
use crate::log;
use crate::security;
use crate::transport::http::observe;

use super::{FORBIDDEN, HookRecord, INTERNAL_ERROR, INVALID_BODY, NOT_FOUND};

pub fn update_hook_by_id_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(FORBIDDEN);

        return;
    }

    let id = String::from_utf8_lossy(ctx.param("id")).into_owned();

    let mut data: Option<Map> = None;
    let decoded = json::unmarshal(ctx.post_body(), &mut data);
    let mut data = match (decoded, data) {
        (Ok(()), Some(d)) => d,
        _ => {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(INVALID_BODY);

            return;
        }
    };

    let mut hook_entity = match hook::get_hook_by_id(&id) {
        Ok(h) => h,
        Err(_) => {
            ctx.set_status_code(STATUS_NOT_FOUND);
            ctx.set_body_string(NOT_FOUND);

            return;
        }
    };

    data.insert("id".to_string(), Value::String(id.clone()));
    hook_entity.from_data_map(&data);

    if let Err(err) = hook::update_hook(&mut hook_entity) {
        let body = BTreeMap::from([("error".to_string(), err.to_string())]);
        let response = json::marshal(&body).unwrap_or_default();
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(response);

        return;
    }

    let response = match json::marshal(&HookRecord::from(&hook_entity)) {
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
        "hook updated",
        &[
            ("entity", log::Value::Str(hook_entity.entity.as_bytes())),
            ("hook_id", log::Value::Str(id.as_bytes())),
            ("event", log::Value::Str(hook_entity.event.as_bytes())),
            ("bypass_acl", log::Value::Bool(hook_entity.bypass_acl)),
            ("enabled", log::Value::Bool(hook_entity.enabled)),
        ],
    );
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response);
}
