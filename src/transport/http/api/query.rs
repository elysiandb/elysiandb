use std::collections::BTreeMap;
use std::fmt;

use crate::acl::{self, ReadMode};
use crate::api;
use crate::cache;
use crate::crypto::sha256;
use crate::engine;
use crate::gocompat::json::{
    self, Decode, DecodeState, Error, Expected, Map, RawMessage, Subject, Value,
};
use crate::gocompat::strconv;
use crate::gocompat::utf8;
use crate::hook::Request;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::json_struct;
use crate::query::{FilterNode, Filters, Ops, Query};
use crate::security;
use crate::transport::http::observe;

use super::core_entity::send_core_entity_forbidden;
use super::guard::{execute_query, guard_stopped, request_guard, send_guard_error};
use super::list::{
    entity_has_hooks, entity_has_post_read_hooks, entity_has_pre_read_hooks, hook_entity,
    hook_request, hooks_enabled,
};
use super::pre_read::{Documents, lazy_pre_read_page};
use super::response::{cache_deps, cache_enabled, count_planned, write_count, write_documents};

json_struct! {
    #[derive(Default)]
    struct RawPayload {
        entity: String => "entity",
        offset: i64 => "offset",
        limit: i64 => "limit",
        filters: Option<Map> => "filters",
        sorts: Option<BTreeMap<String, String>> => "sorts",
        count_only: bool => "countOnly",
        fields: String => "fields",
    }
}

json_struct! {
    #[derive(Default)]
    struct RawSorts {
        sorts: RawMessage => "sorts",
    }
}

#[derive(Clone, Debug, Default)]
pub struct QueryPayload {
    pub entity: String,
    pub offset: i64,
    pub limit: i64,
    pub filters: Option<Map>,
    pub sorts: Option<BTreeMap<String, String>>,
    pub count_only: bool,
    pub fields: String,
    pub sort_order: Vec<String>,
}

impl Decode for QueryPayload {
    const EXPECTED: Expected = Expected::Object;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        let data = d.data();
        let start = d.start();
        let mut payload = RawPayload::default();
        payload.decode(d)?;
        let end = d.start();
        let raw_value = &data[start..end];

        let mut raw = RawSorts::default();
        let _ = json::unmarshal(raw_value, &mut raw);

        *self = QueryPayload {
            entity: payload.entity,
            offset: payload.offset,
            limit: payload.limit,
            filters: payload.filters,
            sorts: payload.sorts,
            count_only: payload.count_only,
            fields: payload.fields,
            sort_order: object_keys_in_order(raw.sorts.as_bytes()),
        };

        Ok(())
    }
}

fn object_keys_in_order(raw: &[u8]) -> Vec<String> {
    let mut d = DecodeState::new(raw);
    if raw.is_empty() || d.peek() != json::Kind::Object || !json::valid(raw) {
        return Vec::new();
    }

    let mut keys = Vec::new();
    let _ = d.object_members(|d, key| {
        keys.push(utf8::decode_string(key));
        d.skip();
        Ok(())
    });

    keys
}

json_struct! {
    #[derive(Default)]
    struct NormalizedSort {
        field: Vec<u8> => "Field",
        ascending: bool => "Ascending",
    }
}

json_struct! {
    #[derive(Default)]
    struct NormalizedPayload {
        count_only: bool => "countOnly",
        entity: String => "entity",
        fields: String => "fields",
        filters: Option<Map> => "filters",
        limit: i64 => "limit",
        offset: i64 => "offset",
        sort_order: Vec<NormalizedSort> => "sortOrder",
        sorts: Option<BTreeMap<String, String>> => "sorts",
    }
}

