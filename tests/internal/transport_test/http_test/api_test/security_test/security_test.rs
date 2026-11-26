use std::io::Read;
use std::os::fd::FromRawFd;
use std::sync::Arc;
use std::time::Duration;

use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::Value;
use elysiandb::httpd::{
    Request, RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NOT_FOUND, STATUS_OK,
};
use elysiandb::security::{self, BasicUser, Role, Session};
use elysiandb::storage;
use elysiandb::transport::http::api::security as http_security;

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
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "user".to_string();
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::load_json_db();

    let _ = security::init_basic_users_storage();
    let _ = security::init_admin_user_if_not_exists();

    Store { _dir: dir }
}

fn new_ctx(method: &str, uri: &str, body: &str, session: Option<&Session>) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(uri);
    req.header.set_method(method);
    if !body.is_empty() {
        req.set_body_string(body);
    }
    if let Some(session) = session {
        req.header
            .set_cookie(security::SESSION_COOKIE_NAME, &session.id);
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
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

fn login(username: &str, role: Role) -> Session {
    ensure_user(username, role.clone());
    security::create_session(username.as_bytes(), role, Duration::from_secs(60))
        .unwrap_or_else(|e| panic!("create session: {e}"))
}

fn create_user(username: &str, password: &str, role: Role) {
    let _ = security::create_basic_user(&BasicUser {
        username: username.as_bytes().to_vec(),
        password: password.as_bytes().to_vec(),
        role,
    });
}

fn admin() -> &'static str {
    security::DEFAULT_ADMIN_USERNAME
}

fn capture_stdout(f: impl FnOnce()) -> Vec<u8> {
    let mut fds = [0; 2];
    assert_eq!(unsafe { libc::pipe(fds.as_mut_ptr()) }, 0, "pipe");
    let saved = unsafe { libc::dup(1) };
    assert!(saved >= 0, "dup");
    unsafe { libc::dup2(fds[1], 1) };
    unsafe { libc::close(fds[1]) };

    f();

    unsafe { libc::dup2(saved, 1) };
    unsafe { libc::close(saved) };

    let mut printed = Vec::new();
    let mut reader = unsafe { std::fs::File::from_raw_fd(fds[0]) };
    let _ = reader.read_to_end(&mut printed);
    printed
}

