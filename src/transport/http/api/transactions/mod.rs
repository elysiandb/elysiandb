mod begin;
mod commit;
mod delete;
mod rollback;
mod update;
mod write;

pub use begin::begin_transaction_controller;
pub use commit::commit_transaction_controller;
pub use delete::delete_transaction_controller;
pub use rollback::rollback_transaction_controller;
pub use update::update_transaction_controller;
pub use write::write_transaction_controller;

use crate::gocompat::errors::{self, BoxError};
use crate::httpd::RequestCtx;

fn tx_id(ctx: &RequestCtx) -> String {
    String::from_utf8_lossy(ctx.param("txId")).into_owned()
}

fn error_body(err: &BoxError) -> Vec<u8> {
    let mut body = br#"{"error":""#.to_vec();
    body.extend_from_slice(&errors::message_bytes(err.as_ref()));
    body.extend_from_slice(br#""}"#);
    body
}