impl QueryPayload {
    pub fn hash(&self) -> [u8; sha256::SIZE] {
        let normalized = NormalizedPayload {
            count_only: self.count_only,
            entity: self.entity.clone(),
            fields: self.fields.clone(),
            filters: Some(self.filters.clone().unwrap_or_default()),
            limit: self.limit,
            offset: self.offset,
            sort_order: self
                .query_for_sorts()
                .ordered_sorts()
                .into_iter()
                .map(|s| NormalizedSort {
                    field: s.field,
                    ascending: s.ascending,
                })
                .collect(),
            sorts: self.sorts.clone(),
        };

        let b = json::marshal(&normalized).unwrap_or_default();

        sha256::sum256(&b)
    }

    pub fn query_for_sorts(&self) -> Query {
        Query {
            sorts: sort_map(self.sorts.as_ref()),
            sort_order: self
                .sort_order
                .iter()
                .map(|k| k.as_bytes().to_vec())
                .collect(),
            ..Query::default()
        }
    }
}

pub fn sort_map(sorts: Option<&BTreeMap<String, String>>) -> BTreeMap<Vec<u8>, Vec<u8>> {
    sorts
        .map(|m| {
            m.iter()
                .map(|(k, v)| (k.as_bytes().to_vec(), v.as_bytes().to_vec()))
                .collect()
        })
        .unwrap_or_default()
}

pub fn decode_query_payload(body: &[u8]) -> Result<QueryPayload, Error> {
    let mut payload = QueryPayload::default();
    json::unmarshal(body, &mut payload)?;
    Ok(payload)
}

fn send_query_error(ctx: &mut RequestCtx, status: u16, message: &str) {
    let body = BTreeMap::from([("error".to_string(), message.to_string())]);
    let body = json::marshal(&body).unwrap_or_default();
    ctx.set_status_code(status);
    ctx.set_body(body);
}

