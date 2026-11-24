use std::sync::atomic::Ordering;

use crate::api;
use crate::httpd::RequestCtx;
use crate::stat;
use crate::storage;

pub fn stats_controller(ctx: &mut RequestCtx) {
    let (keys, with_ttl) = storage::count_keys();
    let stats = stat::stats();
    stats.set_keys_count(keys);
    stats.set_expiration_keys_count(with_ttl);
    stats.entities_count.store(
        i64::try_from(api::count_all_entities()).unwrap_or(i64::MAX),
        Ordering::SeqCst,
    );

    ctx.set_content_type("application/json");
    ctx.write_string(stats.to_json());
}
