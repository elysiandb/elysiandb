use crate::acl;
use crate::engine;
use crate::gocompat::json::{self, Map};
use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::security;
use crate::transport::http::observe;

pub fn get_all_acl_for_username(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);

        return;
    }

    let username = ctx.param("user_name").to_vec();

    let mut acls: Option<Vec<Map>> = None;

    for entity in engine::list_public_entity_types() {
        let Some(acl) = acl::get_acl_entity_for_username(&entity, &username) else {
            continue;
        };

        acls.get_or_insert_with(Vec::new).push(acl.to_data_map());
    }

    match json::marshal(&acls) {
        Ok(response) => {
            ctx.set_status_code(STATUS_OK);
            ctx.set_body(response);
        }
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string(r#"{"error":"internal server error"}"#);
        }
    }
}
