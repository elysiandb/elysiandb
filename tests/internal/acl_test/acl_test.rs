use std::sync::Arc;
use std::sync::atomic::{AtomicI32, Ordering};

use elysiandb::acl::{self, Acl, Permission, Permissions};
use elysiandb::api;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::json::{Map, Value};
use elysiandb::schema;
use elysiandb::security::{self, Role};
use elysiandb::storage;

use crate::tempdir::TempDir;

struct Store {
    _dir: TempDir,
}

impl Drop for Store {
    fn drop(&mut self) {
        let _ = storage::close();
    }
}

fn setup(auth: bool) -> Store {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    cfg.store.shards = 2;
    cfg.security.authentication.enabled = auth;
    cfg.security.authentication.mode = "user".to_string();
    globals::set_config(Some(Arc::new(cfg)));

    storage::load_db();
    storage::load_json_db();
    api::delete_all();

    Store { _dir: dir }
}

fn object(pairs: Vec<(&str, Value)>) -> Map {
    pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect()
}

fn s(v: &str) -> Value {
    Value::String(v.to_string())
}

fn write_acl(username: &str, entity: &str, permissions: Permissions) {
    let a = Acl {
        username: username.as_bytes().to_vec(),
        entity: entity.as_bytes().to_vec(),
        permissions,
    };
    api::write_entity(acl::ACL_ENTITY.as_bytes(), &mut a.to_data_map());
}

fn write_doc(entity: &str, id: &str, owner: &str) {
    api::write_entity(
        entity.as_bytes(),
        &mut object(vec![("id", s(id)), (acl::USERNAME_FIELD, s(owner))]),
    );
}

fn write_user(username: &str, role: Role) {
    api::write_entity(
        security::USER_ENTITY.as_bytes(),
        &mut object(vec![
            ("id", s(username)),
            ("username", s(username)),
            ("role", s(role.as_str())),
        ]),
    );
}

fn perms(granted: &[Permission]) -> Permissions {
    let mut p = acl::new_permissions();
    for g in granted {
        p[*g as usize] = true;
    }
    p
}

fn owned(owner: &str) -> Map {
    object(vec![(acl::USERNAME_FIELD, s(owner))])
}

fn with_a() -> Map {
    object(vec![("a", Value::Int(1))])
}

fn with_id(id: &str) -> Map {
    object(vec![("id", s(id))])
}

#[test]
fn can_create_entity_default_deny() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[]));

    assert!(!acl::can_create_entity(b"u", b"doc"), "expected false");
}

#[test]
fn can_create_entity_with_permission() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Create]));

    assert!(acl::can_create_entity(b"u", b"doc"), "expected true");
}

#[test]
fn can_create_entity_new_entity_type_uses_role_defaults() {
    let _store = setup(true);
    write_user("boss", Role::admin());
    write_user("u", Role::user());

    assert!(
        acl::can_create_entity(b"boss", b"fresh"),
        "expected admin to create a new entity type"
    );

    assert!(
        !acl::can_create_entity(b"u", b"fresh"),
        "expected regular user to be denied"
    );

    assert!(
        !acl::can_create_entity(b"ghost", b"fresh"),
        "expected unknown user to be denied"
    );
}

#[test]
fn can_create_entity_existing_entity_type_without_acl_gets_the_default_acls() {
    let _store = setup(true);
    write_user("boss", Role::admin());
    write_user("u", Role::user());
    write_doc("doc", "1", "x");

    assert!(
        acl::can_create_entity(b"boss", b"doc"),
        "expected admin to create once the default ACLs are written"
    );

    assert!(
        !acl::can_create_entity(b"u", b"doc"),
        "expected regular user to be denied"
    );

    assert!(
        acl::get_acl_entity_for_username(b"doc", b"boss").is_some()
            && acl::get_acl_entity_for_username(b"doc", b"u").is_some(),
        "expected the default ACLs of the entity type to be written"
    );
}

