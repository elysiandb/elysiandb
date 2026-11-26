use std::collections::BTreeMap;
use std::sync::mpsc;
use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use elysiandb::acl::{self, Acl, Permission};
use elysiandb::api;
use elysiandb::cache;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::gocompat::time::Duration as GoDuration;
use elysiandb::httpd::{
    Handler, Request, RequestCtx, STATUS_BAD_REQUEST, STATUS_FORBIDDEN, STATUS_NO_CONTENT,
    STATUS_NOT_FOUND, STATUS_OK,
};
use elysiandb::security::{self, BasicUser, Role};
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
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = false;
    cfg.security.authentication.enabled = false;
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

fn new_user_ctx(username: &str, method: &str, uri: &str, body: &str) -> RequestCtx {
    let mut ctx = new_ctx(method, uri, body);
    ctx.set_user_value(security::USERNAME_KEY, username.as_bytes().to_vec());

    ctx
}

fn status(ctx: &RequestCtx) -> u16 {
    ctx.response.status_code()
}

fn body(ctx: &RequestCtx) -> String {
    String::from_utf8_lossy(ctx.response.body_bytes()).into_owned()
}

fn object(raw: &[u8]) -> Map {
    let mut m: Option<Map> = None;
    let _ = json::unmarshal(raw, &mut m);
    m.unwrap_or_default()
}

fn list(raw: &[u8]) -> Vec<Map> {
    let mut l: Option<Vec<Map>> = None;
    let _ = json::unmarshal(raw, &mut l);
    l.unwrap_or_default()
}

fn doc(text: &str) -> Map {
    let mut m = Map::new();
    if let Err(e) = json::unmarshal(text.as_bytes(), &mut m) {
        panic!("{text}: {e}");
    }
    m
}

fn write(entity: &str, text: &str) {
    api::write_entity(entity.as_bytes(), &mut doc(text));
}

fn hex_of(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn id_of(ctx: &RequestCtx) -> String {
    match object(ctx.response.body_bytes()).get("id") {
        Some(Value::String(id)) => id.clone(),
        other => panic!("no id in response: {other:?}"),
    }
}

fn text_field(m: &Map, key: &str) -> Option<String> {
    match m.get(key) {
        Some(Value::String(v)) => Some(v.clone()),
        _ => None,
    }
}

fn create(entity: &str, body: &str) -> RequestCtx {
    let mut ctx = new_ctx("POST", &format!("/api/{entity}"), body);
    ctx.set_param("entity", entity);
    api_controller::create_controller(&mut ctx);
    ctx
}

#[test]
fn test_create_and_get_by_id() {
    let _s = setup();

    let ctx = create("books", r#"{"title":"Dune"}"#);

    if status(&ctx) != 200 {
        panic!("create failed");
    }

    let id = id_of(&ctx);

    let mut ctx = new_ctx("GET", &format!("/api/books/{id}"), "");
    ctx.set_param("entity", "books");
    ctx.set_param("id", &id);
    api_controller::get_by_id_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("get by id failed");
    }
}

#[test]
fn test_list_and_count() {
    let _s = setup();

    for _ in 0..3 {
        create("item", r#"{"name":"x"}"#);
    }

    let mut ctx = new_ctx("GET", "/api/item", "");
    ctx.set_param("entity", "item");
    api_controller::list_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("list failed");
    }

    let mut ctx = new_ctx("GET", "/api/item/count", "");
    ctx.set_param("entity", "item");
    api_controller::count_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("count failed");
    }
}

