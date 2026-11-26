use crate::acl;
use crate::engine;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_OK};
use crate::kv;
use crate::security;
use crate::transport::http::observe;

use super::put_acl::CORE_ENTITY_ERROR;

pub fn set_default_acl_for_username_and_entity_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);
        return;
    }

    let entity = ctx.param("entity").to_vec();
    let username = ctx.param("user_name").to_vec();

    if engine::is_core_entity_type(&entity) {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(CORE_ENTITY_ERROR);
        return;
    }

    if acl::reset_acl_entity_to_default(&entity, &username).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"unable to reset acl"}"#);
        return;
    }

    observe::audit(
        ctx,
        "acl reset to default",
        kv!["user" => username, "entity" => entity],
    );
    ctx.set_status_code(STATUS_OK);
    ctx.set_body_string(r#"{"status":"ACL reset to default"}"#);
}