#[test]
fn can_create_entity_concurrent_first_writes_of_a_new_entity_type() {
    let _store = setup(true);
    write_user("boss", Role::admin());

    for round in 0..50 {
        let entity = format!("fresh_{round}");
        let denied = Arc::new(AtomicI32::new(0));

        let handles: Vec<_> = (0..20)
            .map(|i| {
                let entity = entity.clone();
                let denied = denied.clone();
                std::thread::spawn(move || {
                    if !acl::can_create_entity(b"boss", entity.as_bytes()) {
                        denied.fetch_add(1, Ordering::SeqCst);
                        return;
                    }
                    write_doc(&entity, &i.to_string(), "boss");
                    if i % 5 == 0 {
                        let _ = acl::init_acl();
                    }
                })
            })
            .collect();

        for h in handles {
            h.join().expect("join");
        }

        let n = denied.load(Ordering::SeqCst);
        assert_eq!(
            n, 0,
            "round {round}: {n} creations denied on a new entity type"
        );
    }
}

#[test]
fn can_create_entity_core_entity_type() {
    let _store = setup(true);
    write_user("boss", Role::admin());

    assert!(
        !acl::can_create_entity(b"boss", acl::ACL_ENTITY.as_bytes()),
        "expected false for an existing core entity"
    );

    assert!(
        !acl::can_create_entity(b"boss", b"_elysiandb_core_fresh"),
        "expected false for a new core entity"
    );
}

#[test]
fn can_write_entity_requires_create() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));

    assert!(
        !acl::can_write_entity(b"u", b"doc", &with_a()),
        "expected false"
    );
}

#[test]
fn can_write_entity_new_document() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Create]));

    assert!(
        acl::can_write_entity(b"u", b"doc", &with_a()),
        "expected true without id"
    );

    assert!(
        acl::can_write_entity(b"u", b"doc", &with_id("missing")),
        "expected true for an unused id"
    );
}

#[test]
fn can_write_entity_existing_id_requires_ownership() {
    let _store = setup(true);

    write_acl(
        "u",
        "doc",
        perms(&[Permission::Create, Permission::OwningUpdate]),
    );
    write_doc("doc", "mine", "u");
    write_doc("doc", "other", "x");

    assert!(
        acl::can_write_entity(b"u", b"doc", &with_id("mine")),
        "expected true for own document"
    );

    assert!(
        !acl::can_write_entity(b"u", b"doc", &with_id("other")),
        "expected false for another user's document"
    );
}

#[test]
fn can_write_entity_existing_id_with_global_update() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Create, Permission::Update]));
    write_doc("doc", "other", "x");

    assert!(
        acl::can_write_entity(b"u", b"doc", &with_id("other")),
        "expected true"
    );
}

#[test]
fn can_write_list_of_entities_existing_ids() {
    let _store = setup(true);

    write_acl(
        "u",
        "doc",
        perms(&[Permission::Create, Permission::OwningUpdate]),
    );
    write_doc("doc", "mine", "u");
    write_doc("doc", "other", "x");

    let ok = acl::can_write_list_of_entities(b"u", b"doc", &[with_a(), with_id("mine")]);
    assert!(ok, "expected true");

    let ok = acl::can_write_list_of_entities(b"u", b"doc", &[with_a(), with_id("other")]);
    assert!(!ok, "expected false");
}

#[test]
fn can_read_entity_global() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Read]));

    assert!(
        acl::can_read_entity(b"u", b"doc", &owned("x")),
        "expected true"
    );
}

#[test]
fn can_read_entity_owning() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningRead]));

    assert!(
        acl::can_read_entity(b"u", b"doc", &owned("u")),
        "expected true"
    );

    assert!(
        !acl::can_read_entity(b"u", b"doc", &owned("x")),
        "expected false"
    );
}

