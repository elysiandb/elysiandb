use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use crate::tempdir::TempDir;
use elysiandb::acl::{self, Permission};
use elysiandb::api::{self, ByteFields};
use elysiandb::configuration::Config;
use elysiandb::crypto::{bcrypt, sha256};
use elysiandb::globals;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{self, Map, Value};
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::security::{self, BasicUser, LoginSource, Role};
use elysiandb::storage;

const R: &[u8] = b"\xff";
const S: &[u8] = b"\xfe";
const F: &[u8] = "\u{fffd}".as_bytes();
const RAW_ENTITY: &[u8] = b"e\xfc";

struct Store {
    dir: TempDir,
}

impl Store {
    fn folder(&self) -> String {
        self.dir.path().to_string_lossy().into_owned()
    }
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn set_mode(mode: &str) {
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.security.authentication.enabled = !mode.is_empty();
    cfg.security.authentication.mode = mode.to_string();
    globals::set_config(Some(Arc::new(cfg)));
}

fn set_folder(folder: &str) {
    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.store.folder = folder.to_string();
    globals::set_config(Some(Arc::new(cfg)));
}

fn open_store() -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.security.authentication.enabled = true;
    cfg.security.authentication.mode = "user".to_string();
    cfg.api.schema.enabled = true;
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();
    security::reset_login_attempts();
    security::init_admin_user_if_not_exists().expect("admin");
    Store { dir }
}

fn put_go_document(entity: &[u8], id: &[u8], bytes: &[u8]) {
    let mut doc = Map::new();
    json::unmarshal(bytes, &mut doc).expect("go document");
    let engine = storage::engine().expect("engine");
    engine
        .update(|tx| {
            tx.put_bytes(entity, id, bytes.to_vec(), Some(doc.clone()))?;
            tx.ensure_type(entity);
            Ok(())
        })
        .expect("put");
}

fn seed_go_user(id: &[u8], password: &str, role: &str) {
    let key = security::create_key_file_or_get_key().expect("key");
    let mut input = password.as_bytes().to_vec();
    input.extend_from_slice(&key);
    let hash = bcrypt::generate_from_password(&sha256::sum256(&input), bcrypt::DEFAULT_COST)
        .expect("hash");
    let hash = String::from_utf8(hash).expect("ascii hash");
    let bytes =
        format!(r#"{{"id":"\ufffd","password":"{hash}","role":"{role}","username":"\ufffd"}}"#);
    put_go_document(security::USER_ENTITY.as_bytes(), id, bytes.as_bytes());
}

fn stored(entity: &str, id: &[u8]) -> Option<Vec<u8>> {
    api::read_entity_bytes_by_id(entity.as_bytes(), id).map(|b| b.to_vec())
}

fn user_keys() -> Vec<Vec<u8>> {
    let mut out = Vec::new();
    let engine = storage::engine().expect("engine");
    engine
        .snapshot(security::USER_ENTITY.as_bytes())
        .ascend(0, |doc| {
            out.push(doc.id().to_vec());
            true
        });
    out.sort();
    out
}

fn password_of(id: &[u8]) -> Option<Value> {
    api::read_entity_by_id(security::USER_ENTITY.as_bytes(), id)
        .and_then(|mut m| m.remove("password"))
}

fn local() -> LoginSource {
    LoginSource {
        ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
        local: true,
    }
}

fn basic_ctx(username: &[u8], password: &[u8]) -> RequestCtx {
    let mut credentials = username.to_vec();
    credentials.push(b':');
    credentials.extend_from_slice(password);
    let mut req = Request::new();
    req.header.set(
        "Authorization",
        format!("Basic {}", STD_ENCODING.encode_to_string(&credentials)),
    );
    let mut ctx = RequestCtx::new();
    ctx.init(
        &req,
        Some(SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), 1)),
    );
    ctx
}

fn cookie_ctx(id: &str) -> RequestCtx {
    let mut req = Request::new();
    req.header.set_cookie(security::SESSION_COOKIE_NAME, id);
    let mut ctx = RequestCtx::new();
    ctx.init(&req, None);
    ctx
}

fn auth(username: &[u8], password: &str) -> bool {
    security::authenticate_user(username, password.as_bytes()).is_some()
}

