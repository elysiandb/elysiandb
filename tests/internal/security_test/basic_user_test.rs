use std::path::Path;

use elysiandb::api;
use elysiandb::gocompat::base64::STD_ENCODING;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::httpd::{Request, RequestCtx};
use elysiandb::security::{self, BasicHashedUser, Role};

use crate::{basic_user, ctx_from, setup};

const USER_ENTITY: &[u8] = security::USER_ENTITY.as_bytes();

fn str_of<'a>(m: &'a Map, key: &str) -> Option<&'a str> {
    m.get(key).and_then(Value::as_str)
}

fn basic_ctx(header: &str) -> RequestCtx {
    let mut req = Request::new();
    req.header.set("Authorization", header);
    ctx_from(&req, None)
}

fn data_map(pairs: &[(&str, &str)]) -> Map {
    pairs
        .iter()
        .map(|(k, v)| (k.to_string(), Value::String(v.to_string())))
        .collect()
}

#[test]
fn generate_key() {
    let key = security::generate_key().expect("generate key");
    assert!(!key.is_empty(), "empty key");
}

#[test]
fn create_key_file_or_get_key_create() {
    let store = setup();

    let key = security::create_key_file_or_get_key().expect("key");
    assert!(!key.is_empty(), "empty key");

    let path = Path::new(&store.path()).join(security::KEY_FILENAME);
    assert!(path.exists(), "expected key file to exist");
}

#[test]
fn create_key_file_or_get_key_read_same_key() {
    let _store = setup();

    let k1 = security::create_key_file_or_get_key().expect("k1");
    let k2 = security::create_key_file_or_get_key().expect("k2");
    assert_eq!(k1, k2, "expected same key, got {k1} and {k2}");
}

#[test]
fn user_entity_schema() {
    let s = security::user_entity_schema();

    for field in ["id", "username", "password", "role"] {
        let v = s
            .get(field)
            .unwrap_or_else(|| panic!("missing field {field} in schema"));
        let m = v
            .as_object()
            .unwrap_or_else(|| panic!("field {field} is not a map"));
        assert_eq!(str_of(m, "type"), Some("string"), "type mismatch");
        assert_eq!(
            m.get("required").and_then(Value::as_bool),
            Some(true),
            "required mismatch"
        );
    }
}

#[test]
fn init_basic_users_storage_creates_entity_type_and_schema() {
    let _store = setup();

    assert!(
        !api::entity_type_exists(USER_ENTITY),
        "should not exist before init"
    );

    security::init_basic_users_storage().expect("init");

    assert!(api::entity_type_exists(USER_ENTITY), "entity not created");

    assert!(
        api::get_entity_schema(USER_ENTITY).is_some(),
        "schema not created"
    );
}

#[test]
fn init_basic_users_storage_idempotent() {
    let _store = setup();
    security::init_basic_users_storage().expect("first init");
    security::init_basic_users_storage().expect("second init");
}

#[test]
fn basic_hashed_user_to_data_map() {
    let _store = setup();

    let u = BasicHashedUser {
        username: b"bob".to_vec(),
        password: "hash".to_string(),
        role: Role::admin(),
    };

    let m = u.to_data_map();
    assert_eq!(str_of(&m, "id"), Some("bob"), "id mismatch");
    assert_eq!(str_of(&m, "username"), Some("bob"), "username mismatch");
    assert_eq!(str_of(&m, "password"), Some("hash"), "password mismatch");
    assert_eq!(
        str_of(&m, "role"),
        Some(security::ROLE_ADMIN),
        "role mismatch"
    );
}

#[test]
fn basic_hashed_user_save() {
    let _store = setup();

    let u = BasicHashedUser {
        username: b"saveuser".to_vec(),
        password: "hash".to_string(),
        role: Role::user(),
    };

    u.save().expect("save");

    assert!(
        api::read_entity_by_id(USER_ENTITY, b"saveuser").is_some(),
        "not saved"
    );
}

#[test]
fn basic_user_to_hased_user() {
    let _store = setup();

    let u = basic_user("test", "pass", Role::admin());

    let hu = u.to_hased_user().expect("hash");

    assert_eq!(hu.username, u.username, "mismatch");
    assert!(!hu.password.is_empty(), "empty hash");
    assert_eq!(hu.role, u.role, "role mismatch");
}

