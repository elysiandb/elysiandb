use crate::httpd::{RequestCtx, STATUS_OK};
use crate::transaction;

pub fn begin_transaction_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    let tx = transaction::begin_transaction();
    ctx.response
        .set_body_string(format!(r#"{{"transaction_id":"{}"}}"#, tx.id));
    ctx.set_status_code(STATUS_OK);
}
