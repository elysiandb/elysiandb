use crate::acl;
use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_OK};
use crate::kv;
use crate::security;
use crate::transport::http::observe;

use super::change_user_role::user_error_status;
use super::response::error_body;

pub fn delete_user_by_username_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    let username = ctx.param("user_name").to_vec();

    if !matches!(
        security::current_user_can_manage_user(ctx, &username),
        Ok(true)
    ) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        return;
    }

    if let Err(err) = security::delete_basic_user(&username) {
        ctx.set_status_code(user_error_status(&err));
        ctx.set_body(error_body(&err.message()));
        return;
    }

    let _ = acl::delete_user_acls(&username);
    observe::audit(ctx, "user deleted", kv!["user" => username]);
    ctx.set_status_code(STATUS_OK);
}