#[test]
fn basic_user_to_hased_user_default_role() {
    let _store = setup();

    let u = basic_user("noRole", "x", Role::default());

    let hu = u.to_hased_user().expect("hash");

    assert_eq!(hu.role, Role::user(), "expected default role user");
}

#[test]
fn create_basic_user_single() {
    let _store = setup();

    let u = basic_user("alice", "secret", Role::admin());

    security::create_basic_user(&u).expect("create");

    assert!(
        api::read_entity_by_id(USER_ENTITY, b"alice").is_some(),
        "not written"
    );
}

#[test]
fn create_basic_user_multiple() {
    let _store = setup();

    let users = [
        basic_user("u1", "p1", Role::user()),
        basic_user("u2", "p2", Role::admin()),
        basic_user("u3", "p3", Role::user()),
    ];

    for u in &users {
        security::create_basic_user(u).expect("create");
    }

    for u in &users {
        assert!(
            api::read_entity_by_id(USER_ENTITY, &u.username).is_some(),
            "missing"
        );
    }
}

#[test]
fn get_basic_user_by_username() {
    let _store = setup();

    let u = basic_user("bob", "pwd", Role::user());
    security::create_basic_user(&u).expect("create");

    let out = security::get_basic_user_by_username(b"bob").expect("get");

    assert_eq!(str_of(&out, "username"), Some("bob"), "username mismatch");
    assert!(
        !out.contains_key("password"),
        "password should not be exposed"
    );
}

#[test]
fn get_basic_user_by_username_not_found() {
    let _store = setup();

    assert!(
        security::get_basic_user_by_username(b"missing").is_err(),
        "expected error"
    );
}

#[test]
fn list_basic_users() {
    let _store = setup();

    let users = [
        basic_user("a", "1", Role::user()),
        basic_user("b", "2", Role::admin()),
    ];

    for u in &users {
        security::create_basic_user(u).expect("create");
    }

    let list = security::list_basic_users().expect("list");

    assert_eq!(list.len(), 2, "expected 2 users, got {}", list.len());

    for u in &list {
        assert!(
            !u.contains_key("password"),
            "password should not be exposed"
        );
    }
}

#[test]
fn init_admin_user_if_not_exists_creates_default_admin() {
    let _store = setup();

    security::init_admin_user_if_not_exists().expect("init admin");

    assert!(
        api::read_entity_by_id(USER_ENTITY, security::DEFAULT_ADMIN_USERNAME.as_bytes()).is_some(),
        "admin not created"
    );
}

#[test]
fn init_admin_user_if_not_exists_does_not_overwrite_existing() {
    let _store = setup();

    let u = basic_user(security::DEFAULT_ADMIN_USERNAME, "custom", Role::admin());
    security::create_basic_user(&u).expect("create");

    security::init_admin_user_if_not_exists().expect("init admin");

    let out = api::read_entity_by_id(USER_ENTITY, security::DEFAULT_ADMIN_USERNAME.as_bytes())
        .unwrap_or_default();
    assert_eq!(
        str_of(&out, "username"),
        Some(security::DEFAULT_ADMIN_USERNAME),
        "username mismatch"
    );
}

#[test]
fn change_user_password_success() {
    let _store = setup();

    let u = basic_user("john", "oldpwd", Role::user());
    security::create_basic_user(&u).expect("create");

    security::change_user_password(b"john", b"newpwd").expect("change");

    assert!(
        security::authenticate_user(b"john", b"newpwd").is_some(),
        "expected success"
    );
}

#[test]
fn delete_basic_user_does_not_delete_admin() {
    let _store = setup();

    let u = basic_user(security::DEFAULT_ADMIN_USERNAME, "x", Role::admin());

    security::create_basic_user(&u).expect("create");

    let _ = security::delete_basic_user(security::DEFAULT_ADMIN_USERNAME.as_bytes());

    assert!(
        api::read_entity_by_id(USER_ENTITY, security::DEFAULT_ADMIN_USERNAME.as_bytes()).is_some(),
        "admin should not be deleted"
    );
}

#[test]
fn check_basic_authentication_success() {
    let _store = setup();

    let u = basic_user("john", "secret", Role::user());
    security::create_basic_user(&u).expect("create");

    let header = format!("Basic {}", STD_ENCODING.encode_to_string(b"john:secret"));

    let ctx = basic_ctx(&header);

    assert!(
        security::check_basic_authentication(&ctx),
        "expected success"
    );
}

