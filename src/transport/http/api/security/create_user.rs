use crate::acl;
use crate::gocompat::json;
use crate::httpd::{
    RequestCtx, STATUS_BAD_REQUEST, STATUS_CONFLICT, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK,
};
use crate::json_struct;
use crate::kv;
use crate::security::{self, BasicUser, ROLE_ADMIN, ROLE_USER, Role, SecurityError};
use crate::transport::http::observe;

use super::response::write_error;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct UserDto {
        pub user: String => "username",
        pub password: String => "password",
        pub role: String => "role",
    }
}

impl UserDto {
    pub fn to_basic_user(&self) -> BasicUser {
        BasicUser {
            username: self.user.as_bytes().to_vec(),
            password: self.password.as_bytes().to_vec(),
            role: Role::new(self.role.clone()),
        }
    }

    pub fn validate(&self) -> Result<(), SecurityError> {
        security::validate_username(self.user.as_bytes())?;

        if !self.role.is_empty() && self.role != ROLE_ADMIN && self.role != ROLE_USER {
            return Err(SecurityError::InvalidRole);
        }

        security::validate_password(self.password.as_bytes())?;

        if security::basic_user_exists(self.user.as_bytes()) {
            return Err(SecurityError::UserAlreadyExists);
        }

        Ok(())
    }
}

pub fn create_user_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        return;
    }

    let mut user = UserDto::default();
    if json::unmarshal(ctx.post_body(), &mut user).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"invalid request body"}"#);

        return;
    }

    if let Err(err) = user.validate() {
        write_error(ctx, create_user_error_status(&err), &err);
        return;
    }

    if let Err(err) = security::create_basic_user(&user.to_basic_user()) {
        write_error(ctx, STATUS_NOT_FOUND, &err);
        return;
    }

    let _ = acl::init_acl();

    observe::audit(
        ctx,
        "user created",
        kv!["user" => user.user, "role" => user.role],
    );
    ctx.set_status_code(STATUS_OK);
}

pub fn create_user_error_status(err: &SecurityError) -> u16 {
    match err {
        SecurityError::InvalidUsername
        | SecurityError::PasswordTooShort
        | SecurityError::InvalidRole => STATUS_BAD_REQUEST,
        SecurityError::UserAlreadyExists => STATUS_CONFLICT,
        _ => STATUS_NOT_FOUND,
    }
}
