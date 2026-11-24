use crate::httpd::{RequestCtx, STATUS_OK};
use crate::storage;
use crate::transport::http::observe;

pub fn reset_controller(ctx: &mut RequestCtx) {
    ctx.set_content_type("application/json");
    ctx.set_status_code(STATUS_OK);

    storage::reset_store();
    observe::audit(ctx, "store reset", &[]);
}
