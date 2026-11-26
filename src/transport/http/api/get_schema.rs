use crate::engine;
use crate::gocompat::json;
use crate::httpd::{RequestCtx, STATUS_INTERNAL_SERVER_ERROR, STATUS_NOT_FOUND, STATUS_OK};
use crate::schema;
use crate::transport::http::observe;

pub fn get_schema_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();

    if !engine::entity_type_exists(&entity) {
        ctx.set_status_code(STATUS_NOT_FOUND);
        ctx.response.header.set("Content-Type", "application/json");
        ctx.set_body_string(r#"{"error":"entity not found"}"#);

        return;
    }

    let Some(schema) = engine::read_entity_by_id(schema::SCHEMA_ENTITY.as_bytes(), &entity) else {
        ctx.set_status_code(STATUS_NOT_FOUND);
        ctx.response.header.set("Content-Type", "application/json");
        ctx.set_body_string(r#"{"error":"schema not found"}"#);

        return;
    };

    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_OK);
    let data = match json::marshal(&schema) {
        Ok(data) => data,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string(r#"{"error":"failed to marshal schema"}"#);

            return;
        }
    };

    ctx.set_body(data);
}
