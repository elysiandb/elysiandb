use crate::acl;
use crate::cache;
use crate::engine;
use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_NO_CONTENT};
use crate::kv;
use crate::security;
use crate::transport::http::observe;

use super::response::cache_enabled;

pub fn destroy_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();

    if security::user_authentication_is_enabled() && !security::current_user_is_admin(ctx) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body(br#"{"error":"only admin users can destroy entities"}"#);
        return;
    }

    engine::delete_all_entities(&entity);
    let _ = acl::delete_acl_for_entity_type(&entity);
    observe::audit(
        ctx,
        "entities destroyed",
        kv!["entity" => entity.as_slice()],
    );

    ctx.set_status_code(STATUS_NO_CONTENT);

    if cache_enabled()
        && let Some(store) = cache::cache_store()
    {
        store.purge(&entity);
    }
}
