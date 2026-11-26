use std::sync::Arc;

use elysiandb::acl::{self, Acl, Permission};
use elysiandb::api;
use elysiandb::cache;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::httpd::{
    Request, RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NOT_FOUND,
};
use elysiandb::security;
use elysiandb::storage;
use elysiandb::sync::{Mutex, MutexGuard};
use elysiandb::transport::http::api as api_controller;
use elysiandb::transport::http::api::transactions as api_transaction;

use crate::tempdir::TempDir;

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

fn update_config(f: impl FnOnce(&mut Config)) {
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    f(&mut cfg);
    globals::set_config(Some(Arc::new(cfg)));
}

fn setup() -> Store {
    let serial = SERIAL.lock();
    let _ = storage::close();
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.api.cache.enabled = false;
    cfg.api.schema.enabled = false;
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::load_json_db();

    cache::init_cache(GoDuration(30 * GoDuration::SECOND.0));
    api::delete_all();

    Store {
        _dir: dir,
        _serial: serial,
    }
}

fn new_ctx(method: &str, uri: &str, body: &str) -> RequestCtx {
    let mut req = Request::new();
    req.set_request_uri(uri);
    req.header.set_method(method);
    if !body.is_empty() {
        req.set_body(body.as_bytes());
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn status(ctx: &RequestCtx) -> u16 {
    ctx.response.status_code()
}

fn object(raw: &[u8]) -> Map {
    let mut m: Option<Map> = None;
    let _ = json::unmarshal(raw, &mut m);
    m.unwrap_or_default()
}

fn text_field(m: &Map, key: &str) -> Option<String> {
    match m.get(key) {
        Some(Value::String(v)) => Some(v.clone()),
        _ => None,
    }
}

fn doc(text: &str) -> Map {
    let mut m = Map::new();
    if let Err(e) = json::unmarshal(text.as_bytes(), &mut m) {
        panic!("{text}: {e}");
    }
    m
}

fn begin_id() -> String {
    let mut ctx = new_ctx("POST", "/api/tx/begin", "");
    api_transaction::begin_transaction_controller(&mut ctx);

    text_field(&object(ctx.response.body_bytes()), "transaction_id").unwrap_or_default()
}

fn commit_ctx(tx_id: &str) -> RequestCtx {
    let mut ctx = new_ctx("POST", "/api/tx/commit", "");
    ctx.set_param("txId", tx_id);
    api_transaction::commit_transaction_controller(&mut ctx);
    ctx
}

#[test]
fn transaction_lifecycle() {
    let _s = setup();

    let mut ctx = new_ctx("POST", "/api/tx/begin", "");
    api_transaction::begin_transaction_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("begin failed");
    }

    let tx_id =
        text_field(&object(ctx.response.body_bytes()), "transaction_id").unwrap_or_default();

    let mut ctx = new_ctx("POST", "/api/tx/write/books", r#"{"title":"Dune"}"#);
    ctx.set_param("txId", &tx_id);
    ctx.set_param("entity", "books");
    api_transaction::write_transaction_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("write failed");
    }

    let ctx = commit_ctx(&tx_id);

    if status(&ctx) != 200 {
        panic!("commit failed");
    }
}

#[test]
fn transaction_rollback() {
    let _s = setup();

    let tx_id = begin_id();

    let mut ctx = new_ctx("POST", "/api/tx/write/test", r#"{"a":1}"#);
    ctx.set_param("txId", &tx_id);
    ctx.set_param("entity", "test");
    api_transaction::write_transaction_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("write failed");
    }

    let mut ctx = new_ctx("POST", "/api/tx/rollback", "");
    ctx.set_param("txId", &tx_id);
    api_transaction::rollback_transaction_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("rollback failed");
    }
}

#[test]
fn transaction_delete() {
    let _s = setup();

    let tx_id = begin_id();

    let mut ctx = new_ctx("POST", "/api/tx/write/test", r#"{"id":"1","a":1}"#);
    ctx.set_param("txId", &tx_id);
    ctx.set_param("entity", "test");
    api_transaction::write_transaction_controller(&mut ctx);

    commit_ctx(&tx_id);

    let tx_id = begin_id();

    let mut ctx = new_ctx("DELETE", "/api/tx/delete/test/1", "");
    ctx.set_param("txId", &tx_id);
    ctx.set_param("entity", "test");
    ctx.set_param("id", "1");
    api_transaction::delete_transaction_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("delete failed");
    }

    let ctx = commit_ctx(&tx_id);

    if status(&ctx) != 200 {
        panic!("commit after delete failed");
    }
}

