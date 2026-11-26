use crate::gocompat::url;
use crate::httpd::{RequestCtx, STATUS_NO_CONTENT};
use crate::storage;
use crate::wildcard;

pub fn delete_key_controller(ctx: &mut RequestCtx) {
    let mut key = ctx.param("key").to_vec();

    if let Ok(dec) = url::path_unescape(&key) {
        key = dec;
    }

    if wildcard::key_contains_wildcard(&key) {
        storage::delete_by_wildcard_key(&key);
    } else {
        storage::delete_by_key(&key);
    }

    ctx.set_status_code(STATUS_NO_CONTENT);
}
