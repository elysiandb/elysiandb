use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use elysiandb::configuration::Config;
use elysiandb::httpd::{Handler, RequestCtx, STATUS_FORBIDDEN, STATUS_UNAUTHORIZED};
use elysiandb::security::{self, Role};

use crate::{replace_basic_check, replace_user_auth, set_config};

fn flag_handler(flag: &Arc<AtomicBool>) -> Handler {
    let flag = flag.clone();
    Arc::new(move |_ctx: &mut RequestCtx| flag.store(true, Ordering::SeqCst))
}

fn auth_config(enabled: bool, mode: &str, token: &str) -> Config {
    let mut cfg = Config::default();
    cfg.security.authentication.enabled = enabled;
    cfg.security.authentication.mode = mode.to_string();
    cfg.security.authentication.token = token.to_string();
    cfg
}

#[test]
fn authenticate_no_authentication() {
    set_config(auth_config(false, "", ""));

    let called = Arc::new(AtomicBool::new(false));
    let handler = security::authenticate(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    handler(&mut ctx);

    assert!(
        called.load(Ordering::SeqCst),
        "handler should be called when authentication disabled"
    );
}

#[test]
fn authenticate_basic_auth_success() {
    set_config(auth_config(true, "basic", ""));

    let mut ctx = RequestCtx::new();
    let called = Arc::new(AtomicBool::new(false));

    let _restore = replace_basic_check(Arc::new(|_| true));

    let handler = security::authenticate(flag_handler(&called));

    handler(&mut ctx);

    assert!(
        called.load(Ordering::SeqCst),
        "handler should be called when basic auth succeeds"
    );
}

#[test]
fn authenticate_basic_auth_fail() {
    set_config(auth_config(true, "basic", ""));

    let mut ctx = RequestCtx::new();
    let called = Arc::new(AtomicBool::new(false));

    let _restore = replace_basic_check(Arc::new(|_| false));

    let handler = security::authenticate(flag_handler(&called));

    handler(&mut ctx);

    assert!(
        !called.load(Ordering::SeqCst),
        "handler should not be called when basic auth fails"
    );
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401 unauthorized"
    );
}

#[test]
fn authenticate_token_auth_success() {
    set_config(auth_config(true, "token", "secret123"));

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Authorization", "Bearer secret123");
    let called = Arc::new(AtomicBool::new(false));

    let handler = security::authenticate(flag_handler(&called));

    handler(&mut ctx);

    assert!(
        called.load(Ordering::SeqCst),
        "handler should be called when token auth succeeds"
    );
}

#[test]
fn authenticate_token_auth_fail() {
    set_config(auth_config(true, "token", "secret123"));

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Authorization", "Bearer wrong");
    let called = Arc::new(AtomicBool::new(false));

    let handler = security::authenticate(flag_handler(&called));

    handler(&mut ctx);

    assert!(
        !called.load(Ordering::SeqCst),
        "handler should not be called when token auth fails"
    );
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401 unauthorized"
    );
}

#[test]
fn authenticate_user_auth_success() {
    set_config(auth_config(true, "user", ""));

    let called = Arc::new(AtomicBool::new(false));
    let called_user_auth = Arc::new(AtomicBool::new(false));

    let seen = called_user_auth.clone();
    let _restore = replace_user_auth(Arc::new(move |next: Handler| {
        let seen = seen.clone();
        Arc::new(move |ctx: &mut RequestCtx| {
            seen.store(true, Ordering::SeqCst);
            next(ctx);
        }) as Handler
    }));

    let handler = security::authenticate(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    handler(&mut ctx);

    assert!(
        called_user_auth.load(Ordering::SeqCst),
        "user auth wrapper should be invoked"
    );
    assert!(
        called.load(Ordering::SeqCst),
        "final handler should be called when user auth enabled"
    );
}

#[test]
fn authenticate_user_auth_fail() {
    set_config(auth_config(true, "user", ""));

    let called = Arc::new(AtomicBool::new(false));

    let _restore = replace_user_auth(Arc::new(|_next: Handler| {
        Arc::new(|ctx: &mut RequestCtx| {
            ctx.response.set_status_code(STATUS_UNAUTHORIZED);
        }) as Handler
    }));

    let handler = security::authenticate(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    handler(&mut ctx);

    assert!(
        !called.load(Ordering::SeqCst),
        "handler should not be called when user auth fails"
    );
    assert_eq!(
        ctx.response.status_code(),
        STATUS_UNAUTHORIZED,
        "expected 401 status when user auth fails"
    );
}

#[test]
fn authentication_is_enabled() {
    set_config(auth_config(true, "", ""));

    assert!(
        security::authentication_is_enabled(),
        "expected authentication enabled"
    );

    set_config(auth_config(false, "", ""));

    assert!(
        !security::authentication_is_enabled(),
        "expected authentication disabled"
    );
}

#[test]
fn basic_authentication_is_enabled() {
    set_config(auth_config(true, "basic", ""));

    assert!(
        security::basic_authentication_is_enabled(),
        "basic auth should be enabled"
    );

    set_config(auth_config(true, "token", ""));

    assert!(
        !security::basic_authentication_is_enabled(),
        "basic auth should be disabled"
    );
}

#[test]
fn token_authentication_is_enabled() {
    set_config(auth_config(true, "token", ""));

    assert!(
        security::token_authentication_is_enabled(),
        "token auth should be enabled"
    );

    set_config(auth_config(true, "basic", ""));

    assert!(
        !security::token_authentication_is_enabled(),
        "token auth should be disabled"
    );
}

#[test]
fn user_authentication_is_enabled() {
    set_config(auth_config(true, "user", ""));

    assert!(
        security::user_authentication_is_enabled(),
        "user auth should be enabled"
    );

    set_config(auth_config(true, "basic", ""));

    assert!(
        !security::user_authentication_is_enabled(),
        "user auth should be disabled"
    );
}

#[test]
fn authenticate_admin_user_mode() {
    set_config(auth_config(true, "user", ""));

    let role = Arc::new(elysiandb::sync::Mutex::new(Role::user()));
    let current = role.clone();
    let _restore = replace_user_auth(Arc::new(move |next: Handler| {
        let current = current.clone();
        Arc::new(move |ctx: &mut RequestCtx| {
            let role = current.lock().clone();
            ctx.set_user_value("role", role);
            next(ctx);
        }) as Handler
    }));

    let called = Arc::new(AtomicBool::new(false));
    let handler = security::authenticate_admin(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    handler(&mut ctx);

    assert!(
        !called.load(Ordering::SeqCst) && ctx.response.status_code() == STATUS_FORBIDDEN,
        "expected 403 for a regular user, got {}",
        ctx.response.status_code()
    );

    *role.lock() = Role::admin();
    let mut ctx = RequestCtx::new();
    handler(&mut ctx);

    assert!(
        called.load(Ordering::SeqCst),
        "handler should be called for an admin"
    );
}

#[test]
fn authenticate_admin_other_modes() {
    let _restore = replace_basic_check(Arc::new(|_| true));

    for mode in ["", "basic", "token"] {
        set_config(auth_config(!mode.is_empty(), mode, "secret123"));

        let called = Arc::new(AtomicBool::new(false));
        let handler = security::authenticate_admin(flag_handler(&called));

        let mut ctx = RequestCtx::new();
        ctx.request.header.set("Authorization", "Bearer secret123");
        handler(&mut ctx);

        assert!(
            called.load(Ordering::SeqCst),
            "handler should be called in mode {mode:?}"
        );
    }
}
