use std::collections::BTreeMap;
use std::time::Instant;

use crate::acl;
use crate::engine;
use crate::gocompat::json::{self, Map, RawMessage};
use crate::hook;
use crate::httpd::{RequestCtx, STATUS_BAD_REQUEST, STATUS_INTERNAL_SERVER_ERROR, STATUS_OK};
use crate::kv;
use crate::log;
use crate::transport::http::observe;

use super::core_entity::reject_core_entity_references;
use super::create::purge_cache;
use super::export::{ACCOUNTS_NOT_IMPORTED_MESSAGE, account_entities};

type Dump = BTreeMap<Vec<u8>, Vec<Option<Map>>>;

type RawDump = BTreeMap<Vec<u8>, Option<Vec<Option<Map>>>>;

fn decode_dump(body: &[u8]) -> Option<Dump> {
    let mut raw: Option<RawDump> = None;
    if json::unmarshal(body, &mut raw).is_err() {
        return None;
    }

    Some(
        raw.unwrap_or_default()
            .into_iter()
            .map(|(entity, items)| (entity, items.unwrap_or_default()))
            .collect(),
    )
}

pub fn import_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");

    let Some(mut dump) = decode_dump(ctx.post_body()) else {
        ctx.set_status_code(STATUS_BAD_REQUEST);
        ctx.set_body(br#"{"error":"invalid JSON dump"}"#);
        return;
    };

    let ignored = remove_account_entities(&mut dump);
    remove_internal_entities(&mut dump);

    let empty = Map::new();
    for items in dump.values() {
        for item in items {
            if reject_core_entity_references(ctx, item.as_ref().unwrap_or(&empty)) {
                return;
            }
        }
    }

    let rejected = engine::validate_import(&dump);
    if !rejected.is_empty() {
        send_import_response(
            ctx,
            STATUS_BAD_REQUEST,
            vec![
                (
                    "error",
                    json_text(&"invalid documents, nothing was imported".to_string()),
                ),
                ("rejected", json_text(&rejected)),
            ],
            &ignored,
        );
        return;
    }

    let start = Instant::now();
    let rejected = engine::import_all(&mut dump);
    for entity in dump.keys() {
        purge_cache::<Vec<u8>>(entity, &[]);
    }

    let _ = acl::init_acl();

    if !rejected.is_empty() {
        observe::set_error(
            ctx,
            &format!("{} documents could not be imported", rejected.len()),
        );
        observe::audit(
            ctx,
            "import done",
            kv![
                "entity_types" => dump.len(),
                "entities" => count_documents(&dump),
                "rejected" => rejected.len(),
                "duration_ms" => log::since(start),
            ],
        );
        send_import_response(
            ctx,
            STATUS_INTERNAL_SERVER_ERROR,
            vec![
                (
                    "error",
                    json_text(&"some documents could not be imported".to_string()),
                ),
                ("rejected", json_text(&rejected)),
            ],
            &ignored,
        );
        return;
    }

    observe::audit(
        ctx,
        "import done",
        kv![
            "entity_types" => dump.len(),
            "entities" => count_documents(&dump),
            "rejected" => 0i64,
            "duration_ms" => log::since(start),
        ],
    );
    send_import_response(
        ctx,
        STATUS_OK,
        vec![("status", json_text(&"import completed".to_string()))],
        &ignored,
    );
}

fn json_text<T: json::Encode + ?Sized>(v: &T) -> RawMessage {
    RawMessage::new(json::marshal(v).unwrap_or_default())
}

fn count_documents(dump: &Dump) -> usize {
    dump.values().map(Vec::len).sum()
}

fn remove_account_entities(dump: &mut Dump) -> Vec<Vec<u8>> {
    let mut ignored = Vec::new();
    for entity in account_entities() {
        if dump.remove(entity.as_bytes()).is_some() {
            ignored.push(entity.as_bytes().to_vec());
        }
    }

    ignored
}

fn remove_internal_entities(dump: &mut Dump) {
    dump.retain(|entity, _| {
        !engine::is_core_entity_type(entity) || entity.as_slice() == hook::HOOK_ENTITY.as_bytes()
    });
}

fn send_import_response(
    ctx: &mut RequestCtx,
    status: u16,
    response: Vec<(&str, RawMessage)>,
    ignored: &[Vec<u8>],
) {
    let mut response: BTreeMap<String, RawMessage> = response
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    if !ignored.is_empty() {
        response.insert("ignored".to_string(), json_text(ignored));
        response.insert(
            "message".to_string(),
            json_text(&ACCOUNTS_NOT_IMPORTED_MESSAGE.to_string()),
        );
    }

    let body = json::marshal(&response).unwrap_or_default();
    ctx.set_status_code(status);
    ctx.set_body(body);
}