#[test]
fn invalid_utf8_usernames_behave_as_in_go() {
    let store = open_store();
    seed_go_user(R, "password-1", "user");
    seed_go_user(S, "password-2", "admin");

    assert!(security::basic_user_exists(R));
    assert!(security::basic_user_exists(S));
    assert!(!security::basic_user_exists(F));

    let mut names: Vec<String> = security::list_basic_users()
        .expect("list")
        .iter()
        .filter_map(|u| {
            u.get("username")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .collect();
    names.sort();
    assert_eq!(names, ["admin", "\u{fffd}", "\u{fffd}"]);

    let got = security::get_basic_user_by_username(R).expect("get");
    assert_eq!(
        json::marshal(&got).expect("marshal"),
        "{\"id\":\"\u{fffd}\",\"role\":\"user\",\"username\":\"\u{fffd}\"}".as_bytes()
    );

    assert!(auth(R, "password-1"));
    assert!(!auth(F, "password-1"));
    assert!(auth(S, "password-2"));
    assert!(!auth(F, "password-2"));
    assert!(security::validate_username(R).is_ok());

    set_mode("basic");
    assert!(security::check_basic_authentication(&basic_ctx(
        R,
        b"password-1"
    )));
    assert!(!security::check_basic_authentication(&basic_ctx(
        F,
        b"password-1"
    )));
    assert!(security::authenticate_credentials(
        b"\xff password-1",
        local()
    ));
    assert!(!security::authenticate_credentials(
        "\u{fffd} password-1".as_bytes(),
        local()
    ));
    set_mode("user");
    assert!(security::authenticate_credentials(
        b"\xfe password-2",
        local()
    ));
    assert!(!security::authenticate_credentials(
        b"\xff password-1",
        local()
    ));

    let session =
        security::create_session(R, Role::user(), Duration::from_secs(3600)).expect("session");
    assert_eq!(session.username.as_bytes(), R);
    let file = std::fs::read(Path::new(&store.folder()).join(security::SESSIONS_FILENAME))
        .expect("sessions file");
    let expected = format!(
        "{{\"sessions\":[{{\"token_hash\":\"{}\",\"username\":\"\\ufffd\",\"role\":\"user\",\"credentials\":\"{}\",\"expires_at\":{}}}]}}\n",
        session.token_hash, session.credentials, session.expires_at
    );
    assert_eq!(String::from_utf8_lossy(&file), expected);

    let found = security::get_session(session.id.as_bytes())
        .expect("get session")
        .expect("found");
    assert_eq!(found.username.as_bytes(), R);
    assert_eq!(found.role, Role::user());

    let ctx = cookie_ctx(&session.id);
    assert!(matches!(
        security::current_user_can_manage_user(&ctx, R),
        Ok(false)
    ));
    assert!(matches!(
        security::current_user_can_manage_user(&ctx, F),
        Ok(true)
    ));

    let err = security::create_session(F, Role::user(), Duration::from_secs(3600))
        .expect_err("no user under the replacement character");
    assert_eq!(err.message(), "user '\u{fffd}' not found".as_bytes());

    let other = TempDir::new();
    set_folder(&other.path().to_string_lossy());
    assert_eq!(security::active_session_count(), 0);
    set_folder(&store.folder());
    assert!(
        security::get_session(session.id.as_bytes())
            .expect("reload")
            .is_none()
    );

    security::change_user_password(R, b"password-9").expect("password");
    assert_eq!(
        user_keys(),
        [b"admin".to_vec(), F.to_vec(), S.to_vec(), R.to_vec()]
    );
    assert!(auth(R, "password-1"));
    assert!(!auth(R, "password-9"));
    assert!(auth(F, "password-9"));
    assert!(!auth(F, "password-1"));
    assert!(
        stored(security::USER_ENTITY, F)
            .expect("replacement document")
            .starts_with("{\"id\":\"\u{fffd}\",\"password\":\"$2a$10$".as_bytes())
    );

    security::change_user_role(S, b"user").expect("role");
    assert_eq!(password_of(F), password_of(S));
    assert_eq!(
        api::read_entity_by_id(security::USER_ENTITY.as_bytes(), F)
            .and_then(|m| m.get("role").and_then(Value::as_str).map(str::to_string)),
        Some("user".to_string())
    );

    security::delete_basic_user(S).expect("delete");
    assert_eq!(user_keys(), [b"admin".to_vec(), F.to_vec(), R.to_vec()]);

    api::write_entity(
        RAW_ENTITY,
        &mut Map::from([("id".to_string(), Value::String("x".to_string()))]),
    );
    put_go_document(
        acl::ACL_ENTITY.as_bytes(),
        b"admin::e\xfc",
        br#"{"entity":"e\ufffd","id":"admin::e\ufffd","permissions":{"create":true,"delete":true,"owning_delete":true,"owning_read":true,"owning_update":true,"owning_write":true,"read":true,"update":true},"username":"admin"}"#,
    );
    let raw = acl::get_acl_entity_for_username(RAW_ENTITY, b"admin").expect("raw acl");
    assert_eq!(raw.entity, "e\u{fffd}".as_bytes());
    assert!(acl::can_create_entity(b"admin", RAW_ENTITY));

    let mut read_only = acl::new_permissions();
    read_only[Permission::Read as usize] = true;
    acl::update_acl_entity_for_username(RAW_ENTITY, b"admin", read_only).expect("update");
    let raw = acl::get_acl_entity_for_username(RAW_ENTITY, b"admin").expect("raw acl");
    assert!(raw.can(Permission::Read) && raw.can(Permission::Create));
    let replaced = acl::get_acl_entity_for_username("e\u{fffd}".as_bytes(), b"admin")
        .expect("acl under the replacement character");
    assert!(replaced.can(Permission::Read) && !replaced.can(Permission::Create));

    acl::reset_acl_entity_to_default(RAW_ENTITY, b"admin").expect("reset");
    let replaced = acl::get_acl_entity_for_username("e\u{fffd}".as_bytes(), b"admin")
        .expect("acl under the replacement character");
    assert!(replaced.can(Permission::Create));

    let err = acl::reset_acl_entity_to_default(b"doc", R).expect_err("raw user");
    assert_eq!(err.message(), b"user \xff not found");
}

const ESCAPED: &str = "\\ufffd";
const ADMIN_PERMISSIONS: &str = r#"{"create":true,"delete":true,"owning_delete":true,"owning_read":true,"owning_update":true,"owning_write":true,"read":true,"update":true}"#;
const USER_PERMISSIONS: &str = r#"{"create":false,"delete":false,"owning_delete":true,"owning_read":true,"owning_update":true,"owning_write":true,"read":false,"update":false}"#;

fn acl_documents() -> Vec<(Vec<u8>, String)> {
    let mut out = Vec::new();
    let engine = storage::engine().expect("engine");
    engine
        .snapshot(acl::ACL_ENTITY.as_bytes())
        .ascend(0, |doc| {
            let bytes = doc.bytes().expect("bytes");
            out.push((
                doc.id().to_vec(),
                String::from_utf8(bytes.to_vec()).expect("utf8 document"),
            ));
            true
        });
    out.sort();
    out
}

fn acl_document(id: &[u8], entity: &str, username: &str, permissions: &str) -> (Vec<u8>, String) {
    (
        id.to_vec(),
        format!(
            r#"{{"entity":"{entity}","id":"{username}::{entity}","permissions":{permissions},"username":"{username}"}}"#
        ),
    )
}

#[test]
fn invalid_utf8_names_are_written_as_in_go() {
    let _store = open_store();
    for (name, password) in [(R, "password-1"), (S, "password-2")] {
        security::create_basic_user(&BasicUser {
            username: name.to_vec(),
            password: password.as_bytes().to_vec(),
            role: Role::user(),
        })
        .expect("create");
    }

    assert_eq!(user_keys(), [b"admin".to_vec(), S.to_vec(), R.to_vec()]);
    for name in [R, S] {
        let text = String::from_utf8(stored(security::USER_ENTITY, name).expect("raw document"))
            .expect("utf8 document");
        assert!(text.starts_with(&format!(r#"{{"id":"{ESCAPED}","password":"$2a$10$"#)));
        assert!(text.ends_with(&format!(r#"","role":"user","username":"{ESCAPED}"}}"#)));
    }
    assert!(auth(R, "password-1"));
    assert!(!auth(F, "password-1"));

    let mut d1 = Map::from([("id".to_string(), Value::String("d1".to_string()))]);
    assert!(api::write_entity(b"doc", &mut d1).is_empty());
    assert!(
        api::write_entity_bytes(RAW_ENTITY, &mut Map::new(), &ByteFields::id(b"x\xfb")).is_empty()
    );
    assert_eq!(stored("e\u{fffd}", b"x\xfb"), None);
    assert_eq!(
        api::read_entity_bytes_by_id(RAW_ENTITY, b"x\xfb").map(|b| b.to_vec()),
        Some(format!(r#"{{"id":"x{ESCAPED}"}}"#).into_bytes())
    );

    acl::init_acl().expect("init acl");
    let raw_entity = format!("e{ESCAPED}");
    let expected = [
        acl_document(b"admin::doc", "doc", "admin", ADMIN_PERMISSIONS),
        acl_document(b"admin::e\xfc", &raw_entity, "admin", ADMIN_PERMISSIONS),
        acl_document(
            "\u{fffd}::doc".as_bytes(),
            "doc",
            "\u{fffd}",
            USER_PERMISSIONS,
        ),
        acl_document(
            b"\xef\xbf\xbd::e\xfc",
            &raw_entity,
            "\u{fffd}",
            USER_PERMISSIONS,
        ),
    ];
    assert_eq!(acl_documents(), expected);
    assert!(acl::get_acl_entity_for_username(RAW_ENTITY, b"admin").is_some());
    assert!(acl::can_create_entity(b"admin", RAW_ENTITY));

    acl::init_acl().expect("init acl again");
    assert_eq!(acl_documents(), expected);
}
