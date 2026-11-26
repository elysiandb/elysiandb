use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::json_struct;
use crate::security;

use super::response::{UserView, user_view_from, write_error, write_json};

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct UsersResponse {
        pub users: Vec<UserView> => "users",
    }
}

pub fn get_users_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    if !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);

        return;
    }

    let users = match security::list_basic_users() {
        Ok(users) => users,
        Err(err) => {
            write_error(ctx, STATUS_INTERNAL_SERVER_ERROR, &err);
            return;
        }
    };

    let response = UsersResponse {
        users: users.iter().map(user_view_from).collect(),
    };

    write_json(ctx, STATUS_OK, &response);
}
