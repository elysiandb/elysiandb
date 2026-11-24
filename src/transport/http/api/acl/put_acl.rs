use std::collections::BTreeMap;

use crate::acl;
use crate::engine;
use crate::gocompat::json;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_OK};
use crate::json_struct;
use crate::kv;
use crate::security;
use crate::transport::http::observe;

pub(crate) const CORE_ENTITY_ERROR: &str = r#"{"error":"core entities have no ACL: users, ACLs and hooks are managed by admin users only"}"#;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct AclPayload {
        pub permissions: Option<BTreeMap<String, bool>> => "permissions",
    }
}

pub fn update_acl_for_username_and_entity_controller(ctx: &mut RequestCtx) {
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

    let mut payload = AclPayload::default();
    if json::unmarshal(ctx.post_body(), &mut payload).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"invalid json body"}"#);
        return;
    }

    let mut perms = acl::new_permissions();
    for (k, v) in payload.permissions.iter().flatten() {
        if let Some(p) = acl::string_to_permission(k) {
            perms[p as usize] = *v;
        }
    }

    if acl::update_acl_entity_for_username(&entity, &username, perms).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"unable to update acl"}"#);
        return;
    }

    observe::audit(
        ctx,
        "acl changed",
        kv!["user" => username, "entity" => entity],
    );
    ctx.set_status_code(STATUS_OK);
    ctx.set_body_string(r#"{"status":"ACL updated successfully"}"#);
}
