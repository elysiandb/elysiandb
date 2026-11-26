use crate::engine;
use crate::httpd::RequestCtx;

pub fn exists_controller(ctx: &mut RequestCtx) {
    let entity = ctx.param("entity").to_vec();
    let id = ctx.param("id").to_vec();
    ctx.response.header.set("Content-Type", "application/json");

    let exists = engine::entity_exists(&entity, &id);
    let mut response: &[u8] = br#"{"exists": false}"#;
    if exists {
        response = br#"{"exists": true}"#;
    }

    ctx.set_body(response);
}
