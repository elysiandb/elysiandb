use std::sync::Arc;

use crate::api;
use crate::engine;
use crate::gocompat::json::Map;
use crate::httpd::{Handler, RequestCtx, STATUS_FORBIDDEN};

const CORE_ENTITY_FORBIDDEN_BODY: &str =
    r#"{"error":"core entities cannot be accessed through the generic entity routes"}"#;

pub fn reject_core_entity(next: Handler) -> Handler {
    Arc::new(move |ctx: &mut RequestCtx| {
        if engine::is_core_entity_type(ctx.param("entity")) {
            send_core_entity_forbidden(ctx);
            return;
        }

        next(ctx);
    })
}

pub fn references_core_entity<E: AsRef<[u8]>>(entity_types: &[E]) -> bool {
    entity_types
        .iter()
        .any(|entity_type| engine::is_core_entity_type(entity_type.as_ref()))
}

pub fn reject_core_entity_references(ctx: &mut RequestCtx, payload: &Map) -> bool {
    if !references_core_entity(&api::nested_entity_types_of_map(payload)) {
        return false;
    }

    send_core_entity_forbidden(ctx);

    true
}

pub fn send_core_entity_forbidden(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_FORBIDDEN);
    ctx.set_body_string(CORE_ENTITY_FORBIDDEN_BODY);
}
