use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::httpd::{
    Handler, Request, RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NO_CONTENT,
    STATUS_OK, STATUS_UNAUTHORIZED,
};
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;
use elysiandb::transport::http::adminui;

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

fn setup() -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "user".to_string();
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();

    Store { _dir: dir }
}

fn setup_token_mode() -> Store {
    let store = setup();
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "token".to_string();
    cfg.security.authentication.token = "secret-token".to_string();
    globals::set_config(Some(Arc::new(cfg)));
    store
}

fn new_ctx(method: &str, path: &str, body: &str) -> RequestCtx {
    let mut req = Request::new();
    req.header.set_method(method);
    req.set_request_uri(path);
    if !body.is_empty() {
        req.set_body(body.as_bytes());
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn create_user(username: &str, password: &str, role: Role) {
    let _ = security::create_basic_user(&BasicUser {
        username: username.as_bytes().to_vec(),
        password: password.as_bytes().to_vec(),
        role,
    });
}

fn ensure_user(username: &str, role: Role) {
    if !security::basic_user_exists(username.as_bytes()) {
        create_user(username, &format!("{username}-password"), role);
    }
}

fn session_id(username: &str, role: Role) -> String {
    security::create_session(username.as_bytes(), role, HOUR)
        .map(|s| s.id)
        .unwrap_or_default()
}

fn guarded(called: &Arc<AtomicBool>) -> Handler {
    let flag = called.clone();
    adminui::admin_auth(Arc::new(move |_ctx: &mut RequestCtx| {
        flag.store(true, Ordering::SeqCst)
    }))
}

fn set_cookie(ctx: &mut RequestCtx, id: &str) {
    ctx.request
        .header
        .set_cookie(security::SESSION_COOKIE_NAME, id);
}

#[test]
fn admin_auth_no_cookie() {
    let _store = setup();
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/admin", "");
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
    assert!(!called.load(Ordering::SeqCst), "should not call next");
}

#[test]
fn admin_auth_invalid_session() {
    let _store = setup();
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/admin", "");
    set_cookie(&mut ctx, "bad");
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
    assert!(!called.load(Ordering::SeqCst), "should not call next");
}

#[test]
fn admin_auth_forbidden_for_non_admin() {
    let _store = setup();
    create_user("u", "x", Role::user());
    let id = session_id("u", Role::user());
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/admin", "");
    set_cookie(&mut ctx, &id);
    h(&mut ctx);
    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
    assert!(!called.load(Ordering::SeqCst), "should not call next");
}

#[test]
fn admin_auth_ok() {
    let _store = setup();
    create_user("a", "x", Role::admin());
    let id = session_id("a", Role::admin());
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/admin", "");
    set_cookie(&mut ctx, &id);
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_OK,
        "unexpected status {}",
        ctx.response.status_code()
    );
    assert!(called.load(Ordering::SeqCst), "expected next to be called");
}

#[test]
fn admin_auth_token_mode_no_token() {
    let _store = setup_token_mode();
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/api/hook/book", "");
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
    assert!(!called.load(Ordering::SeqCst), "should not call next");
}

#[test]
fn admin_auth_token_mode_invalid_token() {
    let _store = setup_token_mode();
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/api/hook/book", "");
    ctx.request
        .header
        .set("Authorization", "Bearer wrong-token");
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
    assert!(!called.load(Ordering::SeqCst), "should not call next");
}

#[test]
fn admin_auth_token_mode_session_cookie_is_not_enough() {
    let _store = setup_token_mode();
    create_user("a", "x", Role::admin());
    let id = session_id("a", Role::admin());
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/api/hook/book", "");
    set_cookie(&mut ctx, &id);
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
    assert!(!called.load(Ordering::SeqCst), "should not call next");
}

#[test]
fn admin_auth_token_mode_ok() {
    let _store = setup_token_mode();
    let called = Arc::new(AtomicBool::new(false));
    let h = guarded(&called);
    let mut ctx = new_ctx("GET", "/api/hook/book", "");
    ctx.request
        .header
        .set("Authorization", "Bearer secret-token");
    h(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_OK,
        "unexpected status {}",
        ctx.response.status_code()
    );
    assert!(called.load(Ordering::SeqCst), "expected next to be called");
}

#[test]
fn login_controller_invalid_json() {
    let _store = setup();
    let mut ctx = new_ctx("POST", "/login", "{invalid");
    adminui::login_controller(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn login_controller_invalid_credentials() {
    let _store = setup();
    let mut ctx = new_ctx("POST", "/login", r#"{"username":"x","password":"y"}"#);
    adminui::login_controller(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
}

#[test]
fn login_controller_ok() {
    let _store = setup();
    create_user("admin", "x", Role::admin());
    let mut ctx = new_ctx("POST", "/login", r#"{"username":"admin","password":"x"}"#);
    adminui::login_controller(&mut ctx);
    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
    assert!(
        !ctx.response
            .header
            .peek_cookie(security::SESSION_COOKIE_NAME)
            .is_empty(),
        "cookie not set"
    );
}

#[test]
fn logout_controller() {
    let _store = setup();

    ensure_user("a", Role::admin());
    let id = session_id("a", Role::admin());

    let mut ctx = new_ctx("POST", "/logout", "");
    set_cookie(&mut ctx, &id);

    adminui::logout_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_NO_CONTENT,
        "expected 204"
    );

    let sc = String::from_utf8_lossy(ctx.response.header.peek("Set-Cookie")).into_owned();
    assert!(!sc.is_empty(), "Set-Cookie missing");

    assert!(
        sc.to_lowercase().contains("expires="),
        "expected expires= in Set-Cookie, got {sc}"
    );

    assert!(
        sc.contains(&format!("{}=", security::SESSION_COOKIE_NAME)),
        "expected cookie key, got {sc}"
    );
}

#[test]
fn me_controller_no_cookie() {
    let _store = setup();
    let mut ctx = new_ctx("GET", "/me", "");
    adminui::me_controller(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
}

#[test]
fn me_controller_invalid_session() {
    let _store = setup();
    let mut ctx = new_ctx("GET", "/me", "");
    set_cookie(&mut ctx, "no");
    adminui::me_controller(&mut ctx);
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401"
    );
}

#[test]
fn me_controller_ok() {
    let _store = setup();
    create_user("zzz", "x", Role::admin());
    let id = session_id("zzz", Role::admin());
    let mut ctx = new_ctx("GET", "/me", "");
    set_cookie(&mut ctx, &id);
    adminui::me_controller(&mut ctx);
    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
    let mut m = Map::new();
    let _ = json::unmarshal(ctx.response.body(), &mut m);
    assert_eq!(
        m.get("username").and_then(Value::as_str),
        Some("zzz"),
        "wrong username"
    );
    assert_eq!(
        m.get("role").and_then(Value::as_str),
        Some(security::ROLE_ADMIN),
        "wrong role"
    );
}

#[test]
fn login_controller_secure_cookie_over_tls() {
    let _store = setup();
    create_user("admin", "pass", Role::admin());

    let mut ctx = new_ctx(
        "POST",
        "/api/security/login",
        r#"{"username":"admin","password":"pass"}"#,
    );
    ctx.request.set_tls(true);

    adminui::login_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_OK,
        "expected 200, got {}",
        ctx.response.status_code()
    );

    let cookie = String::from_utf8_lossy(
        ctx.response
            .header
            .peek_cookie(security::SESSION_COOKIE_NAME),
    )
    .to_lowercase();
    assert!(
        cookie.contains("secure"),
        "expected a Secure cookie over TLS, got {cookie:?}"
    );
}

#[test]
fn is_built_matches_embedded_assets() {
    let embedded = elysiandb::adminui::files()
        .iter()
        .any(|(name, _)| *name == "index.html");
    assert_eq!(
        adminui::is_built(),
        embedded,
        "IsBuilt must report whether dist/index.html is embedded"
    );
}

fn admin_get(path: &str) -> (u16, Option<String>, Vec<u8>) {
    let mut ctx = new_ctx("GET", path, "");
    adminui::admin_ui_handler(&mut ctx);

    let mut wire = Vec::new();
    if let Err(e) = ctx.response.write(&mut wire) {
        panic!("{path}: {e}");
    }
    let text = String::from_utf8_lossy(&wire).into_owned();
    let head = text.split("\r\n\r\n").next().unwrap_or("").to_string();
    let content_type = head
        .split("\r\n")
        .find_map(|l| l.strip_prefix("Content-Type: "))
        .map(str::to_string);

    (
        ctx.response.status_code(),
        content_type,
        ctx.response.body_bytes().to_vec(),
    )
}

fn expected_content_type(name: &str, data: &[u8]) -> Option<&'static str> {
    let base = name.rsplit('/').next().unwrap_or(name);
    let ext = base.rfind('.').map(|i| &base[i..]).unwrap_or("");
    match ext {
        ".js" => Some("text/javascript"),
        ".css" => Some("text/css"),
        ".html" => Some("text/html"),
        ".svg" => Some("image/svg+xml"),
        ".png" => Some("image/png"),
        ".ico" => Some("image/x-icon"),
        _ if data.is_empty() => None,
        _ => Some("text/plain; charset=utf-8"),
    }
}

#[test]
fn admin_ui_handler_serves_the_embedded_assets() {
    for (name, data) in elysiandb::adminui::files() {
        assert!(
            name.split('/')
                .skip(1)
                .all(|part| !part.starts_with('.') && !part.starts_with('_')),
            "{name} must not be embedded"
        );

        let (status, content_type, body) = admin_get(&format!("/admin/{name}"));
        assert_eq!(status, STATUS_OK, "/admin/{name}");
        assert_eq!(body.as_slice(), *data, "/admin/{name}");
        assert_eq!(
            content_type.as_deref(),
            expected_content_type(name, data),
            "/admin/{name}"
        );
    }

    let index = elysiandb::adminui::read_file(b"dist/index.html");
    for path in [
        "/admin",
        "/admin/",
        "/admin/nope/deep",
        "/admin/assets",
        "/admin/../admin/x",
    ] {
        let (status, content_type, body) = admin_get(path);
        match index {
            Some(index) => {
                assert_eq!(status, STATUS_OK, "{path}");
                assert_eq!(content_type.as_deref(), Some("text/html"), "{path}");
                assert_eq!(body.as_slice(), index, "{path}");
            }
            None => {
                assert_eq!(status, 404, "{path}");
                assert_eq!(content_type, None, "{path}");
                assert!(body.is_empty(), "{path}");
            }
        }
    }
}