#[test]
fn test_update_by_id() {
    let _s = setup();

    let ctx = create("profile", r#"{"name":"alice"}"#);
    let id = id_of(&ctx);

    let mut ctx = new_ctx("PUT", &format!("/api/profile/{id}"), r#"{"name":"bob"}"#);
    ctx.set_param("entity", "profile");
    ctx.set_param("id", &id);
    api_controller::update_by_id_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("update failed");
    }
}

fn create_type(entity: &str, body: &str) -> RequestCtx {
    let mut ctx = new_ctx("POST", &format!("/api/{entity}/create"), body);
    ctx.set_param("entity", entity);
    api_controller::create_type_controller(&mut ctx);
    ctx
}

#[test]
fn test_update_by_id_keeps_strict_manual_schema() {
    let _s = setup();

    update_config(|cfg| cfg.api.schema.strict = true);

    create_type(
        "oeuvre",
        r#"{"fields":{"titre":{"type":"string","required":true},"annee":{"type":"number","required":false}}}"#,
    );

    let ctx = create("oeuvre", r#"{"id":"o1","titre":"Alien"}"#);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on create, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let mut ctx = new_ctx("PUT", "/api/oeuvre/o1", r#"{"titre":"Alien (1979)"}"#);
    ctx.set_param("entity", "oeuvre");
    ctx.set_param("id", "o1");
    api_controller::update_by_id_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on update, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let ctx = create("oeuvre", r#"{"titre":"X","note":9}"#);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!(
            "expected 400 for undeclared field after update, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let ctx = create("oeuvre", r#"{"annee":1979}"#);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!(
            "expected 400 for missing required field after update, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }
}

#[test]
fn test_update_list() {
    let _s = setup();

    create("e", r#"[{"a":1},{"a":2}]"#);

    let mut ctx = new_ctx("PUT", "/api/e", r#"[{"a":9}]"#);
    ctx.set_param("entity", "e");
    api_controller::update_list_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("update list failed");
    }
}

fn update_by_id(entity: &str, id: &str, body: &str) -> RequestCtx {
    let mut ctx = new_ctx("PUT", &format!("/api/{entity}/{id}"), body);
    ctx.set_param("entity", entity);
    ctx.set_param("id", id);
    api_controller::update_by_id_controller(&mut ctx);

    ctx
}

fn update_list(entity: &str, body: &str) -> RequestCtx {
    let mut ctx = new_ctx("PUT", &format!("/api/{entity}"), body);
    ctx.set_param("entity", entity);
    api_controller::update_list_controller(&mut ctx);

    ctx
}

#[test]
fn test_update_by_id_missing_entity_is_not_found() {
    let _s = setup();

    write("movie", r#"{"id":"m1","title":"Alien"}"#);

    let ctx = update_by_id("movie", "nope", r#"{"title":"x"}"#);
    if status(&ctx) != STATUS_NOT_FOUND || body(&ctx) != r#"{"error":"entity not found"}"# {
        panic!("expected 404, got {}: {}", status(&ctx), body(&ctx));
    }

    if api::read_entity_by_id(b"movie", b"nope").is_some() {
        panic!("a missing document must not be created by an update");
    }
}

#[test]
fn test_update_by_id_missing_entity_is_not_found_before_strict_validation() {
    let _s = setup();

    update_config(|cfg| cfg.api.schema.strict = true);

    create_type(
        "book",
        r#"{"fields":{"title":{"type":"string","required":true},"year":{"type":"number","required":false}}}"#,
    );

    let ctx = update_by_id("book", "nope", r#"{"year":2000}"#);
    if status(&ctx) != STATUS_NOT_FOUND {
        panic!(
            "expected 404 instead of a validation error, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }
}

#[test]
fn test_update_list_missing_entities_are_not_found() {
    let _s = setup();

    write("movie", r#"{"id":"m1","title":"Alien"}"#);

    let ctx = update_list(
        "movie",
        r#"[{"id":"m1","title":"Alien (1979)"},{"id":"nope","title":"x"},{"id":"nope","title":"y"},{"id":"gone"}]"#,
    );
    if status(&ctx) != STATUS_NOT_FOUND {
        panic!("expected 404, got {}: {}", status(&ctx), body(&ctx));
    }

    if body(&ctx) != r#"{"error":"entity not found","ids":["nope","gone"]}"# {
        panic!("expected the missing ids, got {}", body(&ctx));
    }

    let stored = api::read_entity_by_id(b"movie", b"m1").unwrap_or_default();
    if text_field(&stored, "title").as_deref() != Some("Alien") {
        panic!(
            "a batch with a missing id must not apply any update, got title {:?}",
            stored.get("title")
        );
    }

    let ctx = update_list("movie", r#"[{"id":"m1","title":"Alien (1979)"}]"#);
    if status(&ctx) != STATUS_OK {
        panic!("expected 200, got {}: {}", status(&ctx), body(&ctx));
    }
}

#[test]
fn test_update_list_invalid_body_is_bad_request() {
    let _s = setup();

    for body in [r#"{"id":"m1"}"#, "[]", "not json"] {
        let ctx = update_list("movie", body);
        if status(&ctx) != STATUS_BAD_REQUEST {
            panic!("body {body}: expected 400, got {}", status(&ctx));
        }
    }
}

#[test]
fn test_delete_and_destroy() {
    let _s = setup();

    let ctx = create("d", r#"{"x":1}"#);
    let id = id_of(&ctx);

    let mut ctx = new_ctx("DELETE", &format!("/api/d/{id}"), "");
    ctx.set_param("entity", "d");
    ctx.set_param("id", &id);
    api_controller::delete_by_id_controller(&mut ctx);

    if status(&ctx) != STATUS_NO_CONTENT {
        panic!("delete by id failed");
    }

    let mut ctx = new_ctx("DELETE", "/api/d", "");
    ctx.set_param("entity", "d");
    api_controller::destroy_controller(&mut ctx);

    if status(&ctx) != STATUS_NO_CONTENT {
        panic!("destroy failed");
    }
}

#[test]
fn test_create_type_controller_ok() {
    let _s = setup();

    let body = r#"{"fields":{"title":"string","age":"number"}}"#;
    let mut ctx = new_ctx("POST", "/api/type/person", body);
    ctx.set_param("entity", "person");

    api_controller::create_type_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!("expected 200, got {}", status(&ctx));
    }

    if !api::entity_type_exists(b"person") {
        panic!("entity type not created");
    }
}

fn enable_user_authentication() {
    update_config(|cfg| {
        cfg.api.schema.strict = true;
        cfg.security.authentication.enabled = true;
        cfg.security.authentication.mode = "user".to_string();
    });

    if let Err(err) = security::init_admin_user_if_not_exists() {
        panic!("admin user creation failed: {err}");
    }
}

fn admin_ctx(method: &str, uri: &str, body: &str, entity: &str) -> RequestCtx {
    let mut ctx = new_user_ctx(security::DEFAULT_ADMIN_USERNAME, method, uri, body);
    if !entity.is_empty() {
        ctx.set_param("entity", entity);
    }
    ctx
}

#[test]
fn test_create_strict_manual_schema_with_user_authentication() {
    let _s = setup();

    enable_user_authentication();

    let mut ctx = admin_ctx(
        "POST",
        "/api/oeuvre/create",
        r#"{"fields":{"titre":{"type":"string","required":true},"annee":{"type":"number","required":false}}}"#,
        "oeuvre",
    );
    api_controller::create_type_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on type creation, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let mut ctx = admin_ctx(
        "POST",
        "/api/oeuvre",
        r#"{"titre":"Alien","annee":1979}"#,
        "oeuvre",
    );
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!("expected 200, got {}: {}", status(&ctx), body(&ctx));
    }

    let obj = object(ctx.response.body_bytes());
    if text_field(&obj, acl::USERNAME_FIELD).as_deref() != Some("admin") {
        panic!(
            "expected owner admin, got {:?}",
            obj.get(acl::USERNAME_FIELD)
        );
    }

    let mut ctx = admin_ctx(
        "POST",
        "/api/oeuvre",
        r#"[{"titre":"Alien"},{"titre":"Aliens","annee":1986}]"#,
        "oeuvre",
    );
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on list creation, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let mut ctx = admin_ctx("POST", "/api/oeuvre", r#"{"titre":"X","note":9}"#, "oeuvre");
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!("expected 400 for undeclared field, got {}", status(&ctx));
    }
}

fn assert_manual_schema(step: &str) {
    let mut ctx = admin_ctx("GET", "/api/oeuvre/schema", "", "oeuvre");
    api_controller::get_schema_controller(&mut ctx);

    let sc = object(ctx.response.body_bytes());
    if sc.get("_manual") != Some(&Value::Bool(true)) {
        panic!("manual schema lost after {step}: {}", body(&ctx));
    }

    let field = |name: &str| match sc.get("fields") {
        Some(Value::Object(fields)) => match fields.get(name) {
            Some(Value::Object(f)) => f.clone(),
            _ => Map::new(),
        },
        _ => Map::new(),
    };
    let titre = field("titre");
    let annee = field("annee");
    if titre.get("required") != Some(&Value::Bool(true))
        || annee.get("required") != Some(&Value::Bool(false))
    {
        panic!("required flags changed after {step}: {}", body(&ctx));
    }
}

#[test]
fn test_update_strict_manual_schema_with_user_authentication_keeps_schema() {
    let _s = setup();

    enable_user_authentication();

    let mut ctx = admin_ctx(
        "POST",
        "/api/oeuvre/create",
        r#"{"fields":{"titre":{"type":"string","required":true},"annee":{"type":"number","required":false}}}"#,
        "oeuvre",
    );
    api_controller::create_type_controller(&mut ctx);

    let mut ctx = admin_ctx(
        "POST",
        "/api/oeuvre",
        r#"{"id":"o1","titre":"Alien","annee":1979}"#,
        "oeuvre",
    );
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on create, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let mut ctx = admin_ctx(
        "PUT",
        "/api/oeuvre/o1",
        r#"{"titre":"Alien","annee":1980}"#,
        "oeuvre",
    );
    ctx.set_param("id", "o1");
    api_controller::update_by_id_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on update, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    assert_manual_schema("PUT");

    let mut ctx = admin_ctx("POST", "/api/tx/begin", "", "");
    api_transaction::begin_transaction_controller(&mut ctx);

    let begin = object(ctx.response.body_bytes());
    let tx_id = text_field(&begin, "transaction_id").unwrap_or_default();

    let mut ctx = admin_ctx(
        "PUT",
        &format!("/api/tx/{tx_id}/entity/oeuvre/o1"),
        r#"{"annee":1990}"#,
        "oeuvre",
    );
    ctx.set_param("txId", &tx_id);
    ctx.set_param("id", "o1");
    api_transaction::update_transaction_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on transactional update, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let mut ctx = admin_ctx("POST", &format!("/api/tx/{tx_id}/commit"), "", "");
    ctx.set_param("txId", &tx_id);
    api_transaction::commit_transaction_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!(
            "expected 200 on commit, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    assert_manual_schema("transaction commit");

    let stored = api::read_entity_by_id(b"oeuvre", b"o1").unwrap_or_default();
    if stored.get("annee") != Some(&Value::Number(1990.0))
        || text_field(&stored, acl::USERNAME_FIELD).as_deref()
            != Some(security::DEFAULT_ADMIN_USERNAME)
    {
        panic!("expected updated document owned by admin, got {stored:?}");
    }

    let mut ctx = admin_ctx("POST", "/api/oeuvre", r#"{"titre":"Y","note":9}"#, "oeuvre");
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!(
            "expected 400 for undeclared field after updates, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }

    let mut ctx = admin_ctx("POST", "/api/oeuvre", r#"{"annee":2000}"#, "oeuvre");
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!(
            "expected 400 for missing required field after updates, got {}: {}",
            status(&ctx),
            body(&ctx)
        );
    }
}

#[test]
fn test_create_type_controller_invalid_json() {
    let _s = setup();

    let mut ctx = new_ctx("POST", "/api/type/badjson", "{invalid");
    ctx.set_param("entity", "badjson");

    api_controller::create_type_controller(&mut ctx);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!("expected 400, got {}", status(&ctx));
    }

    if api::entity_type_exists(b"badjson") {
        panic!("type should be rolled back on invalid json");
    }
}

#[test]
fn test_create_type_controller_no_fields() {
    let _s = setup();

    let mut ctx = new_ctx("POST", "/api/type/test", r#"{"x":1}"#);
    ctx.set_param("entity", "test");

    api_controller::create_type_controller(&mut ctx);

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!("expected 400, got {}", status(&ctx));
    }

    if api::entity_type_exists(b"test") {
        panic!("type should be rolled back when no fields provided");
    }
}

fn entities_of(ctx: &RequestCtx) -> Vec<Value> {
    match object(ctx.response.body_bytes()).get("entities") {
        Some(Value::Array(a)) => a.clone(),
        other => panic!("no entities list: {other:?}"),
    }
}

#[test]
fn test_get_entity_types_names_controller_empty() {
    let _s = setup();

    let mut ctx = new_ctx("GET", "/api/types", "");
    api_controller::get_entity_types_names_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!("expected 200, got {}", status(&ctx));
    }

    let entities = entities_of(&ctx);
    if !entities.is_empty() {
        panic!("expected empty list, got {entities:?}");
    }
}

#[test]
fn test_get_entity_types_names_controller_with_entities() {
    let _s = setup();

    let _ = api::create_entity_type(b"book");
    let _ = api::create_entity_type(b"user");

    let mut ctx = new_ctx("GET", "/api/types", "");
    api_controller::get_entity_types_names_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!("expected 200, got {}", status(&ctx));
    }

    let raw = entities_of(&ctx);

    if raw.len() != 2 {
        panic!("expected 2 entities, got {raw:?}");
    }

    let found: Vec<&str> = raw.iter().filter_map(Value::as_str).collect();

    if !found.contains(&"book") || !found.contains(&"user") {
        panic!("unexpected entities list: {raw:?}");
    }
}

fn write_hook(id: &str, name: &str, entity: &str, priority: f64, script: &str) {
    let mut hook = Map::new();
    hook.insert("id".to_string(), s(id));
    hook.insert("name".to_string(), s(name));
    hook.insert("entity".to_string(), s(entity));
    hook.insert("event".to_string(), s(name));
    hook.insert("language".to_string(), s("javascript"));
    hook.insert("priority".to_string(), Value::Number(priority));
    hook.insert("enabled".to_string(), Value::Bool(true));
    hook.insert("bypass_acl".to_string(), Value::Bool(true));
    hook.insert("script".to_string(), s(script));
    api::write_entity(b"_elysiandb_core_hook", &mut hook);
}

fn enable_hooks(enabled: bool) {
    update_config(|cfg| cfg.api.hooks.enabled = enabled);
}

fn list_of(entity: &str, uri: &str) -> RequestCtx {
    let mut ctx = new_ctx("GET", uri, "");
    ctx.set_param("entity", entity);
    api_controller::list_controller(&mut ctx);
    ctx
}

#[test]
fn test_get_by_id_with_post_read_hook() {
    let _s = setup();

    enable_hooks(true);

    let _ = api::create_entity_type(b"book");

    write_hook(
        "h1",
        "post_read",
        "book",
        1.0,
        "\nfunction postRead(ctx) {\n  ctx.entity.hooked = true\n  return ctx.entity\n}\n",
    );

    let ctx = create("book", r#"{"title":"Dune"}"#);
    let id = id_of(&ctx);

    let mut ctx = new_ctx("GET", &format!("/api/book/{id}"), "");
    ctx.set_param("entity", "book");
    ctx.set_param("id", &id);
    api_controller::get_by_id_controller(&mut ctx);

    if status(&ctx) != STATUS_OK {
        panic!("expected 200");
    }

    let obj = object(ctx.response.body_bytes());

    if obj.get("hooked") != Some(&Value::Bool(true)) {
        panic!("hook not applied");
    }
}

#[test]
fn test_list_with_post_read_hook() {
    let _s = setup();

    enable_hooks(true);

    let _ = api::create_entity_type(b"item");

    write_hook(
        "h2",
        "post_read",
        "item",
        1.0,
        "\nfunction postRead(ctx) {\n  ctx.entity.fromHook = \"yes\"\n  return ctx.entity\n}\n",
    );

    for _ in 0..2 {
        create("item", r#"{"a":1}"#);
    }

    let ctx = list_of("item", "/api/item");

    if status(&ctx) != STATUS_OK {
        panic!("expected 200");
    }

    let items = list(ctx.response.body_bytes());

    if items.len() != 2 {
        panic!("expected 2 items");
    }

    for it in &items {
        if text_field(it, "fromHook").as_deref() != Some("yes") {
            panic!("hook not applied on list");
        }
    }
}

#[test]
fn test_list_without_hooks_enabled() {
    let _s = setup();

    enable_hooks(false);

    let _ = api::create_entity_type(b"plain");

    write_hook(
        "h3",
        "post_read",
        "plain",
        1.0,
        "\nfunction postRead(ctx) {\n  ctx.entity.x = 1\n  return ctx.entity\n}\n",
    );

    create("plain", r#"{"a":1}"#);

    let ctx = list_of("plain", "/api/plain");

    let out = list(ctx.response.body_bytes());

    if out[0].contains_key("x") {
        panic!("hook should not be applied when disabled");
    }
}

#[test]
fn test_get_by_id_no_hook_for_entity() {
    let _s = setup();

    enable_hooks(true);

    let _ = api::create_entity_type(b"nohook");

    let ctx = create("nohook", r#"{"a":1}"#);
    let id = id_of(&ctx);

    let mut ctx = new_ctx("GET", &format!("/api/nohook/{id}"), "");
    ctx.set_param("entity", "nohook");
    ctx.set_param("id", &id);
    api_controller::get_by_id_controller(&mut ctx);

    let obj = object(ctx.response.body_bytes());

    if obj.contains_key("hooked") {
        panic!("unexpected hook application");
    }
}

#[test]
fn test_list_with_pre_read_hook_filtering() {
    let _s = setup();

    enable_hooks(true);

    let _ = api::create_entity_type(b"task");

    write_hook(
        "h_pre_1",
        "pre_read",
        "task",
        1.0,
        "\nfunction preRead(ctx) {\n  ctx.entity.allowed = ctx.entity.value === 1\n  return ctx.entity\n}\n",
    );

    create("task", r#"{"value":1}"#);
    create("task", r#"{"value":2}"#);

    let ctx = list_of("task", "/api/task?filter[allowed][eq]=true");

    if status(&ctx) != STATUS_OK {
        panic!("expected 200");
    }

    let items = list(ctx.response.body_bytes());

    if items.len() != 1 {
        panic!("expected 1 item after pre_read filtering");
    }

    if items[0].get("value") != Some(&Value::Number(1.0)) {
        panic!("unexpected filtered item");
    }
}

#[test]
fn test_list_pre_read_hook_ignored_when_disabled() {
    let _s = setup();

    enable_hooks(false);

    let _ = api::create_entity_type(b"preoff");

    write_hook(
        "h_pre_2",
        "pre_read",
        "preoff",
        1.0,
        "\nfunction preRead(ctx) {\n  ctx.entity.flag = true\n  return ctx.entity\n}\n",
    );

    create("preoff", r#"{"a":1}"#);

    let ctx = list_of("preoff", "/api/preoff?filter[flag][eq]=true");

    let items = list(ctx.response.body_bytes());

    if !items.is_empty() {
        panic!("expected no item when pre_read is disabled, got {items:?}");
    }
}

#[test]
fn test_list_pre_read_hook_filtering_excludes_missing_stored_field() {
    let _s = setup();

    enable_hooks(true);

    let _ = api::create_entity_type(b"ticket");

    write_hook(
        "h_pre_4",
        "pre_read",
        "ticket",
        1.0,
        "\nfunction preRead(ctx) {\n  ctx.entity.allowed = ctx.entity.value === 1\n  return ctx.entity\n}\n",
    );

    for body in [
        r#"{"value":1,"tag":"a"}"#,
        r#"{"value":1}"#,
        r#"{"value":2,"tag":"a"}"#,
    ] {
        create("ticket", body);
    }

    let ctx = list_of(
        "ticket",
        "/api/ticket?filter[allowed][eq]=true&filter[tag][eq]=a",
    );

    let items = list(ctx.response.body_bytes());

    if items.len() != 1
        || items[0].get("value") != Some(&Value::Number(1.0))
        || text_field(&items[0], "tag").as_deref() != Some("a")
    {
        panic!("expected only the allowed ticket tagged a, got {items:?}");
    }
}

#[test]
fn test_list_pre_read_and_post_read_together() {
    let _s = setup();

    enable_hooks(true);

    let _ = api::create_entity_type(b"combo");

    write_hook(
        "h_pre_3",
        "pre_read",
        "combo",
        2.0,
        "\nfunction preRead(ctx) {\n  ctx.entity.keep = ctx.entity.x === 1\n  return ctx.entity\n}\n",
    );

    write_hook(
        "h_post_3",
        "post_read",
        "combo",
        1.0,
        "\nfunction postRead(ctx) {\n  ctx.entity.after = \"ok\"\n  return ctx.entity\n}\n",
    );

    create("combo", r#"{"x":1}"#);
    create("combo", r#"{"x":2}"#);

    let ctx = list_of("combo", "/api/combo?filter[keep][eq]=true");

    let items = list(ctx.response.body_bytes());

    if items.len() != 1 {
        panic!("expected 1 item");
    }

    if text_field(&items[0], "after").as_deref() != Some("ok") {
        panic!("post_read hook not applied");
    }
}

fn query(body: &str) -> RequestCtx {
    let mut ctx = new_ctx("POST", "/api/query", body);
    api_controller::query_controller(&mut ctx);
    ctx
}

#[test]
fn test_query_controller_basic_query() {
    let _s = setup();

    write("book", r#"{"id":"1","title":"Go"}"#);
    write("book", r#"{"id":"2","title":"Rust"}"#);

    let ctx = query(
        r#"{
		"entity":"book",
		"filters":{"title":{"eq":"Go"}}
	}"#,
    );

    if status(&ctx) != STATUS_OK {
        panic!("expected 200");
    }

    let out = list(ctx.response.body_bytes());

    if out.len() != 1 || text_field(&out[0], "id").as_deref() != Some("1") {
        panic!("unexpected result {out:?}");
    }
}

#[test]
fn test_query_controller_count_only() {
    let _s = setup();

    for id in ["a", "b", "c"] {
        write("x", &format!(r#"{{"id":"{id}"}}"#));
    }

    let ctx = query(
        r#"{
		"entity":"x",
		"countOnly":true,
		"filters":{"id":{"neq":"__never__"}}
	}"#,
    );

    let mut out: BTreeMap<String, i64> = BTreeMap::new();
    let _ = json::unmarshal(ctx.response.body_bytes(), &mut out);

    if out.get("count") != Some(&3) {
        panic!("expected count 3, got {out:?}");
    }
}

#[test]
fn test_query_controller_fields_filtering() {
    let _s = setup();

    write("f", r#"{"id":"1","title":"Go","meta":{"x":1}}"#);

    let ctx = query(
        r#"{
		"entity":"f",
		"fields":"id,title",
		"filters":{"id":{"eq":"1"}}
	}"#,
    );

    let out = list(ctx.response.body_bytes());

    if out.len() != 1 {
        panic!("expected 1 result, got {out:?}");
    }

    if out[0].contains_key("meta") {
        panic!("meta should be filtered out");
    }
}

#[test]
fn test_query_controller_invalid_json() {
    let _s = setup();

    let ctx = query("{invalid");

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!("expected 400");
    }
}

#[test]
fn test_query_controller_invalid_filter() {
    let _s = setup();

    let ctx = query(
        r#"{
		"entity":"x",
		"filters":{"or":"nope"}
	}"#,
    );

    if status(&ctx) != STATUS_BAD_REQUEST {
        panic!("expected 400");
    }
}

#[test]
fn test_query_controller_cache_hit_and_miss() {
    let _s = setup();

    update_config(|cfg| {
        cfg.api.cache.enabled = true;
        cfg.api.hooks.enabled = false;
    });

    api::delete_all_entities(b"_elysiandb_core_hook");
    let _ = api::delete_entity_type(b"_elysiandb_core_hook");

    write("cachetest", r#"{"id":"1","v":1}"#);

    let body = r#"{
		"entity":"cachetest",
		"filters":{"id":{"eq":"1"}}
	}"#;

    query(body);

    let ctx2 = query(body);

    if ctx2.response.header.peek("X-Elysian-Cache") != b"HIT" {
        panic!("expected cache HIT");
    }
}

#[test]
fn test_query_payload_hash_deterministic() {
    let p = api_controller::QueryPayload {
        entity: "a".to_string(),
        filters: Some(doc(r#"{"or":[{"x":{"eq":"1"}},{"y":{"eq":"2"}}]}"#)),
        sorts: Some(BTreeMap::from([
            ("b".to_string(), "asc".to_string()),
            ("a".to_string(), "desc".to_string()),
        ])),
        ..api_controller::QueryPayload::default()
    };

    let h1 = hex_of(&p.hash());
    let h2 = hex_of(&p.hash());

    if h1 != h2 {
        panic!("hash must be stable, got {h1} vs {h2}");
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
        perms[Permission::OwningRead as usize] = true;
        perms[Permission::OwningUpdate as usize] = true;
        let a = Acl {
            username: username.as_bytes().to_vec(),
            entity: b"notes".to_vec(),
            permissions: perms,
        };
        api::write_entity(acl::ACL_ENTITY.as_bytes(), &mut a.to_data_map());
    }

    store
}

fn create_notes_as(username: &str, body: &str) -> RequestCtx {
    let mut ctx = new_user_ctx(username, "POST", "/api/notes", body);
    ctx.set_param("entity", "notes");
    api_controller::create_controller(&mut ctx);

    ctx
}

fn create_note_as(username: &str, body: &str) -> String {
    let ctx = create_notes_as(username, body);
    if status(&ctx) != 200 {
        panic!("create failed: {}", status(&ctx));
    }

    id_of(&ctx)
}

fn update_note_as(username: &str, id: &str, body: &str) -> RequestCtx {
    let mut ctx = new_user_ctx(username, "PUT", &format!("/api/notes/{id}"), body);
    ctx.set_param("entity", "notes");
    ctx.set_param("id", id);
    api_controller::update_by_id_controller(&mut ctx);

    ctx
}

fn update_notes_as(username: &str, body: &str) -> RequestCtx {
    let mut ctx = new_user_ctx(username, "PUT", "/api/notes", body);
    ctx.set_param("entity", "notes");
    api_controller::update_list_controller(&mut ctx);

    ctx
}

fn assert_note(id: &str, owner: &str, title: &str) {
    let Some(stored) = api::read_entity_by_id(b"notes", id.as_bytes()) else {
        panic!("note {id} not found");
    };

    if text_field(&stored, acl::USERNAME_FIELD).as_deref() != Some(owner)
        || text_field(&stored, "title").as_deref() != Some(title)
    {
        panic!("unexpected note {id}: {stored:?}");
    }
}

#[test]
fn test_update_by_id_owning_update_rejects_other_users_entity() {
    let _s = setup_ownership();

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    let ctx = update_note_as(
        "bob",
        &id,
        &format!(r#"{{"title":"bob","{}":"bob"}}"#, acl::USERNAME_FIELD),
    );
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    let ctx = update_note_as("bob", &id, r#"{"title":"bob"}"#);
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    assert_note(&id, "alice", "alice");
}

#[test]
fn test_update_by_id_owner_can_update_without_username_field() {
    let _s = setup_ownership();

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    let ctx = update_note_as("alice", &id, r#"{"title":"v2"}"#);
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    assert_note(&id, "alice", "v2");
}

#[test]
fn test_update_by_id_owning_update_cannot_change_owner() {
    let _s = setup_ownership();

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    let ctx = update_note_as(
        "alice",
        &id,
        &format!(r#"{{"title":"v2","{}":"bob"}}"#, acl::USERNAME_FIELD),
    );
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    assert_note(&id, "alice", "alice");
}

#[test]
fn test_update_by_id_id_in_payload_does_not_redirect_write() {
    let _s = setup_ownership();

    let alice_id = create_note_as("alice", r#"{"title":"alice"}"#);
    let bob_id = create_note_as("bob", r#"{"title":"bob"}"#);

    let ctx = update_note_as(
        "bob",
        &bob_id,
        &format!(
            r#"{{"id":"{alice_id}","title":"bob v2","{}":"bob"}}"#,
            acl::USERNAME_FIELD
        ),
    );
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    assert_note(&alice_id, "alice", "alice");
    assert_note(&bob_id, "bob", "bob v2");
}

#[test]
fn test_update_by_id_missing_entity_is_not_found_before_acl() {
    let _s = setup_ownership();

    let ctx = update_note_as("bob", "nope", r#"{"title":"bob"}"#);
    if status(&ctx) != STATUS_NOT_FOUND {
        panic!("expected 404, got {}", status(&ctx));
    }

    let ctx = update_notes_as("bob", r#"[{"id":"nope","title":"bob"}]"#);
    if status(&ctx) != STATUS_NOT_FOUND {
        panic!("expected 404, got {}", status(&ctx));
    }
}

#[test]
fn test_update_list_owning_update_rejects_other_users_entity() {
    let _s = setup_ownership();

    let alice_id = create_note_as("alice", r#"{"title":"alice"}"#);
    let bob_id = create_note_as("bob", r#"{"title":"bob"}"#);

    let ctx = update_notes_as(
        "bob",
        &format!(
            r#"[{{"id":"{bob_id}","title":"bob v2"}},{{"id":"{alice_id}","title":"bob","{}":"bob"}}]"#,
            acl::USERNAME_FIELD
        ),
    );
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    assert_note(&alice_id, "alice", "alice");
    assert_note(&bob_id, "bob", "bob");
}

#[test]
fn test_update_list_owner_can_update_without_username_field() {
    let _s = setup_ownership();

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    let ctx = update_notes_as("alice", &format!(r#"[{{"id":"{id}","title":"v2"}}]"#));
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    assert_note(&id, "alice", "v2");
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

fn new_session_ctx(username: &str, method: &str, uri: &str, body: &str) -> RequestCtx {
    ensure_user(username, Role::user());
    let session = match security::create_session(
        username.as_bytes(),
        Role::user(),
        Duration::from_secs(3600),
    ) {
        Ok(session) => session,
        Err(err) => panic!("session creation failed: {err}"),
    };

    let mut req = Request::new();
    req.set_request_uri(uri);
    req.header.set_method(method);
    req.header
        .set_cookie(security::SESSION_COOKIE_NAME, &session.id);
    if !body.is_empty() {
        req.set_body(body.as_bytes());
    }
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);

    ctx
}

fn run_during_concurrent_login(
    first: RequestCtx,
    handler: fn(&mut RequestCtx),
    second: RequestCtx,
) -> RequestCtx {
    let (first_authenticated, first_seen) = mpsc::channel::<()>();
    let (second_authenticated, second_seen) = mpsc::channel::<()>();
    let second_seen = StdMutex::new(second_seen);
    let first_authenticated = StdMutex::new(first_authenticated);

    let done = std::thread::spawn(move || {
        let mut first = first;
        let wrapped: Handler = Arc::new(move |ctx: &mut RequestCtx| {
            if let Ok(tx) = first_authenticated.lock() {
                let _ = tx.send(());
            }
            if let Ok(rx) = second_seen.lock() {
                let _ = rx.recv();
            }
            handler(ctx);
        });
        security::user_auth(wrapped)(&mut first);
        first
    });

    let _ = first_seen.recv();

    let mut second = second;
    let second_authenticated = StdMutex::new(second_authenticated);
    let release: Handler = Arc::new(move |_ctx: &mut RequestCtx| {
        if let Ok(tx) = second_authenticated.lock() {
            let _ = tx.send(());
        }
    });
    security::user_auth(release)(&mut second);

    match done.join() {
        Ok(first) => first,
        Err(_) => panic!("the first request panicked"),
    }
}

#[test]
fn test_create_owner_comes_from_own_request_during_concurrent_login() {
    let _s = setup_ownership();

    let mut alice_ctx = new_session_ctx("alice", "POST", "/api/notes", r#"{"title":"alice"}"#);
    alice_ctx.set_param("entity", "notes");
    let bob_ctx = new_session_ctx("bob", "GET", "/api/notes", "");

    let alice_ctx =
        run_during_concurrent_login(alice_ctx, api_controller::create_controller, bob_ctx);

    if status(&alice_ctx) != 200 {
        panic!("expected 200, got {}", status(&alice_ctx));
    }

    let obj = object(alice_ctx.response.body_bytes());
    if text_field(&obj, acl::USERNAME_FIELD).as_deref() != Some("alice") {
        panic!(
            "expected owner alice, got {:?}",
            obj.get(acl::USERNAME_FIELD)
        );
    }
}

#[test]
fn test_update_by_id_acl_uses_own_request_user_during_concurrent_login() {
    let _s = setup_ownership();

    write(
        "notes",
        &format!(
            r#"{{"id":"n1","title":"alice","{}":"alice"}}"#,
            acl::USERNAME_FIELD
        ),
    );

    let mut bob_ctx = new_session_ctx("bob", "PUT", "/api/notes/n1", r#"{"title":"bob"}"#);
    bob_ctx.set_param("entity", "notes");
    bob_ctx.set_param("id", "n1");
    let alice_ctx = new_session_ctx("alice", "GET", "/api/notes", "");

    let bob_ctx =
        run_during_concurrent_login(bob_ctx, api_controller::update_by_id_controller, alice_ctx);

    if status(&bob_ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&bob_ctx));
    }

    assert_note("n1", "alice", "alice");
}

fn count_notes() -> usize {
    api::list_entities(b"notes", 0, 0, b"", true, None, b"", b"").len()
}

#[test]
fn test_create_existing_id_of_other_user_is_forbidden() {
    let _s = setup_ownership();

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    let ctx = create_notes_as("bob", &format!(r#"{{"id":"{id}","title":"bob"}}"#));
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    let ctx = create_notes_as(
        "bob",
        &format!(r#"[{{"title":"bob new"}},{{"id":"{id}","title":"bob"}}]"#),
    );
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403 on list, got {}", status(&ctx));
    }

    assert_note(&id, "alice", "alice");

    let n = count_notes();
    if n != 1 {
        panic!("expected 1 note, got {n}");
    }
}

#[test]
fn test_create_owner_can_rewrite_own_entity() {
    let _s = setup_ownership();

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    let ctx = create_notes_as("alice", &format!(r#"{{"id":"{id}","title":"v2"}}"#));
    if status(&ctx) != 200 {
        panic!("expected 200, got {}", status(&ctx));
    }

    assert_note(&id, "alice", "v2");
}

#[test]
fn test_create_requires_create_permission() {
    let _s = setup_ownership();

    let mut perms = acl::new_permissions();
    perms[Permission::OwningRead as usize] = true;
    perms[Permission::OwningUpdate as usize] = true;
    let a = Acl {
        username: b"carol".to_vec(),
        entity: b"notes".to_vec(),
        permissions: perms,
    };
    api::write_entity(acl::ACL_ENTITY.as_bytes(), &mut a.to_data_map());

    let ctx = create_notes_as("carol", r#"{"title":"carol"}"#);
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    let ctx = create_notes_as("carol", r#"[{"title":"carol"}]"#);
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403 on list, got {}", status(&ctx));
    }

    let n = count_notes();
    if n != 0 {
        panic!("expected no note, got {n}");
    }
}

#[test]
fn test_create_core_entity_is_forbidden() {
    let _s = setup_ownership();

    let mut ctx = new_user_ctx(
        "bob",
        "POST",
        &format!("/api/{}", acl::ACL_ENTITY),
        r#"{"id":"bob::notes","username":"bob","entity":"notes","permissions":{"update":true}}"#,
    );
    ctx.set_param("entity", acl::ACL_ENTITY);
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }

    if acl::get_acl_entity_for_username(b"notes", b"bob").is_some_and(|a| a.can(Permission::Update))
    {
        panic!("acl was modified");
    }
}

#[test]
fn test_create_new_entity_type_uses_role_defaults() {
    let _s = setup_ownership();

    if let Err(err) = security::init_admin_user_if_not_exists() {
        panic!("admin user creation failed: {err}");
    }

    if let Err(err) = security::create_basic_user(&BasicUser {
        username: b"carol".to_vec(),
        password: b"pw".to_vec(),
        role: Role::user(),
    }) {
        panic!("user creation failed: {err}");
    }

    let mut ctx = new_user_ctx("carol", "POST", "/api/fresh", r#"{"title":"carol"}"#);
    ctx.set_param("entity", "fresh");
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403 for regular user, got {}", status(&ctx));
    }

    let mut ctx = new_user_ctx(
        security::DEFAULT_ADMIN_USERNAME,
        "POST",
        "/api/fresh",
        r#"{"title":"admin"}"#,
    );
    ctx.set_param("entity", "fresh");
    api_controller::create_controller(&mut ctx);

    if status(&ctx) != 200 {
        panic!("expected 200 for admin, got {}", status(&ctx));
    }
}

fn get_note_as(username: &str, id: &str) -> RequestCtx {
    let mut ctx = new_user_ctx(username, "GET", &format!("/api/notes/{id}"), "");
    ctx.set_param("entity", "notes");
    ctx.set_param("id", id);
    api_controller::get_by_id_controller(&mut ctx);

    ctx
}

#[test]
fn test_get_by_id_cache_hit_checks_read_acl() {
    let _s = setup_ownership();
    update_config(|cfg| cfg.api.cache.enabled = true);

    let id = create_note_as("alice", r#"{"title":"alice"}"#);

    for want in ["MISS", "HIT"] {
        let ctx = get_note_as("alice", &id);
        let got = String::from_utf8_lossy(ctx.response.header.peek("X-Elysian-Cache")).into_owned();
        if status(&ctx) != 200 || got != want {
            panic!("expected 200 {want} for owner, got {} {got}", status(&ctx));
        }
    }

    let ctx = get_note_as("bob", &id);
    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}: {}", status(&ctx), body(&ctx));
    }
}

#[test]
fn test_get_by_id_filtered_response_is_not_cached() {
    let _s = setup();
    update_config(|cfg| cfg.api.cache.enabled = true);

    create("books", r#"{"id":"b1","title":"Dune","author":"Herbert"}"#);

    let mut ctx = new_ctx("GET", "/api/books/b1?fields=title", "");
    ctx.set_param("entity", "books");
    ctx.set_param("id", "b1");
    api_controller::get_by_id_controller(&mut ctx);

    let mut ctx = new_ctx("GET", "/api/books/b1", "");
    ctx.set_param("entity", "books");
    ctx.set_param("id", "b1");
    api_controller::get_by_id_controller(&mut ctx);

    let obj = object(ctx.response.body_bytes());
    if text_field(&obj, "author").as_deref() != Some("Herbert") {
        panic!("expected full document, got {}", body(&ctx));
    }
}

#[test]
fn test_query_payload_hash_depends_on_count_only_fields_and_sort_order() {
    let base = api_controller::QueryPayload {
        entity: "book".to_string(),
        ..api_controller::QueryPayload::default()
    };
    let count = api_controller::QueryPayload {
        entity: "book".to_string(),
        count_only: true,
        ..api_controller::QueryPayload::default()
    };
    let fields = api_controller::QueryPayload {
        entity: "book".to_string(),
        fields: "title".to_string(),
        ..api_controller::QueryPayload::default()
    };

    let mut hashes: Vec<String> = Vec::new();
    for p in [&base, &count, &fields] {
        let h = hex_of(&p.hash());
        if !hashes.contains(&h) {
            hashes.push(h);
        }
    }

    if hashes.len() != 3 {
        panic!("expected countOnly and fields to change the cache key");
    }

    let year_first = match api_controller::decode_query_payload(
        br#"{"entity":"flm","sorts":{"year":"desc","title":"asc"}}"#,
    ) {
        Ok(p) => p,
        Err(err) => panic!("{err}"),
    };
    let title_first = match api_controller::decode_query_payload(
        br#"{"entity":"flm","sorts":{"title":"asc","year":"desc"}}"#,
    ) {
        Ok(p) => p,
        Err(err) => panic!("{err}"),
    };

    if year_first.sort_order.len() != 2
        || year_first.sort_order[0] != "year"
        || title_first.sort_order[0] != "title"
    {
        panic!(
            "expected the sort keys to keep the request order, got {:?} and {:?}",
            year_first.sort_order, title_first.sort_order
        );
    }

    if year_first
        .sorts
        .as_ref()
        .and_then(|s| s.get("year"))
        .map(String::as_str)
        != Some("desc")
        || year_first.entity != "flm"
    {
        panic!("expected the payload to be decoded, got {year_first:?}");
    }

    if hex_of(&year_first.hash()) == hex_of(&title_first.hash()) {
        panic!("expected the sort order to change the cache key");
    }

    match api_controller::decode_query_payload(br#"{"entity":"flm","sorts":null}"#) {
        Ok(no_sorts) if no_sorts.sort_order.is_empty() => {}
        other => panic!("expected null sorts to be accepted, got {other:?}"),
    }

    if api_controller::decode_query_payload(br#"{"entity":"flm","sorts":{"year":1}}"#).is_ok() {
        panic!("expected an invalid sort direction type to be rejected");
    }
}

fn leaf_value(node: &elysiandb::query::FilterNode, field: &str, op: &str) -> String {
    node.leaf
        .as_ref()
        .and_then(|leaf| leaf.get(field.as_bytes()))
        .and_then(|ops| ops.get(op.as_bytes()))
        .map(|v| String::from_utf8_lossy(v).into_owned())
        .unwrap_or_default()
}

#[test]
fn test_parse_filter_node_typed_values() {
    let raw = doc(r#"{"year":{"eq":1983.0,"lt":2000.5},"color":{"eq":true},"title":{"eq":"C"}}"#);
    let node = match api_controller::parse_filter_node(Some(&raw)) {
        Ok(node) => node,
        Err(err) => panic!("{err}"),
    };

    let want = [
        ("year", "eq", "1983"),
        ("year", "lt", "2000.5"),
        ("color", "eq", "true"),
        ("title", "eq", "C"),
    ];
    for (field, op, value) in want {
        let got = leaf_value(&node, field, op);
        if got != value {
            panic!("{field}.{op}: expected {value:?}, got {got:?}");
        }
    }

    for raw in [
        r#"{"year":{"eq":null}}"#,
        r#"{"year":{"eq":[1.0]}}"#,
        r#"{"year":{"eq":{}}}"#,
    ] {
        if api_controller::parse_filter_node(Some(&doc(raw))).is_ok() {
            panic!("expected {raw} to be rejected");
        }
    }
}

#[test]
fn test_query_controller_returns_json_errors() {
    let _s = setup();

    for body in [
        "{bad",
        r#"{"entity":"film","filters":{"year":{"eq":null}}}"#,
    ] {
        let ctx = query(body);

        let mut out: Option<BTreeMap<String, String>> = None;
        let decoded = json::unmarshal(ctx.response.body_bytes(), &mut out);
        let message = out
            .as_ref()
            .and_then(|o| o.get("error"))
            .cloned()
            .unwrap_or_default();
        if status(&ctx) != STATUS_BAD_REQUEST || decoded.is_err() || message.is_empty() {
            panic!(
                "{body}: expected a 400 with an error message, got {} {}",
                status(&ctx),
                self::body(&ctx)
            );
        }
    }
}

#[test]
fn test_query_controller_rejects_core_entities() {
    let _s = setup();

    let ctx = query(r#"{"entity":"_elysiandb_core_user"}"#);

    if status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected 403, got {}", status(&ctx));
    }
}

#[test]
fn test_reject_core_entity() {
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let flag = called.clone();
    let h = api_controller::reject_core_entity(Arc::new(move |_ctx: &mut RequestCtx| {
        flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }));

    let mut ctx = new_ctx("GET", "/api/_elysiandb_core_acl", "");
    ctx.set_param("entity", "_elysiandb_core_acl");
    h(&mut ctx);

    if called.load(std::sync::atomic::Ordering::SeqCst) || status(&ctx) != STATUS_FORBIDDEN {
        panic!("expected core entities to be rejected");
    }

    let mut ctx = new_ctx("GET", "/api/books", "");
    ctx.set_param("entity", "books");
    h(&mut ctx);

    if !called.load(std::sync::atomic::Ordering::SeqCst) {
        panic!("expected public entities to pass");
    }
}