pub fn query_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let mut payload = match decode_query_payload(ctx.post_body()) {
        Ok(payload) => payload,
        Err(err) => {
            send_query_error(ctx, STATUS_BAD_REQUEST, &err.message(Subject::Body));
            return;
        }
    };

    if engine::is_core_entity_type(payload.entity.as_bytes()) {
        send_core_entity_forbidden(ctx);
        return;
    }

    if payload.filters.is_none() {
        payload.filters = Some(Map::new());
    }
    if payload.sorts.is_none() {
        payload.sorts = Some(BTreeMap::new());
    }

    let mut current_user = Vec::new();
    if security::user_authentication_is_enabled() {
        current_user = security::get_current_username(ctx);
    }

    let entity = payload.entity.as_bytes().to_vec();

    let mut hash: Option<[u8; sha256::SIZE]> = None;
    if !entity_has_hooks(&entity) && cache_enabled() {
        let mut b = entity.clone();
        b.extend_from_slice(&payload.hash());
        b.extend_from_slice(&current_user);
        let h = sha256::sum256(&b);
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

    let filter = match parse_filter_node(payload.filters.as_ref()) {
        Ok(filter) => filter,
        Err(err) => {
            send_query_error(ctx, STATUS_BAD_REQUEST, &err.to_string());
            return;
        }
    };

    let hooks_enabled = hooks_enabled();
    let pre_read_hooks = hooks_enabled && entity_has_pre_read_hooks(&entity);
    let post_read_hooks = hooks_enabled && entity_has_post_read_hooks(&entity);
    let guard = request_guard(ctx);
    let hooks = hook_request(&current_user).with_guard(guard.clone());

    let q = Query {
        entity: entity.clone(),
        offset: payload.offset,
        limit: payload.limit,
        filter,
        sorts: sort_map(payload.sorts.as_ref()),
        sort_order: payload
            .sort_order
            .iter()
            .map(|k| k.as_bytes().to_vec())
            .collect(),
    };

    let filter_after_hooks = pre_read_hooks && !api::query_filter_is_empty(&q.filter);
    let fields = api::parse_fields_param(payload.fields.as_bytes());

    if !filter_after_hooks {
        let mode = acl::read_mode_for(&current_user, &entity);
        if payload.count_only {
            let res = api::plan_query_guarded(&q, mode != ReadMode::ReadOwned, Some(&guard));
            if let Some(err) = res.err {
                send_guard_error(ctx, err);
                return;
            }
            let paged = q.offset > 0 || q.limit > 0 || !q.ordered_sorts().is_empty();
            let count = count_planned(&res, paged, mode, &current_user);
            write_count(ctx, count, &entity, hash, &cache_deps(&entity, b""));
            return;
        }

        if fields.is_empty() && !pre_read_hooks && !post_read_hooks && mode == ReadMode::ReadAll {
            let res = api::plan_query_guarded(&q, false, Some(&guard));
            if let Some(err) = res.err {
                send_guard_error(ctx, err);
                return;
            }

            if let Err(err) = write_documents(
                ctx,
                &res.docs,
                &entity,
                hash,
                false,
                &cache_deps(&entity, b""),
            ) {
                observe::set_error(ctx, &err);
                ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            }

            return;
        }
    }

    let hook_name = hook_entity(&entity).unwrap_or_default();

    if filter_after_hooks {
        let all = Query {
            entity: q.entity.clone(),
            offset: 0,
            limit: 0,
            filter: FilterNode::with_leaf(Filters::new()),
            sorts: q.sorts.clone(),
            sort_order: q.sort_order.clone(),
        };
        let snap = api::snapshot(&entity);
        let stored =
            |item: &Map| api::apply_stored_query_filter(vec![item.clone()], &q.filter).len() == 1;
        let matches = |item: &Map| api::query_filter_matches_entity(&q.filter, item);
        let candidates = api::stored_candidates(snap.as_ref(), None, Some(&q.filter), b"");
        let each = |f: &mut dyn FnMut(crate::datastore::Doc) -> bool| {
            let _ = api::stream_query_guarded(&all, Some(&guard), snap.clone(), f);
        };
        let data = lazy_pre_read_page(
            &hooks,
            hook_name,
            each,
            b"",
            &stored,
            candidates.as_ref(),
            acl::read_mode_for(&current_user, &entity),
            &current_user,
            &matches,
            payload.offset,
            payload.limit,
        );
        finish_query(
            ctx,
            &payload,
            Documents::Hooked(data),
            &hooks,
            post_read_hooks,
            &fields,
            hash,
        );
        return;
    }

    let (data, err) = execute_query(&guard, &q);
    if let Some(err) = err {
        if guard_stopped(ctx, &guard) {
            return;
        }
        observe::set_error(ctx, &err);
        ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
        return;
    }

    let data = acl::filter_list_of_entities(&current_user, &entity, data);

    let mut data = Documents::Plain(data);
    if pre_read_hooks && !payload.count_only {
        data = data.pre_read(&hooks, hook_name);
    }

    finish_query(ctx, &payload, data, &hooks, post_read_hooks, &fields, hash);
}