#[test]
fn get_users_controller_admin_ok() {
    let _store = setup();

    create_user("u1", "p1", Role::user());

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx("GET", "/api/security/user", "", Some(&s));

    http_security::get_users_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn get_users_controller_does_not_print_the_users() {
    let _store = setup();

    create_user("u1", "p1", Role::user());

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx("GET", "/api/security/user", "", Some(&s));

    let printed = capture_stdout(|| http_security::get_users_controller(&mut ctx));

    assert!(
        printed.is_empty(),
        "expected nothing on stdout, got {:?}",
        String::from_utf8_lossy(&printed)
    );

    let body = String::from_utf8_lossy(ctx.response.body()).into_owned();
    assert!(
        body.contains(r#""username":"u1""#),
        "expected the users in the response, got {body}"
    );
}

#[test]
fn get_users_controller_forbidden() {
    let _store = setup();

    let s = login("user", Role::user());
    let mut ctx = new_ctx("GET", "/api/security/user", "", Some(&s));

    http_security::get_users_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn get_user_by_username_controller_admin_ok() {
    let _store = setup();

    create_user("bob", "x", Role::user());

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx("GET", "/api/security/user/bob", "", Some(&s));
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::get_user_by_username_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");
}

#[test]
fn get_user_by_username_controller_forbidden_self() {
    let _store = setup();

    create_user("bob", "x", Role::user());

    let s = login("bob", Role::user());
    let mut ctx = new_ctx("GET", "/api/security/user/bob", "", Some(&s));
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::get_user_by_username_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn create_user_controller_admin_ok() {
    let _store = setup();

    let s = login(admin(), Role::admin());

    let body = r#"{"username":"alice","password":"alice-pass","role":"user"}"#;
    let mut ctx = new_ctx("POST", "/api/security/user", body, Some(&s));

    http_security::create_user_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    assert!(
        security::get_basic_user_by_username(b"alice").is_ok(),
        "user not created"
    );
}

#[test]
fn create_user_controller_forbidden() {
    let _store = setup();

    let s = login("user", Role::user());

    let body = r#"{"username":"alice","password":"alice-pass","role":"user"}"#;
    let mut ctx = new_ctx("POST", "/api/security/user", body, Some(&s));

    http_security::create_user_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn create_user_controller_invalid_json() {
    let _store = setup();

    let s = login(admin(), Role::admin());

    let mut ctx = new_ctx("POST", "/api/security/user", "{bad", Some(&s));

    http_security::create_user_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn delete_user_by_username_controller_ok() {
    let _store = setup();

    create_user("todelete", "x", Role::user());

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx("DELETE", "/api/security/user/todelete", "", Some(&s));
    ctx.set_user_value("user_name", "todelete".to_string());

    http_security::delete_user_by_username_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    assert!(
        security::get_basic_user_by_username(b"todelete").is_err(),
        "user should be deleted"
    );
}

#[test]
fn delete_user_by_username_controller_forbidden_self() {
    let _store = setup();

    let s = login("bob", Role::user());
    let mut ctx = new_ctx("DELETE", "/api/security/user/bob", "", Some(&s));
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::delete_user_by_username_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn change_user_password_controller_ok() {
    let _store = setup();

    create_user("john", "old", Role::user());

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx(
        "PUT",
        "/api/security/user/john/password",
        r#"{"password":"new-password"}"#,
        Some(&s),
    );
    ctx.set_user_value("user_name", "john".to_string());

    http_security::change_user_password_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    assert!(
        security::authenticate_user(b"john", b"new-password").is_some(),
        "password not changed"
    );
}

#[test]
fn change_user_password_controller_invalid_json() {
    let _store = setup();

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx("PUT", "/api/security/user/john/password", "{bad", Some(&s));
    ctx.set_user_value("user_name", "john".to_string());

    http_security::change_user_password_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn change_user_password_controller_forbidden_self() {
    let _store = setup();

    let s = login("bob", Role::user());
    let mut ctx = new_ctx(
        "PUT",
        "/api/security/user/bob/password",
        r#"{"password":"ghost-password"}"#,
        Some(&s),
    );
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::change_user_password_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn change_user_password_controller_user_not_found() {
    let _store = setup();

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx(
        "PUT",
        "/api/security/user/ghost/password",
        r#"{"password":"ghost-password"}"#,
        Some(&s),
    );
    ctx.set_user_value("user_name", "ghost".to_string());

    http_security::change_user_password_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_NOT_FOUND, "expected 404");
}

#[test]
fn change_user_role_controller_ok() {
    let _store = setup();

    create_user("bob", "x", Role::user());

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx(
        "PUT",
        "/api/security/user/bob/role",
        r#"{"role":"admin"}"#,
        Some(&s),
    );
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::change_user_role_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_OK, "expected 200");

    let u = security::get_basic_user_by_username(b"bob").expect("bob");

    assert_eq!(
        u.get("role").and_then(Value::as_str),
        Some(security::ROLE_ADMIN),
        "role not updated"
    );
}

#[test]
fn change_user_role_controller_forbidden() {
    let _store = setup();

    create_user("bob", "x", Role::user());

    let s = login("bob", Role::user());
    let mut ctx = new_ctx(
        "PUT",
        "/api/security/user/bob/role",
        r#"{"role":"admin"}"#,
        Some(&s),
    );
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::change_user_role_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_FORBIDDEN, "expected 403");
}

#[test]
fn change_user_role_controller_invalid_json() {
    let _store = setup();

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx("PUT", "/api/security/user/bob/role", "{bad", Some(&s));
    ctx.set_user_value("user_name", "bob".to_string());

    http_security::change_user_role_controller(&mut ctx);

    assert_eq!(
        ctx.response.status_code(),
        STATUS_BAD_REQUEST,
        "expected 400"
    );
}

#[test]
fn change_user_role_controller_user_not_found() {
    let _store = setup();

    let s = login(admin(), Role::admin());
    let mut ctx = new_ctx(
        "PUT",
        "/api/security/user/ghost/role",
        r#"{"role":"admin"}"#,
        Some(&s),
    );
    ctx.set_user_value("user_name", "ghost".to_string());

    http_security::change_user_role_controller(&mut ctx);

    assert_eq!(ctx.response.status_code(), STATUS_NOT_FOUND, "expected 404");
}
