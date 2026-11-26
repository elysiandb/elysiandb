use std::sync::Arc;
use std::time::Duration;

use elysiandb::api;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::hook::{self, Hook};
use elysiandb::httpd::{
    Request, RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK,
};
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};
use elysiandb::transport::http::api::hook::{self as http_hook, HookRecord};

use crate::tempdir::TempDir;

const HOUR: Duration = Duration::from_secs(3600);

static SERIAL: Mutex<()> = Mutex::new(());

struct Store {
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
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

fn setup_hook_test() -> Store {
    let serial = SERIAL.lock();
    let _ = storage::close();
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = true;
    cfg.api.hooks.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::load_json_db();
    api::delete_all();

    hook::init_hooks();

    api::write_entity(
        b"_elysiandb_core_user",
        &mut object(vec![
            ("id", s("admin")),
            ("username", s("admin")),
            ("password", s("admin-password-hash")),
            ("role", s("admin")),
        ]),
    );

    Store {
        _dir: dir,
        _serial: serial,
    }
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

fn doc_hook() -> Hook {
    Hook {
        entity: "doc".to_string(),
        name: "h1".to_string(),
        event: hook::HOOK_EVENT_POST_READ.to_string(),
        language: "javascript".to_string(),
        enabled: true,
        ..Hook::default()
    }
}

fn first_id() -> String {
    hook::get_hooks_for_entity("doc").expect("hooks")[0]
        .id
        .clone()
}

#[test]
fn get_hooks_for_entity_ok() {
    let _s = setup_hook_test();

    let _ = hook::create_hook(&mut doc_hook());

    let mut ctx = admin_ctx("GET", None);
    ctx.set_user_value("entity", "doc".to_string());

    http_hook::get_hooks_for_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    let mut out: Vec<HookRecord> = Vec::new();
    let _ = json::unmarshal(ctx.response.body(), &mut out);

    assert_eq!(out.len(), 1, "expected 1 hook");
}

#[test]
fn get_hooks_for_entity_forbidden() {
    let _s = setup_hook_test();

    let mut ctx = user_ctx("GET");
    ctx.set_user_value("entity", "doc".to_string());

    http_hook::get_hooks_for_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn create_hook_for_entity_ok() {
    let _s = setup_hook_test();

    let body = json::marshal(&object(vec![
        ("name", s("h1")),
        ("event", s(hook::HOOK_EVENT_POST_READ)),
        ("language", s("javascript")),
        ("enabled", Value::Bool(true)),
    ]))
    .expect("body");

    let mut ctx = admin_ctx("POST", Some(&body));
    ctx.set_user_value("entity", "doc".to_string());

    http_hook::create_hook_for_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    let mut created = HookRecord::default();
    if let Err(err) = json::unmarshal(ctx.response.body(), &mut created) {
        panic!("invalid response body: {err}");
    }

    assert!(!created.id.is_empty(), "expected non-empty id in response");

    let list = hook::get_hooks_for_entity("doc").expect("hooks");
    assert_eq!(list.len(), 1, "expected 1 stored hook, got {}", list.len());

    assert_eq!(
        list[0].id, created.id,
        "expected response id to match stored id"
    );

    assert_eq!(
        created.script, list[0].script,
        "expected response script to match stored script"
    );
}

#[test]
fn create_hook_for_entity_invalid_json() {
    let _s = setup_hook_test();

    let mut ctx = admin_ctx("POST", Some(b"{bad"));
    ctx.set_user_value("entity", "doc".to_string());

    http_hook::create_hook_for_entity_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn create_hook_for_entity_forbidden() {
    let _s = setup_hook_test();

    let mut ctx = user_ctx("POST");
    ctx.set_user_value("entity", "doc".to_string());

    http_hook::create_hook_for_entity_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn get_hook_by_id_ok() {
    let _s = setup_hook_test();

    let _ = hook::create_hook(&mut doc_hook());

    let id = first_id();

    let mut ctx = admin_ctx("GET", None);
    ctx.set_user_value("id", id);

    http_hook::get_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn get_hook_by_id_not_found() {
    let _s = setup_hook_test();

    let mut ctx = admin_ctx("GET", None);
    ctx.set_user_value("id", "missing".to_string());

    http_hook::get_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_NOT_FOUND, "expected 404");
}

#[test]
fn get_hook_by_id_forbidden() {
    let _s = setup_hook_test();

    let mut ctx = user_ctx("GET");
    ctx.set_user_value("id", "x".to_string());

    http_hook::get_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn update_hook_by_id_ok() {
    let _s = setup_hook_test();

    let _ = hook::create_hook(&mut doc_hook());

    let id = first_id();

    let body = json::marshal(&object(vec![("name", s("updated"))])).expect("body");

    let mut ctx = admin_ctx("POST", Some(&body));
    ctx.set_user_value("id", id);

    http_hook::update_hook_by_id_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_OK,
        "expected 200, got {} {}",
        ctx.response.status_code(),
        String::from_utf8_lossy(ctx.response.body())
    );
}

#[test]
fn update_hook_by_id_invalid_json() {
    let _s = setup_hook_test();

    let mut ctx = admin_ctx("POST", Some(b"{bad"));
    ctx.set_user_value("id", "x".to_string());

    http_hook::update_hook_by_id_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn update_hook_by_id_forbidden() {
    let _s = setup_hook_test();

    let mut ctx = user_ctx("POST");
    ctx.set_user_value("id", "x".to_string());

    http_hook::update_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn delete_hook_by_id_ok() {
    let _s = setup_hook_test();

    let _ = hook::create_hook(&mut doc_hook());

    let id = first_id();

    let mut ctx = admin_ctx("DELETE", None);
    ctx.set_user_value("id", id);

    http_hook::delete_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn delete_hook_by_id_not_found() {
    let _s = setup_hook_test();

    let mut ctx = admin_ctx("DELETE", None);
    ctx.set_user_value("id", "missing".to_string());

    http_hook::delete_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_NOT_FOUND, "expected 404");
}

#[test]
fn delete_hook_by_id_forbidden() {
    let _s = setup_hook_test();

    let mut ctx = user_ctx("DELETE");
    ctx.set_user_value("id", "x".to_string());

    http_hook::delete_hook_by_id_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}
