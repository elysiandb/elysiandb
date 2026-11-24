use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use elysiandb::configuration::Config;
use elysiandb::httpd::{Handler, RequestCtx};
use elysiandb::security;

use crate::{set_config, string_list};

fn set_allowed_origins(origins: &[&str]) {
    let mut cfg = Config::default();
    cfg.server.http.cors.allowed_origins = string_list(origins);
    set_config(cfg);
}

fn flag_handler(flag: &Arc<AtomicBool>) -> Handler {
    let flag = flag.clone();
    Arc::new(move |_ctx: &mut RequestCtx| flag.store(true, Ordering::SeqCst))
}

fn noop() -> Handler {
    Arc::new(|_ctx: &mut RequestCtx| {})
}

fn header<'a>(ctx: &'a RequestCtx, name: &str) -> &'a [u8] {
    ctx.response.header.peek(name)
}

#[test]
fn cors_with_origin() {
    set_allowed_origins(&["http://example.com/"]);
    let called = Arc::new(AtomicBool::new(false));

    let h = security::cors(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Origin", "http://example.com");
    ctx.request.header.set_method("GET");

    h(&mut ctx);

    assert!(called.load(Ordering::SeqCst), "next handler not called");

    assert_eq!(
        header(&ctx, "Access-Control-Allow-Origin"),
        b"http://example.com",
        "allow origin header not set"
    );
    assert_eq!(
        header(&ctx, "Access-Control-Allow-Credentials"),
        b"true",
        "credentials header not set"
    );
    assert!(
        !header(&ctx, "Access-Control-Allow-Methods").is_empty(),
        "methods header missing"
    );
}

#[test]
fn cors_origin_not_allowed_by_default() {
    set_allowed_origins(&[]);
    let called = Arc::new(AtomicBool::new(false));
    let h = security::cors(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Origin", "https://evil.example");
    ctx.request.header.set_method("GET");

    h(&mut ctx);

    assert!(called.load(Ordering::SeqCst), "next handler not called");

    for name in [
        "Access-Control-Allow-Origin",
        "Access-Control-Allow-Credentials",
        "Access-Control-Allow-Methods",
    ] {
        assert!(
            ctx.response.header.peek_all(name).is_empty(),
            "{name} should not be set for an origin outside the allow-list, got {:?}",
            header(&ctx, name)
        );
    }
}

#[test]
fn cors_origin_not_in_allow_list() {
    set_allowed_origins(&["https://app.example"]);
    let h = security::cors(noop());

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Origin", "https://evil.example");
    ctx.request.header.set_method("OPTIONS");

    h(&mut ctx);

    assert!(
        ctx.response
            .header
            .peek_all("Access-Control-Allow-Origin")
            .is_empty(),
        "origin outside the allow-list should not be reflected, got {:?}",
        header(&ctx, "Access-Control-Allow-Origin")
    );
}

#[test]
fn cors_wildcard_allows_any_origin() {
    set_allowed_origins(&["*"]);
    let h = security::cors(noop());

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Origin", "https://any.example");
    ctx.request.header.set_method("GET");

    h(&mut ctx);

    assert_eq!(
        header(&ctx, "Access-Control-Allow-Origin"),
        b"https://any.example",
        "expected the origin to be reflected with '*'"
    );
}

#[test]
fn cors_no_origin() {
    set_allowed_origins(&["http://example.com"]);
    let called = Arc::new(AtomicBool::new(false));

    let h = security::cors(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    ctx.request.header.set_method("GET");

    h(&mut ctx);

    assert!(called.load(Ordering::SeqCst), "next handler not called");

    assert!(
        ctx.response
            .header
            .peek_all("Access-Control-Allow-Origin")
            .is_empty(),
        "origin header should not be set"
    );
}

#[test]
fn cors_options() {
    set_allowed_origins(&["http://example.com"]);
    let called = Arc::new(AtomicBool::new(false));

    let h = security::cors(flag_handler(&called));

    let mut ctx = RequestCtx::new();
    ctx.request.header.set_method("OPTIONS");
    ctx.request.header.set("Origin", "http://example.com");

    h(&mut ctx);

    assert!(
        !called.load(Ordering::SeqCst),
        "next handler should not be called on OPTIONS"
    );

    assert_eq!(
        ctx.response.status_code(),
        204,
        "unexpected status code: {}",
        ctx.response.status_code()
    );
}
