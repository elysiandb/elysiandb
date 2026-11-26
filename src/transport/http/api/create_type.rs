use crate::acl;
use crate::engine;
use crate::gocompat::errors;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_OK};
use crate::kv;
use crate::transport::http::observe;

use super::put_schema::{SchemaPayloadError, encode_storable, schema_fields};

pub fn create_type_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    if let Err(err) = engine::create_entity_type(&entity) {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(errors::message_bytes(err.as_ref()));

        return;
    }

    let fields_raw = match schema_fields(ctx.post_body()) {
        Ok(fields) => fields,
        Err(SchemaPayloadError::InvalidJson) => {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"invalid json"}"#);
            let _ = engine::delete_entity_type(&entity);

            return;
        }
        Err(SchemaPayloadError::FieldsNotAnObject) => {
            ctx.set_status_code(STATUS_BAD_REQUEST);
            ctx.set_body_string(r#"{"error":"schema.fields must be an object"}"#);
            let _ = engine::delete_entity_type(&entity);

            return;
        }
    };

    let storable = engine::update_entity_schema(&entity, &fields_raw);

    let _ = acl::init_acl();

    observe::audit(
        ctx,
        "entity type created",
        kv!["entity" => entity.as_slice()],
    );
    let out = encode_storable(&entity, &storable);
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(out);
}
