use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use crate::assert_manifest_entry;
use crate::bool_of;
use crate::common::{read_jsonl, str_field};
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::security::{self, Session};

const ROWS: usize = 12;

fn variant(name: &str) -> (bool, &'static str) {
    match name {
        "plain" => (false, ""),
        "peer-loopback" => (true, ""),
        "proto-https" => (false, "https"),
        "proto-list" => (false, "HTTPS, http"),
        "proto-spaces" => (false, " https "),
        "proto-http-first" => (false, "http, https"),
        other => panic!("unknown request variant {other}"),
    }
}

#[test]
fn cookie_rows_match_go() {
    assert_manifest_entry("cookies.jsonl");
    let mut checked = 0;
    for row in &read_jsonl("security/cookies.jsonl") {
        let (with_peer, proto) = variant(str_field(row, "request"));

        let mut ctx = RequestCtx::new();
        if !proto.is_empty() {
            ctx.request.header.set("X-Forwarded-Proto", proto);
        }
        if with_peer {
            let req = Request::new();
            ctx.init(
                &req,
                Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1)),
            );
        }

        if bool_of(row, "clear") {
            security::clear_session_cookie(&mut ctx);
        } else {
            let session = Session {
                id: "0123456789abcdef".to_string(),
                ..Session::default()
            };
            security::set_session_cookie(&mut ctx, &session);
        }

        assert_eq!(
            String::from_utf8_lossy(ctx.response.header.peek("Set-Cookie")),
            str_field(row, "set_cookie"),
            "{row:?}"
        );
        checked += 1;
    }
    assert_eq!(checked, ROWS);
}
