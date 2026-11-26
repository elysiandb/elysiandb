use std::time::Instant;

use crate::api::{self, MigrationError};
use crate::engine;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_NOT_FOUND, STATUS_OK};
use crate::kv;
use crate::log;
use crate::transport::http::observe;

fn quoted_entity_message(prefix: &[u8], entity: &[u8], suffix: &[u8]) -> Vec<u8> {
    let mut body = prefix.to_vec();
    body.extend_from_slice(entity);
    body.extend_from_slice(suffix);
    body
}

pub fn migrate_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let entity = ctx.param("entity").to_vec();

    if !engine::entity_type_exists(&entity) {
        ctx.set_status_code(STATUS_NOT_FOUND);
        ctx.set_body(quoted_entity_message(
            br#"{"error" : "Entity '"#,
            &entity,
            br#"' does not exist."}"#,
        ));

        return;
    }

    let migration_queries = api::parse_migration_query(ctx.post_body(), &entity);

    if migration_queries.is_empty() {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body_string(
            r#"{"error" : "No valid migration queries found in the request body."}"#,
        );
        return;
    }

    let start = Instant::now();
    let (updated, result) = api::execute_migrations_count(&migration_queries);
    if let Err(err) = result {
        if let MigrationError::NotAnObject(cause) = &err {
            std::panic::panic_any(cause.to_string());
        }

        observe::audit(
            ctx,
            "migration failed",
            kv!["entity" => entity.as_slice(), "updated" => updated, "err" => log::display(&err)],
        );
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(format!(r#"{{"error" : "Migration failed: {err}"}}"#));
        return;
    }

    observe::audit(
        ctx,
        "migration done",
        kv![
            "entity" => entity.as_slice(),
            "queries" => migration_queries.len(),
            "updated" => updated,
            "duration_ms" => log::since(start),
        ],
    );
    ctx.set_status_code(STATUS_OK);
    ctx.set_body(quoted_entity_message(
        br#"{"message" : "Entity '"#,
        &entity,
        br#"' migrated successfully."}"#,
    ));
}
