use crate::httpd::{HEADER_AUTHORIZATION, RequestCtx};

use super::authentication::{
    authentication_is_enabled, basic_authentication_is_enabled, token_authentication_is_enabled,
};
use super::basic_user::{BasicHeader, parse_basic_header};
use super::session::get_current_username;

pub fn actor(ctx: &RequestCtx) -> Vec<u8> {
    let username = get_current_username(ctx);
    if !username.is_empty() {
        return username.into_bytes();
    }

    if basic_authentication_is_enabled() {
        let username = basic_username(ctx);
        if !username.is_empty() {
            return username;
        }
    }

    if token_authentication_is_enabled() {
        return b"token".to_vec();
    }

    if !authentication_is_enabled() {
        return b"anonymous".to_vec();
    }

    b"unknown".to_vec()
}

fn basic_username(ctx: &RequestCtx) -> Vec<u8> {
    match parse_basic_header(ctx.request.header.peek(HEADER_AUTHORIZATION)) {
        BasicHeader::Decoded {
            credentials: Some((username, _)),
            ..
        } => username,
        _ => Vec::new(),
    }
}
