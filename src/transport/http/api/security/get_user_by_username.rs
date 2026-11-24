use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK};
use crate::security;

use super::response::{user_view_from, write_error, write_json};

pub fn get_user_by_username_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    let username = ctx.param("user_name").to_vec();

    if !matches!(
        security::current_user_can_manage_user(ctx, &username),
        Ok(true)
    ) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        return;
    }

    match security::get_basic_user_by_username(&username) {
        Ok(user) => write_json(ctx, STATUS_OK, &user_view_from(&user)),
        Err(err) => write_error(ctx, STATUS_NOT_FOUND, &err),
    }
}
