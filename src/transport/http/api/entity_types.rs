use std::collections::BTreeMap;

use crate::engine;
use crate::gocompat::json;
use crate::httpd::{RequestCtx, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::transport::http::observe;

pub fn get_entity_types_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let entity_types = engine::list_public_entity_types();
    let mut entity_schemas: Vec<String> = Vec::with_capacity(entity_types.len());
    for entity_type in &entity_types {
        let schema = engine::get_entity_schema(entity_type);
        let schema_bytes = match json::marshal(&schema) {
            Ok(b) => b,
            Err(err) => {
                observe::set_error(ctx, &err);
                ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
                ctx.set_body(br#"{"error":"failed to marshal entity schema"}"#);
                return;
            }
        };

        entity_schemas.push(String::from_utf8_lossy(&schema_bytes).into_owned());
    }

    let response = BTreeMap::from([("entities".to_string(), entity_schemas)]);
    let response_bytes = match json::marshal(&response) {
        Ok(b) => b,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body(br#"{"error":"failed to marshal response"}"#);
            return;
        }
    };

    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response_bytes);
}
