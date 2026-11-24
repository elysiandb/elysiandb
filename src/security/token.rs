use crate::crypto::subtle;
use crate::httpd::{HEADER_AUTHORIZATION, RequestCtx};
use crate::sync::RwLock;

use super::basic_user::Check;
use super::config;

const BEARER_PREFIX: &[u8] = b"Bearer ";

static CHECK_TOKEN_AUTHENTICATION: RwLock<Option<Check>> = RwLock::new(None);

pub fn check_token_authentication(ctx: &RequestCtx) -> bool {
    let replaced = CHECK_TOKEN_AUTHENTICATION.read().clone();
    match replaced {
        Some(f) => f(ctx),
        None => default_check_token_authentication(ctx),
    }
}

pub fn set_check_token_authentication(f: Option<Check>) -> Option<Check> {
    std::mem::replace(&mut *CHECK_TOKEN_AUTHENTICATION.write(), f)
}

fn default_check_token_authentication(ctx: &RequestCtx) -> bool {
    let provided = ctx.request.header.peek(HEADER_AUTHORIZATION);
    if provided.is_empty() {
        return false;
    }

    if provided.len() <= BEARER_PREFIX.len() || &provided[..BEARER_PREFIX.len()] != BEARER_PREFIX {
        return false;
    }

    token_matches(&provided[BEARER_PREFIX.len()..])
}

pub fn token_matches(provided: &[u8]) -> bool {
    let cfg = config();
    let expected = cfg.security.authentication.token.as_bytes();
    if expected.is_empty() {
        return false;
    }

    subtle::constant_time_compare(provided, expected) == 1
}