#[test]
fn transaction_update() {
    let _s = setup();

    let tx_id = begin_id();

    let mut ctx = new_ctx("POST", "/api/tx/write/u", r#"{"id":"1","a":1}"#);
    ctx.set_param("txId", &tx_id);
    ctx.set_param("entity", "u");
    api_transaction::write_transaction_controller(&mut ctx);

    commit_ctx(&tx_id);

    let tx_id = begin_id();

    let mut ctx = new_ctx("PUT", "/api/tx/update/u/1", r#"{"a":2}"#);
    ctx.set_param("txId", &tx_id);
    ctx.set_param("entity", "u");
    ctx.set_param("id", "1");
    api_transaction::update_transaction_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("update failed");
    }

    let ctx = commit_ctx(&tx_id);

    if status(&ctx) != 200 {
        panic!("commit after update failed");
    }
}

fn setup_ownership() -> Store {
    let store = setup();

    update_config(|cfg| {
        cfg.security.authentication.enabled = true;
        cfg.security.authentication.mode = "user".to_string();
    });

    for username in ["alice", "bob"] {
        let mut perms = acl::new_permissions();
        perms[Permission::Create as usize] = true;
        perms[Permission::OwningUpdate as usize] = true;
        perms[Permission::OwningDelete as usize] = true;
        let a = Acl {
            username: username.as_bytes().to_vec(),
            entity: b"notes".to_vec(),
            permissions: perms,
        };
        api::write_entity(acl::ACL_ENTITY.as_bytes(), &mut a.to_data_map());
    }

    api::write_entity(
        b"notes",
        &mut doc(&format!(
            r#"{{"id":"1","title":"alice","{}":"alice"}}"#,
            acl::USERNAME_FIELD
        )),
    );

    store
}

fn begin() -> String {
    begin_id()
}

fn commit(tx_id: &str) {
    let ctx = commit_ctx(tx_id);

    if status(&ctx) != 200 {
        panic!("commit failed: {}", status(&ctx));
    }
}

fn new_tx_ctx(username: &str, method: &str, uri: &str, body: &str, tx_id: &str) -> RequestCtx {
    let mut ctx = new_ctx(method, uri, body);
    ctx.set_user_value(security::USERNAME_KEY, username.as_bytes().to_vec());
    ctx.set_param("txId", tx_id);
    ctx.set_param("entity", "notes");

    ctx
}

fn tx_write_note(username: &str, tx_id: &str, body: &str) -> RequestCtx {
    let mut ctx = new_tx_ctx(
        username,
        "POST",
        &format!("/api/tx/{tx_id}/entity/notes"),
        body,
        tx_id,
    );
    api_transaction::write_transaction_controller(&mut ctx);

    ctx
}

fn tx_update_note(username: &str, tx_id: &str, id: &str, body: &str) -> RequestCtx {
    let mut ctx = new_tx_ctx(
        username,
        "PUT",
        &format!("/api/tx/{tx_id}/entity/notes/{id}"),
        body,
        tx_id,
    );
    ctx.set_param("id", id);
    api_transaction::update_transaction_controller(&mut ctx);

    ctx
}

fn tx_delete_note(username: &str, tx_id: &str, id: &str) -> RequestCtx {
    let mut ctx = new_tx_ctx(
        username,
        "DELETE",
        &format!("/api/tx/{tx_id}/entity/notes/{id}"),
        "",
        tx_id,
    );
    ctx.set_param("id", id);
    api_transaction::delete_transaction_controller(&mut ctx);

    ctx
}

fn stored_note(id: &str) -> Map {
    api::read_entity_by_id(b"notes", id.as_bytes()).unwrap_or_default()
}

#[test]
fn transaction_update_owning_update_rejects_other_users_entity() {
    let _s = setup_ownership();

    let tx_id = begin();

    let ctx = tx_update_note(
        "bob",
        &tx_id,
        "1",
        &format!(r#"{{"title":"bob","{}":"bob"}}"#, acl::USERNAME_FIELD),
    );
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    commit(&tx_id);

    let stored = stored_note("1");
    if text_field(&stored, "title").as_deref() != Some("alice")
        || text_field(&stored, acl::USERNAME_FIELD).as_deref() != Some("alice")
    {
        panic!("note was modified: {stored:?}");
    }
}

#[test]
fn transaction_update_owner_allowed() {
    let _s = setup_ownership();

    let tx_id = begin();

    let ctx = tx_update_note("alice", &tx_id, "1", r#"{"title":"v2"}"#);
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    commit(&tx_id);

    let stored = stored_note("1");
    if text_field(&stored, "title").as_deref() != Some("v2")
        || text_field(&stored, acl::USERNAME_FIELD).as_deref() != Some("alice")
    {
        panic!("unexpected note: {stored:?}");
    }
}

#[test]
fn transaction_delete_owning_delete_rejects_other_users_entity() {
    let _s = setup_ownership();

    let tx_id = begin();

    let ctx = tx_delete_note("bob", &tx_id, "1");
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    commit(&tx_id);

    if api::read_entity_by_id(b"notes", b"1").is_none() {
        panic!("note was deleted");
    }
}

#[test]
fn transaction_delete_owner_allowed() {
    let _s = setup_ownership();

    let tx_id = begin();

    let ctx = tx_delete_note("alice", &tx_id, "1");
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    commit(&tx_id);

    if api::read_entity_by_id(b"notes", b"1").is_some() {
        panic!("note was not deleted");
    }
}

#[test]
fn transaction_write_stamps_owner() {
    let _s = setup_ownership();

    let tx_id = begin();

    let ctx = tx_write_note(
        "alice",
        &tx_id,
        &format!(
            r#"{{"id":"2","title":"new","{}":"bob"}}"#,
            acl::USERNAME_FIELD
        ),
    );
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    commit(&tx_id);

    let stored = stored_note("2");
    if text_field(&stored, "title").as_deref() != Some("new")
        || text_field(&stored, acl::USERNAME_FIELD).as_deref() != Some("alice")
    {
        panic!("unexpected note: {stored:?}");
    }
}

#[test]
fn transaction_write_existing_id_of_other_user_is_forbidden() {
    let _s = setup_ownership();

    let tx_id = begin();

    let ctx = tx_write_note("bob", &tx_id, r#"{"id":"1","title":"bob"}"#);
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    commit(&tx_id);

    let stored = stored_note("1");
    if text_field(&stored, "title").as_deref() != Some("alice")
        || text_field(&stored, acl::USERNAME_FIELD).as_deref() != Some("alice")
    {
        panic!("note was modified: {stored:?}");
    }
}

#[test]
fn transaction_write_requires_create_permission() {
    let _s = setup_ownership();

    let mut perms = acl::new_permissions();
    perms[Permission::OwningUpdate as usize] = true;
    let a = Acl {
        username: b"carol".to_vec(),
        entity: b"notes".to_vec(),
        permissions: perms,
    };
    api::write_entity(acl::ACL_ENTITY.as_bytes(), &mut a.to_data_map());

    let tx_id = begin();

    let ctx = tx_write_note("carol", &tx_id, r#"{"id":"3","title":"carol"}"#);
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    commit(&tx_id);

    if api::read_entity_by_id(b"notes", b"3").is_some() {
        panic!("note was created");
    }
}

#[test]
fn transaction_write_null_payload() {
    let _s = setup();

    let tx_id = begin();

    let mut ctx = new_tx_ctx(
        "",
        "POST",
        &format!("/api/tx/{tx_id}/entity/notes"),
        "null",
        &tx_id,
    );
    api_transaction::write_transaction_controller(&mut ctx);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!("expected 400, got {}", status(&ctx));
    }
}

fn get_note(id: &str) -> RequestCtx {
    let mut ctx = new_ctx("GET", &format!("/api/notes/{id}"), "");
    ctx.set_param("entity", "notes");
    ctx.set_param("id", id);
    api_controller::get_by_id_controller(&mut ctx);

    ctx
}

#[test]
fn transaction_commit_purges_cache() {
    let _s = setup();
    update_config(|cfg| cfg.api.cache.enabled = true);

    api::write_entity(b"notes", &mut doc(r#"{"id":"1","title":"v1"}"#));

    for want in ["MISS", "HIT"] {
        let ctx = get_note("1");
        let got = String::from_utf8_lossy(ctx.response.header.peek("X-Elysian-Cache")).into_owned();
        if got != want {
            panic!("expected {want}, got {got}");
        }
    }

    let tx_id = begin();
    tx_update_note("", &tx_id, "1", r#"{"title":"v2"}"#);
    commit(&tx_id);

    let obj = object(get_note("1").response.body_bytes());
    if text_field(&obj, "title").as_deref() != Some("v2") {
        panic!("expected updated document after commit, got {obj:?}");
    }

    let tx_id = begin();
    tx_delete_note("", &tx_id, "1");
    commit(&tx_id);

    let ctx = get_note("1");
    if status(&ctx) != STATUS_NOT_FOUND {
        panic!(
            "expected 404 after delete commit, got {}: {}",
            status(&ctx),
            String::from_utf8_lossy(ctx.response.body_bytes())
        );
    }
}
