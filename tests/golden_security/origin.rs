use std::sync::Arc;

use crate::common::{read_jsonl, str_field};
use crate::tempdir::TempDir;
use crate::{assert_manifest_entry, bool_of, security_config};
use elysiandb::globals;
use elysiandb::httpd::RequestCtx;
use elysiandb::security;

const ALLOWED_ROWS: usize = 10;
const REJECTION_ROWS: usize = 210;

#[test]
fn origin_rows_match_go() {
    assert_manifest_entry("origin.jsonl");
    let dir = TempDir::new();
    globals::set_config(Some(Arc::new(security_config(
        &dir.path().to_string_lossy(),
        "user",
    ))));

    let (mut allowed, mut rejection) = (0, 0);
    for row in &read_jsonl("security/origin.jsonl") {
        match str_field(row, "fn") {
            "OriginIsAllowed" => {
                assert_eq!(
                    security::origin_is_allowed(str_field(row, "origin").as_bytes()),
                    bool_of(row, "allowed"),
                    "{row:?}"
                );
                allowed += 1;
            }
            "CrossOriginWriteIsRejected" => {
                let mut ctx = RequestCtx::new();
                ctx.request.header.set_method(str_field(row, "method"));
                ctx.request.header.set_host(str_field(row, "host"));
                let origin = str_field(row, "origin");
                if !origin.is_empty() {
                    ctx.request.header.set("Origin", origin);
                }
                let site = str_field(row, "sec_fetch_site");
                if !site.is_empty() {
                    ctx.request.header.set("Sec-Fetch-Site", site);
                }
                assert_eq!(
                    security::cross_origin_write_is_rejected(&ctx),
                    bool_of(row, "rejected"),
                    "{row:?}"
                );
                rejection += 1;
            }
            other => panic!("unknown function {other}"),
        }
    }
    assert_eq!((allowed, rejection), (ALLOWED_ROWS, REJECTION_ROWS));
}
