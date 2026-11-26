use crate::gocompat::json;
use crate::httpd::{
    RequestCtx, STATUS_BAD_REQUEST, STATUS_CONFLICT, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK,
};
use crate::json_struct;
use crate::kv;
use crate::security::{self, SecurityError};
use crate::transport::http::observe;

use super::response::error_body;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct UserRoleDto {
        pub role: String => "role",
    }
}

pub fn change_user_role_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    let username = ctx.param("user_name").to_vec();

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        return;
    }

    let mut user_dto = UserRoleDto::default();
    if json::unmarshal(ctx.post_body(), &mut user_dto).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"invalid request body"}"#);
        return;
    }

    if let Err(err) = security::change_user_role(&username, user_dto.role.as_bytes()) {
        ctx.set_status_code(user_error_status(&err));
        ctx.set_body(error_body(&err.message()));
        return;
    }

    observe::audit(
        ctx,
        "user role changed",
        kv!["user" => username, "role" => user_dto.role],
    );
    ctx.set_status_code(STATUS_OK);
}

pub fn user_error_status(err: &SecurityError) -> u16 {
    match err {
        SecurityError::InvalidRole => STATUS_BAD_REQUEST,
        SecurityError::DefaultAdminProtected => STATUS_FORBIDDEN,
        SecurityError::LastAdmin => STATUS_CONFLICT,
        _ => STATUS_NOT_FOUND,
    }
}
