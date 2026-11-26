use std::collections::BTreeSet;
use std::io::Write;

use crate::acl;
use crate::gocompat::json;
use crate::httpd::{RequestCtx, STATUS_OK};
use crate::schema;
use crate::security;
use crate::storage;

pub const ACCOUNTS_NOT_IMPORTED_MESSAGE: &str =
    "accounts and ACLs are not imported, use a backup and restore to move them";

pub fn account_entities() -> [&'static str; 2] {
    [acl::ACL_ENTITY, security::USER_ENTITY]
}

pub fn export_controller(ctx: &mut RequestCtx) {
    ctx.response.header.set("Content-Type", "application/json");
    ctx.set_status_code(STATUS_OK);

    stream_export(ctx);
}

fn stream_export(ctx: &mut RequestCtx) {
    let Some(e) = storage::engine() else {
        ctx.set_body_string("{}");
        return;
    };

    let snaps = e.snapshot_all();
    let mut skipped: BTreeSet<&[u8]> = BTreeSet::new();
    skipped.insert(schema::SCHEMA_ENTITY.as_bytes());
    for entity in account_entities() {
        skipped.insert(entity.as_bytes());
    }

    let mut names: Vec<Vec<u8>> = e
        .types()
        .into_iter()
        .filter(|t| !skipped.contains(t.as_slice()))
        .collect();
    names.sort();
    names.dedup();

    ctx.set_body_stream_writer(move |w| {
        let _ = w.write_all(b"{");
        for (i, name) in names.iter().enumerate() {
            if i > 0 {
                let _ = w.write_all(b",");
            }

            let key = json::marshal(name).unwrap_or_default();
            let _ = w.write_all(&key);
            let _ = w.write_all(b":[");

            if let Some(snap) = snaps.get(name) {
                let mut first = true;
                snap.ascend(0, |d| {
                    let Ok(b) = d.bytes() else {
                        return true;
                    };

                    if !first {
                        let _ = w.write_all(b",");
                    }
                    first = false;
                    let _ = w.write_all(&b);

                    true
                });
            }

            let _ = w.write_all(b"]");
            let _ = w.flush();
        }
        let _ = w.write_all(b"}");
    });
}
