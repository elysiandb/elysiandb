use std::sync::Arc;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Value};
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::storage;
use elysiandb::transport::http::controller;

use crate::tempdir::TempDir;

struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn setup() -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.stats.enabled = false;
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::reset_store();

    Store { _dir: dir }
}

fn new_ctx(method: &str, path: &str, body: &str) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(path);
    req.header.set_method(method);
    if !body.is_empty() {
        req.set_body(body.as_bytes());
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn body(ctx: &RequestCtx) -> String {
    String::from_utf8_lossy(ctx.response.body_bytes()).into_owned()
}

fn put(key: &str, value: &str) {
    let mut ctx = new_ctx("PUT", &format!("/kv/{key}"), value);
    ctx.set_param("key", key);
    controller::put_key_controller(&mut ctx);
}

#[test]
fn test_health_controller() {
    let _store = setup();

    let mut ctx = new_ctx("GET", "/health", "");
    controller::health_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), 200, "health failed");
}

#[test]
fn test_put_and_get() {
    let _store = setup();

    let mut ctx = new_ctx("PUT", "/kv/foo", "bar");
    ctx.set_param("key", "foo");
    controller::put_key_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), 204, "put failed");

    let mut ctx = new_ctx("GET", "/kv/foo", "");
    ctx.set_param("key", "foo");
    controller::get_key_controller(&mut ctx);

    assert!(body(&ctx).contains(r#""value":"bar""#), "get failed");
}

#[test]
fn test_delete_key_controller() {
    let _store = setup();

    put("a", "1");

    let mut ctx = new_ctx("DELETE", "/kv/a", "");
    ctx.set_param("key", "a");
    controller::delete_key_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), 204, "delete failed");
}

#[test]
fn test_reset_controller() {
    let _store = setup();

    put("x", "y");

    let mut ctx = new_ctx("POST", "/reset", "");
    controller::reset_controller(&mut ctx);

    let mut ctx = new_ctx("GET", "/kv/x", "");
    ctx.set_param("key", "x");
    controller::get_key_controller(&mut ctx);

    assert!(body(&ctx).contains(r#""value":null"#), "reset failed");
}

#[test]
fn test_save_controller() {
    let _store = setup();

    let mut ctx = new_ctx("POST", "/save", "");
    controller::save_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), 204, "save failed");
}

#[test]
fn test_multi_get_controller() {
    let _store = setup();

    put("a", "1");
    put("b", "2");

    let mut ctx = new_ctx("GET", "/kv/mget?keys=a,b,c", "");
    controller::multi_get_controller(&mut ctx);

    let body = body(&ctx);
    assert!(
        body.contains(r#""key":"a""#)
            && body.contains(r#""key":"b""#)
            && body.contains(r#""key":"c""#),
        "mget failed"
    );
}

#[test]
fn test_wildcard_get_controller() {
    let _store = setup();

    put("a1", "x");
    put("a2", "y");

    let mut ctx = new_ctx("GET", "/kv/a*", "");
    ctx.set_param("key", "a*");
    controller::get_key_controller(&mut ctx);

    let body = body(&ctx);
    assert!(
        body.contains("a1") && body.contains("a2"),
        "wildcard get failed"
    );
    assert_eq!(
        body, r#"[{"key":"a1","value":"x"},{"key":"a2","value":"y"}]"#,
        "wildcard matches come in ascending key order"
    );
}

#[test]
fn test_stats_controller() {
    let _store = setup();

    let mut ctx = new_ctx("GET", "/stats", "");
    controller::stats_controller(&mut ctx);

    assert!(
        json::unmarshal_value(ctx.response.body_bytes()).is_ok(),
        "invalid stats json"
    );
}

#[test]
fn test_get_config_controller() {
    let _store = setup();

    let mut ctx = new_ctx("GET", "/config", "");
    controller::get_config_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        200,
        "expected 200, got {}",
        ctx.response.status_code()
    );

    let raw = ctx.response.body_bytes();
    assert!(!raw.is_empty(), "empty body");

    let value = match json::unmarshal_value(raw) {
        Ok(v) => v,
        Err(e) => panic!("invalid json: {e}"),
    };
    let Value::Object(out) = &value else {
        panic!("expected an object, got {value:?}");
    };

    assert!(!out.is_empty(), "expected non-empty config object");

    let found = out.keys().any(|k| {
        let k = k.to_lowercase();
        k.contains("store") || k.contains("folder") || k.contains("shard")
    });

    assert!(
        found,
        "config json does not contain expected storage-related keys: {out:?}"
    );
}
