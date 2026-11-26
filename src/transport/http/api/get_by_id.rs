use crate::acl;
use crate::api;
use crate::cache;
use crate::engine;
use crate::gocompat::json::{self, Map};
use crate::hook;
use crate::httpd::{
    RequestCtx, STATUS_FORBIDDEN, STATUS_INTERNAL_SERVER_ERROR, STATUS_NOT_FOUND, STATUS_OK,
};
use crate::security;
use crate::transport::http::observe;

use super::guard::{guard_stopped, request_guard};
use super::list::{
    entity_has_hooks, entity_has_post_read_hooks, hook_entity, hook_request, hooks_enabled,
};
use super::pre_read::{marshal_document, marshal_map, release_document};
use super::response::cache_enabled;

pub fn get_by_id_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let id = ctx.param("id").to_vec();
    let username = security::get_current_username(ctx);
    let fields_param = ctx.query_args().peek("fields").to_vec();
    let fields = api::parse_fields_param(&fields_param);
    let includes_param = ctx.query_args().peek("includes").to_vec();

    let cacheable = cache_enabled()
        && !entity_has_hooks(&entity)
        && fields.is_empty()
        && includes_param.is_empty();

    if cacheable
        && let Some(store) = cache::cache_store()
        && let Some(v) = store.get_by_id(&entity, &id)
        && can_read_cached_entity(&username, &entity, &v)
    {
        ctx.response.header.set("Content-Type", "application/json");
        ctx.response.header.set("X-Elysian-Cache", "HIT");
        ctx.set_status_code(STATUS_OK);
        ctx.set_body(&v[..]);
        return;
    }

    let Some(mut data) = engine::read_entity_by_id(&entity, &id) else {
        ctx.set_status_code(STATUS_NOT_FOUND);
        ctx.response.header.set("X-Elysian-Cache", "MISS");
        ctx.set_body_string("Entity not found");

        return;
    };

    if !acl::can_read_entity(&username, &entity, &data) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.response.header.set("X-Elysian-Cache", "MISS");
        ctx.set_body_string("Access denied");

        return;
    }

    if !includes_param.is_empty() {
        let mut list = vec![data];
        engine::apply_includes(&mut list, &includes_param);
        data = list.pop().unwrap_or_default();
    }

    if !fields.is_empty() {
        data = engine::filter_fields(&data, &fields);
    }

    let response = if hooks_enabled() && entity_has_post_read_hooks(&entity) {
        let guard = request_guard(ctx);
        let doc = hook_request(&username).with_guard(guard.clone()).post_read(
            hook_entity(&entity).unwrap_or_default(),
            &hook::document_from_json(&data),
        );
        if guard_stopped(ctx, &guard) {
            release_document(&doc);
            return;
        }

        let response = marshal_document(&doc);
        release_document(&doc);
        response
    } else {
        marshal_map(&data)
    };

    let response = match response {
        Ok(response) => response,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.response.header.set("X-Elysian-Cache", "MISS");
            ctx.set_body_string("Error processing request");
            return;
        }
    };

    if cacheable && let Some(store) = cache::cache_store() {
        store.set_by_id_with_deps(&entity, &id, response.clone(), &[entity.as_slice()]);
    }

    ctx.response.header.set("Content-Type", "application/json");
    ctx.response.header.set("X-Elysian-Cache", "MISS");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response);
}

fn can_read_cached_entity(username: &[u8], entity: &[u8], cached: &[u8]) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    let mut data: Option<Map> = None;
    if json::unmarshal(cached, &mut data).is_err() {
        return false;
    }

    acl::can_read_entity(username, entity, &data.unwrap_or_default())
}
