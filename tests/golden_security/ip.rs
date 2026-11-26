use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;

use crate::common::{read_jsonl, str_field};
use crate::tempdir::TempDir;
use crate::{assert_manifest_entry, bool_of, security_config};
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::security;

const PARSE_ROWS: usize = 16;
const NETWORK_ROWS: usize = 8;
const SOURCE_ROWS: usize = 10;

fn check_parse_ip(row: &Value) {
    let parsed = security::parse_ip(str_field(row, "in").as_bytes());
    assert_eq!(parsed.is_some(), bool_of(row, "valid"), "{row:?}");
    let Some(ip) = parsed else {
        return;
    };
    assert_eq!(ip.to_string(), str_field(row, "string"), "{row:?}");
    assert_eq!(ip.is_loopback(), bool_of(row, "loopback"), "{row:?}");
    assert_eq!(matches!(ip, IpAddr::V4(_)), bool_of(row, "is4"), "{row:?}");
}

fn check_parse_network(row: &Value) {
    let network = security::parse_network(str_field(row, "network").as_bytes());
    assert_eq!(network.is_some(), bool_of(row, "parsed"), "{row:?}");
    let Some(network) = network else {
        assert!(row.get("contains").is_none(), "{row:?}");
        return;
    };
    let ip = security::parse_ip(str_field(row, "ip").as_bytes()).expect("ip");
    assert_eq!(network.contains(ip), bool_of(row, "contains"), "{row:?}");
}

fn check_login_source(row: &Value) {
    let mut req = Request::new();
    if let Some(headers) = row.get("headers").and_then(Value::as_object) {
        assert!(headers.len() <= 1, "{row:?}");
        for (name, value) in headers {
            req.header.set(name, value.as_str().expect("header value"));
        }
    }
    let peer: IpAddr = str_field(row, "peer").parse().expect("peer");
    let mut ctx = RequestCtx::new();
    ctx.init(&req, Some(SocketAddr::new(peer, 4000)));

    let src = security::login_source_from_request(&ctx);
    assert_eq!(src.local, bool_of(row, "local"), "{row:?}");
    assert_eq!(src.client_key(), str_field(row, "client_key"), "{row:?}");
    assert_eq!(src.to_string(), str_field(row, "ip"), "{row:?}");
}

#[test]
fn ip_rows_match_go() {
    assert_manifest_entry("ip.jsonl");
    let dir = TempDir::new();
    globals::set_config(Some(Arc::new(security_config(
        &dir.path().to_string_lossy(),
        "user",
    ))));

    let (mut parse, mut network, mut source) = (0, 0, 0);
    for row in &read_jsonl("security/ip.jsonl") {
        match str_field(row, "fn") {
            "ParseIP" => {
                check_parse_ip(row);
                parse += 1;
            }
            "parseNetwork" => {
                check_parse_network(row);
                network += 1;
            }
            "LoginSourceFromRequest" => {
                check_login_source(row);
                source += 1;
            }
            other => panic!("unknown function {other}"),
        }
    }
    assert_eq!(
        (parse, network, source),
        (PARSE_ROWS, NETWORK_ROWS, SOURCE_ROWS)
    );
}
