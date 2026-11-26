use std::collections::BTreeMap;

use crate::acl;
use crate::api::{ByteFields, encode_document};
use crate::engine;
use crate::globals;
use crate::gocompat::json::{self, Map, Value};
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK};
use crate::schema;
use crate::security;

use super::core_entity::{references_core_entity, send_core_entity_forbidden};
use super::create::{nested_entity_types_in_body, purge_cache, send_validation_errors};

fn strict_schema() -> bool {
    globals::config().is_some_and(|cfg| cfg.api.schema.strict)
}

fn decode_object(body: &[u8]) -> Result<Map, ()> {
    let mut obj: Option<Map> = None;
    if json::unmarshal(body, &mut obj).is_err() {
        return Err(());
    }

    Ok(obj.unwrap_or_default())
}

pub fn update_by_id_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let id = ctx.param("id").to_vec();
    let body = ctx.post_body().to_vec();

    let nested_types = nested_entity_types_in_body(&body);
    if references_core_entity(&nested_types) {
        send_core_entity_forbidden(ctx);
        return;
    }

    let Some(stored) = engine::read_entity_by_id(&entity, &id) else {
        send_entity_not_found(ctx, None);
        return;
    };

    if strict_schema()
        && schema::is_manual_schema(&entity, None)
        && let Ok(obj) = decode_object(&body)
    {
        let errs = schema::validate_entity_update(&entity, Some(&stored), &obj, None);
        if !errs.is_empty() {
            send_validation_errors(ctx, &errs);

            return;
        }
    }

    if handle_single_update(ctx, &entity, &id, &body) {
        purge_cache(&entity, &nested_types);
        return;
    }

    send_bad_request(ctx);
}

fn decode_list(body: &[u8]) -> Option<Vec<Map>> {
    let mut list: Option<Vec<Option<Map>>> = None;
    if json::unmarshal(body, &mut list).is_err() {
        return None;
    }

    let list: Vec<Map> = list?.into_iter().map(Option::unwrap_or_default).collect();
    if list.is_empty() {
        return None;
    }

    Some(list)
}

pub fn update_list_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let body = ctx.post_body().to_vec();

    let nested_types = nested_entity_types_in_body(&body);
    if references_core_entity(&nested_types) {
        send_core_entity_forbidden(ctx);
        return;
    }

    let Some(list) = decode_list(&body) else {
        send_bad_request(ctx);
        return;
    };

    let (stored, missing) = read_entities_to_update(&entity, &list);
    if !missing.is_empty() {
        send_entity_not_found(ctx, Some(&missing));
        return;
    }

    if strict_schema() && schema::is_manual_schema(&entity, None) {
        for obj in &list {
            let id = match obj.get("id") {
                Some(Value::String(id)) => id.as_str(),
                _ => "",
            };
            let errs = schema::validate_entity_update(&entity, stored.get(id), obj, None);
            if !errs.is_empty() {
                send_validation_errors(ctx, &errs);

                return;
            }
        }
    }

    handle_batch_update(ctx, &entity, &list);
    purge_cache(&entity, &nested_types);
}

fn read_entities_to_update(entity: &[u8], list: &[Map]) -> (BTreeMap<String, Map>, Vec<String>) {
    let mut stored: BTreeMap<String, Map> = BTreeMap::new();
    let mut missing: Vec<String> = Vec::new();

    for obj in list {
        let Some(Value::String(id)) = obj.get("id") else {
            continue;
        };
        if id.is_empty() || stored.contains_key(id) || missing.contains(id) {
            continue;
        }

        match engine::read_entity_by_id(entity, id.as_bytes()) {
            None => missing.push(id.clone()),
            Some(data) => {
                stored.insert(id.clone(), data);
            }
        }
    }

    (stored, missing)
}

fn handle_single_update(ctx: &mut RequestCtx, entity: &[u8], id: &[u8], body: &[u8]) -> bool {
    let mut single: Option<Map> = None;
    if json::unmarshal(body, &mut single).is_err() {
        return false;
    }
    let Some(single) = single.filter(|s| !s.is_empty()) else {
        return false;
    };

    if !acl::can_update_entity(&security::get_current_username(ctx), entity, id, &single) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body(br#"{"error":"forbidden"}"#);

        return true;
    }

    let data = engine::update_entity_by_id(entity, id, &single);

    let response = match &data {
        Some(data) => encode_document(data, &ByteFields::id(id)).unwrap_or_default(),
        None => b"null".to_vec(),
    };
    send_json_response(ctx, response);

    true
}

fn handle_batch_update(ctx: &mut RequestCtx, entity: &[u8], list: &[Map]) {
    if !acl::can_update_list_of_entities(&security::get_current_username(ctx), entity, list) {
        ctx.set_status_code(STATUS_FORBIDDEN);
        ctx.set_body(br#"{"error":"forbidden"}"#);

        return;
    }

    let data = engine::update_list_of_entities(entity, list);
    let response = json::marshal(&data).unwrap_or_default();
    send_json_response(ctx, response);
}

fn send_entity_not_found(ctx: &mut RequestCtx, ids: Option<&[String]>) {
    let mut body: BTreeMap<String, Value> = BTreeMap::new();
    body.insert(
        "error".to_string(),
        Value::String("entity not found".to_string()),
    );
    if let Some(ids) = ids {
        body.insert(
            "ids".to_string(),
            Value::Array(ids.iter().map(|id| Value::String(id.clone())).collect()),
        );
    }

    let response = json::marshal(&body).unwrap_or_default();
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_NOT_FOUND);
    ctx.set_body(response);
}

fn send_json_response(ctx: &mut RequestCtx, response: Vec<u8>) {
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response);
}

fn send_bad_request(ctx: &mut RequestCtx) {
    ctx.set_status_code(STATUS_BAD_REQUEST);
    ctx.set_body_string("Invalid JSON");
}
