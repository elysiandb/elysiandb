use std::collections::BTreeMap;

use crate::engine;
use crate::gocompat::json;
use crate::httpd::{RequestCtx, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::transport::http::observe;

pub fn get_entity_types_names_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let entity_types = engine::list_public_entity_types();

    let response = BTreeMap::from([("entities".to_string(), entity_types)]);
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
