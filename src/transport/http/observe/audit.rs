use crate::httpd::RequestCtx;
use crate::log::{self, Attr, Logger, ToValue};
use crate::security;

use super::observe::request_id;

const AUDIT_LOG: Logger = log::for_component("audit");

pub fn audit(ctx: &RequestCtx, msg: &str, args: &[Attr<'_>]) {
    let actor = security::actor(ctx);
    let remote_addr = security::login_source_from_request(ctx).to_string();
    let id = request_id(ctx);

    let mut attrs: Vec<Attr<'_>> = vec![
        ("actor", actor.to_value()),
        ("source", "http".to_value()),
        ("remote_addr", remote_addr.to_value()),
    ];
    if !id.is_empty() {
        attrs.push(("request_id", id.to_value()));
    }
    attrs.extend_from_slice(args);

    AUDIT_LOG.info(msg, &attrs);
}
