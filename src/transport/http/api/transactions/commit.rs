use crate::acl;
use crate::api;
use crate::cache;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_OK};
use crate::transaction;

use super::super::response::cache_enabled;
use super::{error_body, tx_id};

pub fn commit_transaction_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let tx_id = tx_id(ctx);
    let tx = transaction::get_transaction(&tx_id).ok();

    let err = transaction::commit_transaction(&tx_id);

    if let Some(tx) = &tx
        && cache_enabled()
        && let Some(store) = cache::cache_store()
    {
        for op in &tx.ops {
            store.purge(&op.entity);

            for nested in api::nested_entity_types_of_map(&op.data) {
                store.purge(&nested);
            }
        }
    }

    if let Err(err) = err {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(error_body(&err));

        return;
    }

    let _ = acl::init_acl();

    ctx.set_status_code(STATUS_OK);
}
