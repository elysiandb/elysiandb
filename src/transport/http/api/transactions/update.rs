use crate::acl;
use crate::engine;
use crate::globals;
use crate::gocompat::json::{self, Map};
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_OK};
use crate::schema;
use crate::security;
use crate::transaction::{self, TxOperation};

use super::super::core_entity::reject_core_entity_references;
use super::{error_body, tx_id};

pub fn update_transaction_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let tx_id = tx_id(ctx);
    let entity = ctx.param("entity").to_vec();
    let id = ctx.param("id").to_vec();

    let mut payload: Option<Map> = None;
    if json::unmarshal(ctx.post_body(), &mut payload).is_err() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"invalid json"}"#);

        return;
    }
    let payload = payload.unwrap_or_default();

    if reject_core_entity_references(ctx, &payload) {
        return;
    }

    let Some(existing) = engine::read_entity_by_id(&entity, &id) else {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"entity not found"}"#);

        return;
    };

    if !acl::can_update_entity(&security::get_current_username(ctx), &entity, &id, &payload) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);

        return;
    }

    if globals::config().is_some_and(|cfg| cfg.api.schema.enabled)
        && entity != schema::SCHEMA_ENTITY.as_bytes()
    {
        let errors = schema::validate_entity_update(&entity, Some(&existing), &payload, None);
        if !errors.is_empty() {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"schema validation failed"}"#);

            return;
        }
    }

    let op = TxOperation {
        kind: "update".to_string(),
        entity,
        id,
        data: payload,
        ..TxOperation::default()
    };

    if let Err(err) = transaction::add_operation(&tx_id, op) {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(error_body(&err));

        return;
    }

    ctx.set_status_code(STATUS_OK);
}
