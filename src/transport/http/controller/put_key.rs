use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_NO_CONTENT};
use crate::storage;

pub fn put_key_controller(ctx: &mut RequestCtx) {
    let key = ctx.param("key").to_vec();
    let ttl = ctx.query_args().get_uint_or_zero("ttl");

    let buf = ctx.post_body().to_vec();

    let result = if ttl > 0 {
        storage::put_key_value_with_ttl(&key, &buf, ttl)
    } else {
        storage::put_key_value(&key, &buf)
    };

    if result.is_err() {
        ctx.error("Failed to store key-value pair", STATUS_BAD_REQUEST);
        return;
    }

    ctx.set_status_code(STATUS_NO_CONTENT);
}
