use crate::httpd::{RequestCtx, STATUS_NO_CONTENT};
use crate::storage;
use crate::transport::http::observe;

pub fn save_controller(ctx: &mut RequestCtx) {
    ctx.set_content_type("application/json");
    ctx.set_status_code(STATUS_NO_CONTENT);

    storage::write_to_db();
    observe::audit(ctx, "manual flush", &[]);
}
