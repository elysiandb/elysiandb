use crate::acl;
use crate::engine;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_OK};
use crate::security;
use crate::transaction::{self, TxOperation};

use super::{error_body, tx_id};

pub fn delete_transaction_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let tx_id = tx_id(ctx);
    let entity = ctx.param("entity").to_vec();
    let id = ctx.param("id").to_vec();

    let existing = engine::read_entity_by_id(&entity, &id).unwrap_or_default();
    if !acl::can_delete_entity(&security::get_current_username(ctx), &entity, &existing) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body_string(r#"{"error":"forbidden"}"#);

        return;
    }

    let op = TxOperation {
        kind: "delete".to_string(),
        entity,
        id,
        ..TxOperation::default()
    };

    if let Err(err) = transaction::add_operation(&tx_id, op) {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(error_body(&err));

        return;
    }

    ctx.set_status_code(STATUS_OK);
}