#[test]
fn init_basic_users_storage_schema_matches_user_entity_schema() {
    let _store = setup();

    security::init_basic_users_storage().expect("init");

    let expected = api::update_entity_schema(USER_ENTITY, &security::user_entity_schema());
    let stored = api::get_entity_schema(USER_ENTITY).expect("missing schema");

    assert!(expected == stored, "schema mismatch");
}

#[test]
fn change_user_role_success() {
    let _store = setup();

    let u = basic_user("roleuser", "pwd", Role::user());

    security::create_basic_user(&u).expect("create");

    security::change_user_role(b"roleuser", security::ROLE_ADMIN.as_bytes()).expect("change");

    let out = api::read_entity_by_id(USER_ENTITY, b"roleuser").expect("user not found");

    assert_eq!(
        str_of(&out, "role"),
        Some(security::ROLE_ADMIN),
        "expected role admin, got {:?}",
        out.get("role")
    );
}

#[test]
fn change_user_role_user_not_found() {
    let _store = setup();

    assert!(
        security::change_user_role(b"missing", security::ROLE_ADMIN.as_bytes()).is_err(),
        "expected error"
    );
}

#[test]
fn change_user_role_preserves_password() {
    let _store = setup();

    let u = basic_user("keepPwd", "secret", Role::user());

    security::create_basic_user(&u).expect("create");

    let before = api::read_entity_by_id(USER_ENTITY, b"keepPwd").expect("missing user");

    security::change_user_role(b"keepPwd", security::ROLE_ADMIN.as_bytes()).expect("change");

    let after = api::read_entity_by_id(USER_ENTITY, b"keepPwd").expect("missing user after update");

    assert!(
        before.get("password") == after.get("password"),
        "password should not change"
    );
}

#[test]
fn change_user_password_user_not_found() {
    let _store = setup();

    assert!(
        security::change_user_password(b"ghost", b"x").is_err(),
        "expected error"
    );
}

#[test]
fn authenticate_user_failure() {
    let _store = setup();

    let u = basic_user("john", "secret", Role::user());
    security::create_basic_user(&u).expect("create");

    assert!(
        security::authenticate_user(b"john", b"bad").is_none(),
        "expected failure"
    );
}

#[test]
fn authenticate_user_user_not_found() {
    let _store = setup();

    assert!(
        security::authenticate_user(b"ghost", b"x").is_none(),
        "expected failure"
    );
}

#[test]
fn delete_basic_user_deletes_regular_user() {
    let _store = setup();

    let u = basic_user("todelete", "x", Role::user());
    security::create_basic_user(&u).expect("create");

    let _ = security::delete_basic_user(b"todelete");

    assert!(
        api::read_entity_by_id(USER_ENTITY, b"todelete").is_none(),
        "user should be deleted"
    );
}

#[test]
fn check_basic_authentication_missing_header() {
    let _store = setup();

    let ctx = RequestCtx::new();
    assert!(
        !security::check_basic_authentication(&ctx),
        "expected failure"
    );
}

#[test]
fn check_basic_authentication_invalid_prefix() {
    let _store = setup();

    let ctx = basic_ctx("Bearer abc");

    assert!(
        !security::check_basic_authentication(&ctx),
        "expected failure"
    );
}

#[test]
fn check_basic_authentication_invalid_base64() {
    let _store = setup();

    let ctx = basic_ctx("Basic !!!");

    assert!(
        !security::check_basic_authentication(&ctx),
        "expected failure"
    );
}

#[test]
fn check_basic_authentication_invalid_payload() {
    let _store = setup();

    let header = format!("Basic {}", STD_ENCODING.encode_to_string(b"badpayload"));
    let ctx = basic_ctx(&header);

    assert!(
        !security::check_basic_authentication(&ctx),
        "expected failure"
    );
}

#[test]
fn basic_hashed_user_from_data_map_invalid_username() {
    let err = BasicHashedUser::from_data_map(&data_map(&[("password", "x"), ("role", "user")]));
    assert!(err.is_err(), "expected error");
}

#[test]
fn basic_hashed_user_from_data_map_invalid_password() {
    let err = BasicHashedUser::from_data_map(&data_map(&[("username", "x"), ("role", "user")]));
    assert!(err.is_err(), "expected error");
}

#[test]
fn basic_hashed_user_from_data_map_invalid_role() {
    let err = BasicHashedUser::from_data_map(&data_map(&[("username", "x"), ("password", "y")]));
    assert!(err.is_err(), "expected error");
}