#[test]
fn can_update_entity_global() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Update]));
    write_doc("doc", "1", "x");

    assert!(
        acl::can_update_entity(b"u", b"doc", b"1", &with_a()),
        "expected true"
    );

    assert!(
        acl::can_update_entity(b"u", b"doc", b"1", &owned("u")),
        "expected true"
    );
}

#[test]
fn can_update_entity_owning() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));
    write_doc("doc", "mine", "u");
    write_doc("doc", "other", "x");

    assert!(
        acl::can_update_entity(b"u", b"doc", b"mine", &with_a()),
        "expected true"
    );

    assert!(
        acl::can_update_entity(b"u", b"doc", b"mine", &owned("u")),
        "expected true"
    );

    assert!(
        !acl::can_update_entity(b"u", b"doc", b"other", &with_a()),
        "expected false"
    );
}

#[test]
fn can_update_entity_owning_uses_stored_owner() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));
    write_doc("doc", "other", "x");

    assert!(
        !acl::can_update_entity(b"u", b"doc", b"other", &owned("u")),
        "expected false"
    );
}

#[test]
fn can_update_entity_owning_cannot_change_owner() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));
    write_doc("doc", "mine", "u");

    assert!(
        !acl::can_update_entity(b"u", b"doc", b"mine", &owned("x")),
        "expected false"
    );

    assert!(
        !acl::can_update_entity(
            b"u",
            b"doc",
            b"mine",
            &object(vec![(acl::USERNAME_FIELD, Value::Null)])
        ),
        "expected false"
    );
}

#[test]
fn can_update_entity_owning_missing_entity() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));

    assert!(
        !acl::can_update_entity(b"u", b"doc", b"missing", &owned("u")),
        "expected false"
    );
}

#[test]
fn can_delete_entity_global() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Delete]));

    assert!(
        acl::can_delete_entity(b"u", b"doc", &owned("x")),
        "expected true"
    );
}

#[test]
fn can_delete_entity_owning() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningDelete]));

    assert!(
        acl::can_delete_entity(b"u", b"doc", &owned("u")),
        "expected true"
    );

    assert!(
        !acl::can_delete_entity(b"u", b"doc", &owned("x")),
        "expected false"
    );
}

#[test]
fn can_update_list_of_entities_global() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Update]));
    write_doc("doc", "1", "x");
    write_doc("doc", "2", "y");

    let ok = acl::can_update_list_of_entities(
        b"u",
        b"doc",
        &[
            object(vec![("id", s("1")), ("a", Value::Int(1))]),
            object(vec![("id", s("2")), ("a", Value::Int(2))]),
        ],
    );
    assert!(ok, "expected true");
}

#[test]
fn can_update_list_of_entities_owning() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));
    write_doc("doc", "1", "u");
    write_doc("doc", "2", "u");
    write_doc("doc", "3", "x");

    let ok = acl::can_update_list_of_entities(
        b"u",
        b"doc",
        &[
            object(vec![("id", s("1")), ("a", Value::Int(1))]),
            object(vec![("id", s("2")), (acl::USERNAME_FIELD, s("u"))]),
        ],
    );
    assert!(ok, "expected true");

    let ok = acl::can_update_list_of_entities(
        b"u",
        b"doc",
        &[
            object(vec![("id", s("1")), ("a", Value::Int(1))]),
            object(vec![("id", s("3")), ("a", Value::Int(1))]),
        ],
    );
    assert!(!ok, "expected false");
}

#[test]
fn can_update_list_of_entities_owning_uses_stored_owner() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));
    write_doc("doc", "1", "u");
    write_doc("doc", "2", "x");

    let ok = acl::can_update_list_of_entities(
        b"u",
        b"doc",
        &[
            object(vec![("id", s("1")), (acl::USERNAME_FIELD, s("u"))]),
            object(vec![("id", s("2")), (acl::USERNAME_FIELD, s("u"))]),
        ],
    );
    assert!(!ok, "expected false");
}

