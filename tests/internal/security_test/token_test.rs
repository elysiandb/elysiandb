use elysiandb::httpd::RequestCtx;
use elysiandb::security;

use crate::update_config;

fn set_token(token: &str) {
    update_config(|cfg| cfg.security.authentication.token = token.to_string());
}

#[test]
fn check_token_authentication_valid_token() {
    set_token("secret123");

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Authorization", "Bearer secret123");

    assert!(
        security::check_token_authentication(&ctx),
        "expected authentication to succeed"
    );
}

#[test]
fn check_token_authentication_invalid_token() {
    set_token("secret123");

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Authorization", "Bearer wrong");

    assert!(
        !security::check_token_authentication(&ctx),
        "expected authentication to fail with invalid token"
    );
}

#[test]
fn check_token_authentication_missing_header() {
    set_token("secret123");

    let ctx = RequestCtx::new();

    assert!(
        !security::check_token_authentication(&ctx),
        "expected authentication to fail without header"
    );
}

#[test]
fn check_token_authentication_no_bearer_prefix() {
    set_token("secret123");

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Authorization", "Token secret123");

    assert!(
        !security::check_token_authentication(&ctx),
        "expected authentication to fail without Bearer prefix"
    );
}

#[test]
fn check_token_authentication_empty_token() {
    set_token("");

    let mut ctx = RequestCtx::new();
    ctx.request.header.set("Authorization", "Bearer ");

    assert!(
        !security::check_token_authentication(&ctx),
        "expected authentication to fail when expected token is empty"
    );
}
