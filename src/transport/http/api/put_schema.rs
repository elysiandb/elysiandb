use crate::api::{self, ByteFields};
use crate::engine;
use crate::gocompat::json::{self, Map, Value};
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_NOT_FOUND, STATUS_OK};
use crate::kv;
use crate::transport::http::observe;

pub(crate) enum SchemaPayloadError {
    InvalidJson,
    FieldsNotAnObject,
}

pub(crate) fn schema_fields(body: &[u8]) -> Result<Map, SchemaPayloadError> {
    let mut payload: Option<Map> = None;
    if json::unmarshal(body, &mut payload).is_err() {
        return Err(SchemaPayloadError::InvalidJson);
    }

    match payload.as_mut().and_then(|p| p.get_mut("fields")) {
        Some(Value::Object(fields)) => Ok(std::mem::take(fields)),
        _ => Err(SchemaPayloadError::FieldsNotAnObject),
    }
}

pub(crate) fn encode_storable(entity: &[u8], storable: &Map) -> Vec<u8> {
    api::encode_document(storable, &ByteFields::id(entity)).unwrap_or_default()
}

pub fn put_schema_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();

    if !engine::entity_type_exists(&entity) {
        ctx.set_status_code(STATUS_NOT_FOUND);
        ctx.set_body_string(r#"{"error":"entity not found"}"#);
        return;
    }

    let fields_raw = match schema_fields(ctx.post_body()) {
        Ok(fields) => fields,
        Err(SchemaPayloadError::InvalidJson) => {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"invalid json"}"#);

            return;
        }
        Err(SchemaPayloadError::FieldsNotAnObject) => {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"schema.fields must be an object"}"#);

            return;
        }
    };

    let storable = engine::update_entity_schema(&entity, &fields_raw);

    observe::audit(ctx, "schema changed", kv!["entity" => entity.as_slice()]);
    let out = encode_storable(&entity, &storable);
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(out);
}