#[test]
fn can_update_list_of_entities_owning_cannot_change_owner() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));
    write_doc("doc", "1", "u");

    let ok = acl::can_update_list_of_entities(
        b"u",
        b"doc",
        &[object(vec![("id", s("1")), (acl::USERNAME_FIELD, s("x"))])],
    );
    assert!(!ok, "expected false");
}

#[test]
fn can_update_list_of_entities_owning_requires_id() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningUpdate]));

    let ok = acl::can_update_list_of_entities(b"u", b"doc", &[owned("u")]);
    assert!(!ok, "expected false");
}

#[test]
fn filter_list_of_entities_global_read() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::Read]));

    let out = acl::filter_list_of_entities(b"u", b"doc", vec![owned("u"), owned("x")]);
    assert_eq!(out.len(), 2, "expected 2");
}

#[test]
fn filter_list_of_entities_owning() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[Permission::OwningRead]));

    let out = acl::filter_list_of_entities(b"u", b"doc", vec![owned("u"), owned("x")]);
    assert_eq!(out.len(), 1, "expected 1");
}

#[test]
fn filter_list_of_entities_no_permission() {
    let _store = setup(true);

    write_acl("u", "doc", perms(&[]));

    let out = acl::filter_list_of_entities(b"u", b"doc", vec![owned("u")]);
    assert!(out.is_empty(), "expected empty");
}

#[test]
fn all_allowed_when_auth_disabled() {
    let _store = setup(false);

    assert!(acl::can_create_entity(b"u", b"x"), "expected true");
    assert!(
        acl::can_read_entity(b"u", b"x", &Map::new()),
        "expected true"
    );
    assert!(
        acl::can_update_entity(b"u", b"x", b"1", &Map::new()),
        "expected true"
    );
    assert!(
        acl::can_delete_entity(b"u", b"x", &Map::new()),
        "expected true"
    );
    assert!(
        acl::can_write_entity(b"u", b"x", &with_id("1")),
        "expected true"
    );

    let out = acl::filter_list_of_entities(b"u", b"x", vec![with_a()]);
    assert_eq!(out.len(), 1, "expected passthrough");
}

#[test]
fn init_acl_replaces_an_inferred_schema() {
    let _store = setup(true);

    let mut cfg = globals::config().map(|c| (*c).clone()).unwrap_or_default();
    cfg.api.schema.enabled = true;
    cfg.api.schema.strict = true;
    globals::set_config(Some(Arc::new(cfg)));

    write_user("admin", Role::admin());
    let permissions_field = object(vec![
        ("name", s("permissions")),
        ("type", s("map[string]bool")),
        ("required", Value::Bool(true)),
    ]);
    api::write_entity(
        schema::SCHEMA_ENTITY.as_bytes(),
        &mut object(vec![
            ("id", s(acl::ACL_ENTITY)),
            (
                "fields",
                Value::Object(object(vec![(
                    "permissions",
                    Value::Object(permissions_field),
                )])),
            ),
        ]),
    );

    let result = acl::init_acl();
    assert!(
        result.is_ok(),
        "expected the ACLs to be created, got {result:?}"
    );

    assert!(
        schema::is_manual_schema(acl::ACL_ENTITY.as_bytes(), None),
        "expected the ACL entity to get its manual schema"
    );

    api::write_entity(b"book", &mut with_id("b1"));
    let result = acl::init_acl();
    assert!(
        result.is_ok(),
        "expected the ACLs of a new entity type to be created, got {result:?}"
    );

    assert!(
        acl::get_acl_entity_for_username(b"book", b"admin").is_some(),
        "expected an ACL for the new entity type"
    );

    let result = acl::update_acl_entity_for_username(b"book", b"admin", acl::new_permissions());
    assert!(
        result.is_ok(),
        "expected the ACL update to succeed, got {result:?}"
    );
}
