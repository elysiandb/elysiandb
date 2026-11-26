use crate::acl;
use crate::globals;
use crate::gocompat::json::{self, Map, Value};
use crate::gocompat::utf8;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_OK};
use crate::schema;
use crate::security;
use crate::transaction::{self, TxOperation};

use super::super::core_entity::reject_core_entity_references;
use super::{error_body, tx_id};

pub fn write_transaction_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let tx_id = tx_id(ctx);
    let entity = ctx.param("entity").to_vec();

    let mut payload: Option<Map> = None;
    let decoded = json::unmarshal(ctx.post_body(), &mut payload);
    let (Ok(()), Some(mut payload)) = (decoded, payload) else {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(r#"{"error":"invalid json"}"#);

        return;
    };

    if reject_core_entity_references(ctx, &payload) {
        return;
    }

    let username = security::get_current_username(ctx);
    if !acl::can_write_entity(&username, &entity, &payload) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);

        return;
    }

    if globals::config().is_some_and(|cfg| cfg.api.schema.enabled)
        && entity != schema::SCHEMA_ENTITY.as_bytes()
    {
        let errors = schema::validate_entity(&entity, &payload, None);
        if !errors.is_empty() {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"schema validation failed"}"#);

            return;
        }
    }

    if security::user_authentication_is_enabled() {
        payload.insert(
            acl::USERNAME_FIELD.to_string(),
            Value::String(utf8::decode_string(&username)),
        );
    }

    let op = TxOperation {
        kind: "write".to_string(),
        entity,
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
