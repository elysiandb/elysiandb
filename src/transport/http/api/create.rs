use crate::acl;
use crate::api::{self, ByteFields, WriteSimulation, encode_document};
use crate::cache;
use crate::engine;
use crate::globals;
use crate::gocompat::json::{self, Map, Value};
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_OK};
use crate::schema::{self, ValidationError};
use crate::security;
use crate::uuid;

use super::core_entity::{references_core_entity, send_core_entity_forbidden};
use super::response::cache_enabled;

fn strict_schema() -> bool {
    globals::config().is_some_and(|cfg| cfg.api.schema.strict)
}

pub fn create_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let body = ctx.post_body().to_vec();

    let nested_types = nested_entity_types_in_body(&body);
    if references_core_entity(&nested_types) {
        send_core_entity_forbidden(ctx);
        return;
    }

    if strict_schema()
        && schema::is_manual_schema(&entity, None)
        && let Ok(tmp) = json::unmarshal_value(&body)
    {
        if let Value::Object(obj) = &tmp {
            let errs = schema::validate_entity(&entity, obj, None);
            if !errs.is_empty() {
                send_validation_errors(ctx, &errs);

                return;
            }
        }

        if let Value::Array(arr) = &tmp {
            for it in arr {
                if let Value::Object(obj) = it {
                    let errs = schema::validate_entity(&entity, obj, None);
                    if !errs.is_empty() {
                        send_validation_errors(ctx, &errs);

                        return;
                    }
                }
            }
        }
    }

    if handle_single_entity(ctx, &entity, &body) {
        finalize_create(&entity, &nested_types);
        return;
    }

    if handle_entity_list(ctx, &entity, &body) {
        finalize_create(&entity, &nested_types);
        return;
    }

    ctx.set_status_code(STATUS_BAD_REQUEST);
    ctx.set_body_string("Invalid JSON");
}

pub(crate) fn send_validation_errors(ctx: &mut RequestCtx, errs: &[ValidationError]) {
    let b = json::marshal(errs).unwrap_or_default();
    ctx.set_status_code(STATUS_BAD_REQUEST);
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_body(b);
}

fn owner_fields(username: &[u8]) -> ByteFields {
    if security::user_authentication_is_enabled() {
        return ByteFields::new().with(acl::USERNAME_FIELD, username);
    }

    ByteFields::new()
}

fn ensure_id(data: &mut Map) {
    let has_id = matches!(data.get("id"), Some(Value::String(id)) if !id.is_empty());
    if !has_id {
        data.insert("id".to_string(), Value::String(uuid::new_v4()));
    }
}

fn handle_single_entity(ctx: &mut RequestCtx, entity: &[u8], body: &[u8]) -> bool {
    let mut data: Option<Map> = None;
    if json::unmarshal(body, &mut data).is_err() {
        return false;
    }
    let Some(mut data) = data.filter(|d| !d.is_empty()) else {
        return false;
    };

    let username = security::get_current_username(ctx);
    if !acl::can_write_entity(&username, entity, &data) {
        send_forbidden(ctx);
        return true;
    }

    ensure_id(&mut data);

    let fields = owner_fields(&username);

    let errors = engine::write_entity_bytes(entity, &mut data, &fields);
    if !errors.is_empty() {
        let response = json::marshal(&errors).unwrap_or_default();
        ctx.response.header.set("Content-Type", "application/json");
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(response);
        return true;
    }

    let response = encode_document(&data, &fields).unwrap_or_default();
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response);

    true
}

fn decode_object_list(body: &[u8]) -> Option<Vec<Option<Map>>> {
    let mut list: Option<Vec<Option<Map>>> = None;
    if json::unmarshal(body, &mut list).is_err() {
        return None;
    }

    list
}

fn handle_entity_list(ctx: &mut RequestCtx, entity: &[u8], body: &[u8]) -> bool {
    let Some(items) = decode_object_list(body).filter(|l| !l.is_empty()) else {
        return false;
    };

    let mut list: Vec<Map> = Vec::with_capacity(items.len());
    for item in items {
        let Some(item) = item else {
            return false;
        };
        list.push(item);
    }

    let username = security::get_current_username(ctx);
    if !acl::can_write_list_of_entities(&username, entity, &list) {
        send_forbidden(ctx);
        return true;
    }

    let fields = owner_fields(&username);
    for item in list.iter_mut() {
        ensure_id(item);
        fields.apply(item);
    }

    let (mut validation_errors, valid) = simulate_list_write(entity, &list);
    if valid {
        validation_errors = engine::write_list_of_entities_bytes(entity, &mut list, &fields);
    }

    if has_validation_errors(&validation_errors) {
        send_list_validation_errors(ctx, &validation_errors);

        return true;
    }

    let mut response = vec![b'['];
    for (i, item) in list.iter().enumerate() {
        if i > 0 {
            response.push(b',');
        }
        response.extend_from_slice(&encode_document(item, &fields).unwrap_or_default());
    }
    response.push(b']');
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(response);

    true
}

fn simulate_list_write(entity: &[u8], list: &[Map]) -> (Vec<Vec<ValidationError>>, bool) {
    let mut simulation = WriteSimulation::new(
        Box::new(engine::get_entity_schema),
        Box::new(engine::read_entity_by_id),
    );

    let validation_errors: Vec<Vec<ValidationError>> = list
        .iter()
        .map(|item| simulation.write(entity, item))
        .collect();

    let valid = !has_validation_errors(&validation_errors);
    (validation_errors, valid)
}

fn has_validation_errors(validation_errors: &[Vec<ValidationError>]) -> bool {
    validation_errors.iter().any(|errs| !errs.is_empty())
}

fn send_list_validation_errors(ctx: &mut RequestCtx, validation_errors: &[Vec<ValidationError>]) {
    let response = json::marshal(validation_errors).unwrap_or_default();
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_BAD_REQUEST);
    ctx.set_body(response);
}

pub(crate) fn nested_entity_types_in_body(body: &[u8]) -> Vec<Vec<u8>> {
    match json::unmarshal_value(body) {
        Ok(data) => api::nested_entity_types(&data),
        Err(_) => Vec::new(),
    }
}

pub(crate) fn purge_cache<E: AsRef<[u8]>>(entity: &[u8], nested_types: &[E]) {
    if !cache_enabled() {
        return;
    }

    let Some(store) = cache::cache_store() else {
        return;
    };

    store.purge(entity);
    for nested in nested_types {
        store.purge(nested.as_ref());
    }
}

pub(crate) fn send_forbidden(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_FORBIDDEN);
    ctx.set_body_string(r#"{"error":"forbidden"}"#);
}

fn finalize_create(entity: &[u8], nested_types: &[Vec<u8>]) {
    purge_cache(entity, nested_types);

    let _ = acl::init_acl();
}
