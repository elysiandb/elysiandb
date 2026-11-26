use crate::acl::{self, ReadMode};
use crate::api::{self, ListQuery};
use crate::httpd::{RequestCtx, STATUS_OK};
use crate::security;

use super::response::count_planned;

pub fn count_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    ctx.response.header.set("Content-Type", "application/json");

    let username = security::get_current_username(ctx);

    let count = match acl::read_mode_for(&username, &entity) {
        ReadMode::ReadAll => api::count_entities(&entity),
        mode => {
            let res = api::plan(ListQuery {
                entity: &entity,
                ..ListQuery::default()
            });
            count_planned(&res, true, mode, &username)
        }
    };

    ctx.set_status_code(STATUS_OK);
    ctx.set_body(format!(r#"{{"count":{count}}}"#));
}
