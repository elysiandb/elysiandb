use crate::acl;
use crate::gocompat::json;
use crate::httpd::{
    RequestCtx, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_NOT_FOUND, STATUS_OK,
};
use crate::security;
use crate::transport::http::observe;

pub fn get_acl_for_username_and_entity_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);

        return;
    }

    let entity = ctx.param("entity").to_vec();
    let username = ctx.param("user_name").to_vec();

    let Some(acl) = acl::get_acl_entity_for_username(&entity, &username) else {
        ctx.set_status_code(STATUS_NOT_FOUND);
        return;
    };

    let data = acl.to_data_map();

    match json::marshal(&data) {
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
