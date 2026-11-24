use crate::globals;
use crate::httpd::{RequestCtx, STATUS_OK, STATUS_SERVICE_UNAVAILABLE};

pub fn health_controller(ctx: &mut RequestCtx) {
    if globals::is_starting() {
        ctx.set_status_code(STATUS_SERVICE_UNAVAILABLE);
        return;
    }

    ctx.set_status_code(STATUS_OK);
}
