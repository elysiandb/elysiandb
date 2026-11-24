use std::net::{IpAddr, SocketAddr};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::common::{read_jsonl, str_field};
use crate::tempdir::TempDir;
use crate::{assert_manifest_entry, int_of};
use elysiandb::acl;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::httpd::{Handler, Request, RequestCtx};
use elysiandb::security;
use elysiandb::storage;
use elysiandb::transport::http::adminui;
use elysiandb::yaml::StringList;

const ROWS: usize = 51;
const REPLAYED: usize = 40;
const PASSED_THROUGH: usize = 11;

const RECORDED_HEADERS: [&str; 14] = [
    "Content-Type",
    "Set-Cookie",
    "Retry-After",
    "Access-Control-Allow-Origin",
    "Access-Control-Allow-Methods",
    "Access-Control-Allow-Headers",
    "Access-Control-Allow-Credentials",
    "Vary",
    "Allow",
    "X-Content-Type-Options",
    "X-Frame-Options",
    "Referrer-Policy",
    "Content-Security-Policy",
    "Cache-Control",
];

const DOWNSTREAM_HEADERS: [&str; 2] = ["Content-Type", "Allow"];

const PASS_THROUGH: [(&str, &str); PASSED_THROUGH] = [
    ("", "health"),
    ("", "list-no-credentials"),
    ("", "kv-no-credentials"),
    ("", "same-origin-write"),
    ("", "config"),
    ("basic", "health"),
    ("basic", "basic-ok"),
    ("basic", "basic-admin-route"),
    ("token", "health"),
    ("token", "token-ok"),
    ("user", "health"),
];

struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn open_store(mode: &str) -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = !mode.is_empty();
    cfg.security.authentication.mode = mode.to_string();
    if mode == "token" {
        cfg.security.authentication.token = "golden-token".to_string();
    }
    cfg.server.http.cors.allowed_origins =
        StringList(Some(vec!["http://allowed.test".to_string()]));
    cfg.server.http.trusted_proxies = StringList(Some(vec!["10.0.0.0/8".to_string()]));
    cfg.api.schema.enabled = true;
    cfg.api.hooks.enabled = true;
    cfg.stats.enabled = true;
    cfg.log.level = "error".to_string();
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::load_json_db();
    security::reset_login_attempts();
    let _ = security::init_admin_user_if_not_exists();
    let _ = acl::init_acl();

    Store { _dir: dir }
}

fn chain(mode: &str, reached: &Arc<AtomicBool>) -> Handler {
    let flag = reached.clone();
    let stub: Handler = Arc::new(move |_ctx: &mut RequestCtx| flag.store(true, Ordering::SeqCst));
    let authenticated = security::authenticate(stub.clone());
    let admin = security::authenticate_admin(stub.clone());
    let login: Handler = Arc::new(adminui::login_controller);
    let me = adminui::admin_auth(Arc::new(adminui::me_controller));
    let user_mode = mode == "user";

    let inner: Handler = Arc::new(move |ctx: &mut RequestCtx| {
        let route = (ctx.method().to_vec(), ctx.path().to_vec());
        match (route.0.as_slice(), route.1.as_slice()) {
            (b"GET", b"/health") => stub(ctx),
            (b"GET" | b"POST", b"/api/book") | (b"DELETE", b"/api/book/x") => authenticated(ctx),
            (b"GET", b"/kv/x" | b"/config") => admin(ctx),
            (b"POST", b"/api/security/login") if user_mode => login(ctx),
            (b"GET", b"/api/security/me") if user_mode => me(ctx),
            (method, path) => panic!(
                "no route for {} {}",
                String::from_utf8_lossy(method),
                String::from_utf8_lossy(path)
            ),
        }
    });

    security::protect(inner)
}

fn mask(value: &[u8]) -> String {
    let text = String::from_utf8_lossy(value).into_owned();
    let Some(start) = text.find("edb_session=") else {
        return text;
    };
    let id_start = start + "edb_session=".len();
    let id_len = text[id_start..]
        .bytes()
        .take_while(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        .count();
    if id_len == 0 {
        return text;
    }
    format!("{}<SID>{}", &text[..id_start], &text[id_start + id_len..])
}

fn request(row: &Value) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(str_field(row, "path"));
    req.header.set_method(str_field(row, "method"));
    req.header.set_host("h");
    if let Some(headers) = row.get("request_headers").and_then(Value::as_array) {
        for h in headers {
            let h = h.as_str().expect("header");
            let (k, v) = h.split_once(": ").expect("header shape");
            req.header.set(k, v);
        }
    }
    let body = str_field(row, "body");
    if !body.is_empty() {
        req.set_body_string(body);
    }

    let peer: IpAddr = str_field(row, "peer").parse().expect("peer");
    let mut ctx = RequestCtx::new();
    ctx.init(&req, Some(SocketAddr::new(peer, 40000)));
    ctx
}

fn expected_header<'a>(row: &'a Value, name: &str) -> &'a str {
    row.get("headers")
        .and_then(|h| h.get(name))
        .and_then(Value::as_str)
        .unwrap_or("")
}

#[test]
fn http_rows_replayed_through_the_security_chain() {
    assert_manifest_entry("http.jsonl");
    let rows = read_jsonl("security/http.jsonl");
    let (mut replayed, mut passed) = (0, 0);
    let mut current: Option<(String, Store, Handler, Arc<AtomicBool>)> = None;

    for row in &rows {
        let mode = str_field(row, "mode");
        let name = str_field(row, "name");
        if current.as_ref().is_none_or(|(m, _, _, _)| m != mode) {
            drop(current.take());
            let store = open_store(mode);
            let reached = Arc::new(AtomicBool::new(false));
            let handler = chain(mode, &reached);
            current = Some((mode.to_string(), store, handler, reached));
        }
        let Some((_, _, handler, reached)) = &current else {
            unreachable!("store is open");
        };

        reached.store(false, Ordering::SeqCst);
        let mut ctx = request(row);
        handler(&mut ctx);

        let pass_through = PASS_THROUGH.contains(&(mode, name));
        assert_eq!(
            reached.load(Ordering::SeqCst),
            pass_through,
            "{mode}/{name}: security decision"
        );

        for header in RECORDED_HEADERS {
            if pass_through && DOWNSTREAM_HEADERS.contains(&header) {
                continue;
            }
            assert_eq!(
                mask(ctx.response.header.peek(header)),
                expected_header(row, header),
                "{mode}/{name}: {header}"
            );
        }

        if pass_through {
            passed += 1;
            continue;
        }

        assert_eq!(
            i64::from(ctx.response.status_code()),
            int_of(row, "status"),
            "{mode}/{name}: status"
        );
        assert_eq!(
            mask(ctx.response.body()),
            str_field(row, "response_body"),
            "{mode}/{name}: body"
        );
        replayed += 1;
    }
    drop(current);

    assert_eq!(rows.len(), ROWS);
    assert_eq!((replayed, passed), (REPLAYED, PASSED_THROUGH));
}