fn finish_query(
    ctx: &mut RequestCtx,
    payload: &QueryPayload,
    mut data: Documents,
    hooks: &Request,
    post_read_hooks: bool,
    fields: &[Vec<u8>],
    hash: Option<&[u8]>,
) {
    let guard = request_guard(ctx);
    if guard_stopped(ctx, &guard) {
        data.release();
        return;
    }

    let entity = payload.entity.as_bytes();
    if payload.count_only {
        let count = data.len();
        data.release();
        write_count(ctx, count, entity, hash, &cache_deps(entity, b""));
        return;
    }

    if post_read_hooks {
        data = data.post_read(hooks, hook_entity(entity).unwrap_or_default(), fields);
        if guard_stopped(ctx, &guard) {
            return;
        }
    }

    let encoded = match data.encode(fields) {
        Ok(encoded) => encoded,
        Err(err) => {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
            return;
        }
    };

    let Some(response_body) = encoded.in_memory().map(<[u8]>::to_vec) else {
        ctx.set_status_code(STATUS_OK);
        if let Err(err) = encoded.send(ctx) {
            observe::set_error(ctx, &err);
            ctx.set_status_code(STATUS_INTERNAL_SERVER_ERROR);
        }
        return;
    };

    ctx.set_body(&response_body);
    ctx.set_status_code(STATUS_OK);

    if cache_enabled()
        && let (Some(hash), Some(store)) = (hash, cache::cache_store())
    {
        store.set_with_deps(entity, hash, response_body, &cache_deps(entity, b""));
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterError(String);

impl fmt::Display for FilterError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for FilterError {}

pub fn filter_value_to_string(val: &Value) -> Option<Vec<u8>> {
    match val {
        Value::String(s) => Some(s.as_bytes().to_vec()),
        Value::Number(f) => Some(strconv::format_float(*f, b'f', -1, 64).into_bytes()),
        Value::Bool(b) => Some(strconv::format_bool(*b).as_bytes().to_vec()),
        _ => None,
    }
}

fn parse_leaf(raw: &Map) -> Result<Filters, FilterError> {
    let mut leaf = Filters::new();
    for (field, v) in raw {
        let Value::Object(ops_raw) = v else {
            return Err(FilterError(format!("invalid filter for field {field}")));
        };

        let mut ops = Ops::new();
        for (op, val) in ops_raw {
            let Some(s) = filter_value_to_string(val) else {
                return Err(FilterError(format!(
                    "invalid value for {field}.{op}: expected a string, a number or a boolean"
                )));
            };

            ops.insert(op.as_bytes().to_vec(), s);
        }

        leaf.insert(field.as_bytes().to_vec(), ops);
    }

    Ok(leaf)
}

struct ParseFrame<'a> {
    items: &'a [Value],
    is_or: bool,
    next: usize,
    nodes: Vec<FilterNode>,
}

fn item_error(is_or: bool) -> FilterError {
    FilterError(if is_or {
        "or item must be an object".to_string()
    } else {
        "and item must be an object".to_string()
    })
}

pub fn parse_filter_node(raw: Option<&Map>) -> Result<FilterNode, FilterError> {
    let Some(root) = raw else {
        return Ok(FilterNode::default());
    };

    let mut stack: Vec<ParseFrame<'_>> = Vec::new();
    let mut cur: &Map = root;
    loop {
        let branch = match (cur.get("or"), cur.get("and")) {
            (Some(v), _) => Some((v, true)),
            (None, Some(v)) => Some((v, false)),
            (None, None) => None,
        };

        let mut result = match branch {
            Some((v, is_or)) => {
                let Value::Array(items) = v else {
                    return Err(FilterError(if is_or {
                        "or must be an array".to_string()
                    } else {
                        "and must be an array".to_string()
                    }));
                };

                match items.first() {
                    None => {
                        if is_or {
                            FilterNode::with_or(Vec::new())
                        } else {
                            FilterNode::with_and(Vec::new())
                        }
                    }
                    Some(Value::Object(m)) => {
                        stack.push(ParseFrame {
                            items,
                            is_or,
                            next: 0,
                            nodes: Vec::with_capacity(items.len()),
                        });
                        cur = m;
                        continue;
                    }
                    Some(_) => return Err(item_error(is_or)),
                }
            }
            None => FilterNode::with_leaf(parse_leaf(cur)?),
        };

        loop {
            let Some(top) = stack.last_mut() else {
                return Ok(result);
            };

            top.nodes.push(result);
            top.next += 1;
            let items = top.items;
            if top.next < items.len() {
                let Value::Object(m) = &items[top.next] else {
                    return Err(item_error(top.is_or));
                };
                cur = m;
                break;
            }

            let Some(done) = stack.pop() else {
                return Ok(FilterNode::default());
            };
            result = if done.is_or {
                FilterNode::with_or(done.nodes)
            } else {
                FilterNode::with_and(done.nodes)
            };
        }
    }
}
