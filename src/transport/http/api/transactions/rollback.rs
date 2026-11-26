use crate::httpd::{RequestCtx, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::transaction;
use crate::transport::http::observe;

use super::{error_body, tx_id};

pub fn rollback_transaction_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    let tx_id = tx_id(ctx);

    if let Err(err) = transaction::rollback_transaction(&tx_id) {
        observe::set_error(ctx, &err);
        ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
        ctx.set_body(error_body(&err));

        return;
    }

    ctx.set_status_code(STATUS_OK);
}
