use std::sync::Arc;
use std::time::Duration;

use elysiandb::api;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::httpd::{
    Request, RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK,
};
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;
use elysiandb::transport::http::api::acl as http_acl;

use crate::tempdir::TempDir;

const HOUR: Duration = Duration::from_secs(3600);

struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn object(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn setup_acl_test() -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::load_json_db();

    api::delete_all();
    let _ = api::create_entity_type(b"doc");
    api::update_entity_schema(
        b"doc",
        &object(vec![(
            "title",
            Value::Object(object(vec![("type", s("string"))])),
        )]),
    );

    api::write_entity(
        b"_elysiandb_core_user",
        &mut object(vec![
            ("id", s("admin")),
            ("username", s("admin")),
            ("password", s("admin-password-hash")),
            ("role", s("admin")),
        ]),
    );

    let permissions = [
        "create",
        "read",
        "update",
        "delete",
        "owning_read",
        "owning_write",
        "owning_delete",
        "owning_update",
    ]
    .into_iter()
    .map(|p| (p, Value::Bool(true)))
    .collect();
    api::write_entity(
        b"_elysiandb_core_acl",
        &mut object(vec![
            ("id", s("admin::doc")),
            ("username", s("admin")),
            ("entity", s("doc")),
            ("permissions", Value::Object(object(permissions))),
        ]),
    );

    Store { _dir: dir }
}

fn ensure_user(username: &str, role: Role) {
    if !security::basic_user_exists(username.as_bytes()) {
        let _ = security::create_basic_user(&BasicUser {
            username: username.as_bytes().to_vec(),
            password: format!("{username}-password").into_bytes(),
            role,
        });
    }
}

fn session_ctx(username: &str, role: Role, method: &str, body: Option<&[u8]>) -> RequestCtx {
    ensure_user(username, role.clone());
    let s = security::create_session(username.as_bytes(), role, HOUR).unwrap_or_default();

    let mut req = Request::new();
    req.header.set_method(method);
    req.header.set_cookie(security::SESSION_COOKIE_NAME, &s.id);
    if let Some(body) = body {
        req.set_body(body);
    }

    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn admin_ctx(method: &str, body: Option<&[u8]>) -> RequestCtx {
    session_ctx("admin", Role::admin(), method, body)
}

fn user_ctx(method: &str) -> RequestCtx {
    session_ctx("user", Role::user(), method, None)
}

fn set_params(ctx: &mut RequestCtx, entity: Option<&str>, username: &str) {
    if let Some(entity) = entity {
        ctx.set_user_value("entity", entity.to_string());
    }
    ctx.set_user_value("user_name", username.to_string());
}

#[test]
fn get_acl_for_username_and_entity_ok() {
    let _store = setup_acl_test();

    let mut ctx = admin_ctx("GET", None);
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::get_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn get_acl_for_username_and_entity_not_found() {
    let _store = setup_acl_test();

    let mut ctx = admin_ctx("GET", None);
    set_params(&mut ctx, Some("missing"), "admin");

    http_acl::get_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_NOT_FOUND, "expected 404");
}

#[test]
fn get_acl_for_username_and_entity_forbidden() {
    let _store = setup_acl_test();

    let mut ctx = user_ctx("GET");
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::get_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn get_all_acl_for_username_ok() {
    let _store = setup_acl_test();

    let mut ctx = admin_ctx("GET", None);
    set_params(&mut ctx, None, "admin");

    http_acl::get_all_acl_for_username(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    let mut out: Vec<Map> = Vec::new();
    let _ = json::unmarshal(ctx.response.body(), &mut out);

    assert_eq!(out.len(), 1, "expected 1 acl");
}

#[test]
fn get_all_acl_for_username_forbidden() {
    let _store = setup_acl_test();

    let mut ctx = user_ctx("GET");
    set_params(&mut ctx, None, "admin");

    http_acl::get_all_acl_for_username(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn update_acl_for_username_and_entity_ok() {
    let _store = setup_acl_test();

    let body = json::marshal(&object(vec![(
        "permissions",
        Value::Object(object(vec![("read", Value::Bool(true))])),
    )]))
    .expect("marshal");

    let mut ctx = admin_ctx("POST", Some(&body));
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::update_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn update_acl_for_username_and_entity_invalid_json() {
    let _store = setup_acl_test();

    let mut ctx = admin_ctx("POST", Some(b"{bad"));
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::update_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn update_acl_for_username_and_entity_forbidden() {
    let _store = setup_acl_test();

    let mut ctx = user_ctx("POST");
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::update_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn set_default_acl_for_username_and_entity_ok() {
    let _store = setup_acl_test();

    let mut ctx = admin_ctx("POST", None);
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::set_default_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn set_default_acl_for_username_and_entity_bad_request() {
    let _store = setup_acl_test();

    let mut ctx = admin_ctx("POST", None);
    set_params(&mut ctx, Some("doc"), "ghost");

    http_acl::set_default_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn set_default_acl_for_username_and_entity_forbidden() {
    let _store = setup_acl_test();

    let mut ctx = user_ctx("POST");
    set_params(&mut ctx, Some("doc"), "admin");

    http_acl::set_default_acl_for_username_and_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}
