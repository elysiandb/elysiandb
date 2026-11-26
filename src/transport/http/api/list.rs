use crate::acl::{self, ReadMode};
use crate::api::{self, ListQuery};
use crate::cache;
use crate::globals;
use crate::gocompat::strings;
use crate::gocompat::utf8;
use crate::hook;
use crate::httpd::{Args, RequestCtx, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::query::Filters;
use crate::security;
use crate::transport::http::observe;

use super::guard::{guard_stopped, list_entities, request_guard, send_guard_error};
use super::pre_read::{Documents, lazy_pre_read_page};
use super::response::{cache_deps, cache_enabled, count_planned, write_count, write_documents};

pub(crate) fn hook_entity(entity: &[u8]) -> Option<&str> {
    std::str::from_utf8(entity).ok()
}

pub(crate) fn entity_has_hooks(entity: &[u8]) -> bool {
    hook_entity(entity).is_some_and(hook::entity_has_hooks)
}

pub(crate) fn hooks_enabled() -> bool {
    globals::config().is_some_and(|cfg| cfg.api.hooks.enabled)
}

pub(crate) fn entity_has_pre_read_hooks(entity: &[u8]) -> bool {
    hook_entity(entity).is_some_and(hook::entity_has_pre_read_hooks)
}

pub(crate) fn entity_has_post_read_hooks(entity: &[u8]) -> bool {
    hook_entity(entity).is_some_and(hook::entity_has_post_read_hooks)
}

pub(crate) fn hook_request(username: &[u8]) -> hook::Request {
    hook::Request::new(&utf8::decode_string(username))
}

fn send_processing_error(ctx: &mut RequestCtx) {
    ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
    ctx.response.header.set("X-Elysian-Cache", "MISS");
    ctx.set_body_string("Error processing request");
}

pub fn list_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let args = ctx.query_args();
    let limit = args.get_uint_or_zero("limit");
    let offset = args.get_uint_or_zero("offset");
    let (sort_field, sort_ascending) = parse_sort_param(args);
    let filters = parse_filter_param(args);
    let search = args.peek("search").to_vec();
    let fields_param = args.peek("fields").to_vec();
    let includes_param = args.peek("includes").to_vec();
    let count_only_param = args.get_bool("countOnly");

    let mut current_user = Vec::new();
    if security::user_authentication_is_enabled() {
        current_user = security::get_current_username(ctx);
    }

    let mut hash: Option<[u8; 32]> = None;
    if !entity_has_hooks(&entity) && cache_enabled() {
        let h = cache::hash_query(
            &entity,
            limit,
            offset,
            &sort_field,
            sort_ascending,
            &filters,
            &fields_param,
            &search,
            &includes_param,
            count_only_param,
            &current_user,
        );
        hash = Some(h);
        if let Some(store) = cache::cache_store()
            && let Some(cached) = store.get(&entity, &h)
        {
            ctx.response.header.set("Content-Type", "application/json");
            ctx.response.header.set("X-Elysian-Cache", "HIT");
            ctx.set_status_code(STATUS_OK);
            ctx.set_body(&cached[..]);

            return;
        }
    }
    let hash = hash.as_ref().map(|h| h.as_slice());

    let hooks_enabled = hooks_enabled();
    let pre_read_hooks = hooks_enabled && entity_has_pre_read_hooks(&entity);
    let post_read_hooks = hooks_enabled && entity_has_post_read_hooks(&entity);
    let paginate_after_hooks = pre_read_hooks && !filters.is_empty();
    let guard = request_guard(ctx);
    let hooks = hook_request(&current_user).with_guard(guard.clone());

    if !paginate_after_hooks {
        let includes = api::merge_includes(&includes_param, &api::extract_auto_includes(&filters));
        let fields = api::parse_fields_param(&fields_param);
        let mode = acl::read_mode_for(&current_user, &entity);

        if count_only_param {
            let res = api::plan(ListQuery {
                entity: &entity,
                limit,
                offset,
                sort_field: &sort_field,
                sort_asc: sort_ascending,
                filters: Some(&filters),
                search: &search,
                includes: &includes,
                count_only: mode != ReadMode::ReadOwned,
                guard: Some(&guard),
                ..ListQuery::default()
            });
            if let Some(err) = res.err {
                send_guard_error(ctx, err);
                return;
            }
            let count = count_planned(
                &res,
                limit > 0 || offset > 0 || !sort_field.is_empty(),
                mode,
                &current_user,
            );
            write_count(ctx, count, &entity, hash, &cache_deps(&entity, &includes));
            return;
        }

        if includes.is_empty()
            && fields.is_empty()
            && !pre_read_hooks
            && !post_read_hooks
            && mode == ReadMode::ReadAll
        {
            let res = api::plan(ListQuery {
                entity: &entity,
                limit,
                offset,
                sort_field: &sort_field,
                sort_asc: sort_ascending,
                filters: Some(&filters),
                search: &search,
                guard: Some(&guard),
                ..ListQuery::default()
            });
            if let Some(err) = res.err {
                send_guard_error(ctx, err);
                return;
            }

            if let Err(err) = write_documents(
                ctx,
                &res.docs,
                &entity,
                hash,
                true,
                &cache_deps(&entity, b""),
            ) {
                observe::set_error(ctx, &err);
                send_processing_error(ctx);
            }

            return;
        }
    }

    let hook_name = hook_entity(&entity).unwrap_or_default();

    let mut data = if paginate_after_hooks {
        let includes = api::merge_includes(&includes_param, &api::extract_auto_includes(&filters));
        let snap = api::snapshot(&entity);
        let stored =
            |item: &crate::gocompat::json::Map| api::stored_filters_match_entity(item, &filters);
        let matches = |item: &crate::gocompat::json::Map| api::filters_match_entity(item, &filters);
        let candidates = api::stored_candidates(snap.as_ref(), Some(&filters), None, &includes);
        let each = |f: &mut dyn FnMut(crate::datastore::Doc) -> bool| {
            api::plan(ListQuery {
                entity: &entity,
                sort_field: &sort_field,
                sort_asc: sort_ascending,
                search: &search,
                includes: &includes,
                guard: Some(&guard),
                snapshot: snap.clone(),
                each: Some(f),
                ..ListQuery::default()
            });
        };
        Documents::Hooked(lazy_pre_read_page(
            &hooks,
            hook_name,
            each,
            &includes,
            &stored,
            candidates.as_ref(),
            acl::read_mode_for(&current_user, &entity),
            &current_user,
            &matches,
            offset,
            limit,
        ))
    } else {
        Documents::Plain(list_entities(
            &guard,
            &entity,
            limit,
            offset,
            &sort_field,
            sort_ascending,
            &filters,
            &search,
            &includes_param,
        ))
    };

    if !paginate_after_hooks && let Documents::Plain(plain) = data {
        data = Documents::Plain(acl::filter_list_of_entities(&current_user, &entity, plain));
    }

    if pre_read_hooks {
        data = data.pre_read(&hooks, hook_name).apply_filters(&filters);
    }

    if guard_stopped(ctx, &guard) {
        data.release();
        return;
    }

    let includes = api::merge_includes(&includes_param, &api::extract_auto_includes(&filters));
    if count_only_param {
        let count = data.len();
        data.release();
        write_count(ctx, count, &entity, hash, &cache_deps(&entity, &includes));
        return;
    }

    let fields = api::parse_fields_param(&fields_param);

    if post_read_hooks {
        data = data.post_read(&hooks, hook_name, &fields);
        if guard_stopped(ctx, &guard) {
            return;
        }
    }

    let encoded = match data.encode(&fields) {
        Ok(encoded) => encoded,
        Err(err) => {
            observe::set_error(ctx, &err);
            send_processing_error(ctx);
            return;
        }
    };

    ctx.response.header.set("Content-Type", "application/json");
    ctx.response.header.set("X-Elysian-Cache", "MISS");
    ctx.set_status_code(STATUS_OK);

    let Some(response) = encoded.in_memory().map(<[u8]>::to_vec) else {
        if let Err(err) = encoded.send(ctx) {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            ctx.set_body_string("Error processing request");
        }
        return;
    };

    ctx.set_body(&response);

    if cache_enabled()
        && let (Some(hash), Some(store)) = (hash, cache::cache_store())
    {
        store.set_with_deps(&entity, hash, response, &cache_deps(&entity, &includes));
    }
}

pub fn parse_sort_param(params: &Args) -> (Vec<u8>, bool) {
    let mut field: Vec<u8> = Vec::new();

    for (key, v) in params.all() {
        if key.starts_with(b"sort[") && key.ends_with(b"]") {
            field = key[b"sort[".len()..key.len() - 1].to_vec();
            let val = strings::to_lower(strings::trim_space(v));
            match val.as_slice() {
                b"asc" => return (field, true),
                b"desc" => return (field, false),
                _ => field = Vec::new(),
            }
        }
    }

    (field, true)
}

pub fn parse_filter_param(params: &Args) -> Filters {
    let mut filters = Filters::new();
    for (key, v) in params.all() {
        if key.starts_with(b"filter[") && key.ends_with(b"]") {
            let inner = &key[b"filter[".len()..key.len() - 1];
            let val = strings::trim_space(v).to_vec();

            let (field, op) = match inner.windows(2).position(|w| w == b"][") {
                Some(i) => (inner[..i].to_vec(), inner[i + 2..].to_vec()),
                None => (inner.to_vec(), b"eq".to_vec()),
            };

            filters.entry(field).or_default().insert(op, val);
        }
    }

    filters
}
