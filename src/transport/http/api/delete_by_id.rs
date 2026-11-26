use crate::acl;
use crate::cache;
use crate::engine;
use crate::httpd::{RequestCtx, STATUS_FORBIDDEN, STATUS_NO_CONTENT, STATUS_NOT_FOUND};
use crate::security;

use super::response::cache_enabled;

pub fn delete_by_id_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let id = ctx.param("id").to_vec();

    let Some(data) = engine::read_entity_by_id(&entity, &id) else {
        ctx.set_status_code(STATUS_NOT_FOUND);
        return;
    };

    if !acl::can_delete_entity(&security::get_current_username(ctx), &entity, &data) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        return;
    }

    engine::delete_entity_by_id(&entity, &id);

    ctx.set_status_code(STATUS_NO_CONTENT);

    if cache_enabled()
        && let Some(store) = cache::cache_store()
    {
        store.purge(&entity);
    }
}
