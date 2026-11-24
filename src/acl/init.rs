use std::fmt;
use std::os::unix::ffi::OsStrExt;

use crate::engine;
use crate::gocompat::json::{self, Encode, EncodeState, Map, Value};
use crate::kv;
use crate::log;
use crate::schema::{self, SCHEMA_ENTITY};
use crate::security::{self, Role, USER_ENTITY, text};
use crate::storage;
use crate::sync::Mutex;

pub const USERNAME_FIELD: &str = "_elysiandb_core_username";
pub const ACL_ENTITY: &str = "_elysiandb_core_acl";

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Permission {
    Create,
    Read,
    Update,
    Delete,
    OwningRead,
    OwningWrite,
    OwningDelete,
    OwningUpdate,
}

impl Permission {
    pub const ALL: [Permission; 8] = [
        Permission::Create,
        Permission::Read,
        Permission::Update,
        Permission::Delete,
        Permission::OwningRead,
        Permission::OwningWrite,
        Permission::OwningDelete,
        Permission::OwningUpdate,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Permission::Create => "create",
            Permission::Read => "read",
            Permission::Update => "update",
            Permission::Delete => "delete",
            Permission::OwningRead => "owning_read",
            Permission::OwningWrite => "owning_write",
            Permission::OwningDelete => "owning_delete",
            Permission::OwningUpdate => "owning_update",
        }
    }
}

pub type Permissions = [bool; 8];

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AclError(pub Vec<u8>);

impl AclError {
    pub fn message(&self) -> &[u8] {
        &self.0
    }
}

impl fmt::Display for AclError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&text(&self.0))
    }
}

fn concat(parts: &[&[u8]]) -> Vec<u8> {
    parts.concat()
}

impl std::error::Error for AclError {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Acl {
    pub username: Vec<u8>,
    pub entity: Vec<u8>,
    pub permissions: Permissions,
}

pub fn new_permissions() -> Permissions {
    [false; 8]
}

pub fn default_permissions_for_role(role: &Role) -> Permissions {
    let mut p = new_permissions();

    if role.is_admin() {
        return [true; 8];
    }

    p[Permission::OwningRead as usize] = true;
    p[Permission::OwningWrite as usize] = true;
    p[Permission::OwningDelete as usize] = true;
    p[Permission::OwningUpdate as usize] = true;

    p
}

impl Acl {
    pub fn to_data_map(&self) -> Map {
        let mut perms = Map::new();
        for p in Permission::ALL {
            perms.insert(
                p.name().to_string(),
                Value::Bool(self.permissions[p as usize]),
            );
        }

        let id = get_acl_entity_id(&self.username, &self.entity);

        let mut m = Map::new();
        m.insert("id".to_string(), Value::String(text(&id)));
        m.insert("username".to_string(), Value::String(text(&self.username)));
        m.insert("entity".to_string(), Value::String(text(&self.entity)));
        m.insert("permissions".to_string(), Value::Object(perms));
        m
    }

    pub fn can(&self, p: Permission) -> bool {
        self.permissions[p as usize]
    }

    pub fn grant(&mut self, p: Permission) {
        self.permissions[p as usize] = true;
    }

    pub fn revoke(&mut self, p: Permission) {
        self.permissions[p as usize] = false;
    }
}

impl Encode for Acl {
    fn encode(&self, e: &mut EncodeState) -> Result<(), json::Error> {
        let mut first = true;
        e.write_field_name(&mut first, "username");
        e.write_string(&self.username);
        e.write_field_name(&mut first, "entity");
        e.write_string(&self.entity);
        e.write_field_name(&mut first, "permissions");
        e.write_byte(b'{');
        for (i, allowed) in self.permissions.iter().enumerate() {
            if i > 0 {
                e.write_byte(b',');
            }
            e.write_string(i.to_string().as_bytes());
            e.write_byte(b':');
            e.write_bool(*allowed);
        }
        e.write_byte(b'}');
        e.end_struct(first);
        Ok(())
    }
}

fn field_schema(kind: &str) -> Value {
    let mut m = Map::new();
    m.insert("type".to_string(), Value::String(kind.to_string()));
    m.insert("required".to_string(), Value::Bool(true));
    Value::Object(m)
}

pub fn acl_entity_schema() -> Map {
    let mut permissions = Map::new();
    for p in Permission::ALL {
        permissions.insert(p.name().to_string(), field_schema("boolean"));
    }

    let mut permissions_field = Map::new();
    permissions_field.insert("type".to_string(), Value::String("object".to_string()));
    permissions_field.insert("required".to_string(), Value::Bool(true));
    permissions_field.insert("fields".to_string(), Value::Object(permissions));

    let mut m = Map::new();
    m.insert("username".to_string(), field_schema("string"));
    m.insert("entity".to_string(), field_schema("string"));
    m.insert("permissions".to_string(), Value::Object(permissions_field));
    m
}

pub fn init_acl_storage() {
    if !engine::entity_type_exists(ACL_ENTITY.as_bytes()) {
        engine::add_entity_type(ACL_ENTITY.as_bytes());
    }

    let current = engine::get_entity_schema(ACL_ENTITY.as_bytes());
    if !schema::is_manual_schema(ACL_ENTITY.as_bytes(), current.as_ref()) {
        engine::update_entity_schema(ACL_ENTITY.as_bytes(), &acl_entity_schema());
    }
}

static LAST_SIGNATURE: Mutex<Option<Vec<u8>>> = Mutex::new(None);

fn acl_signature() -> Vec<u8> {
    let Some(e) = storage::engine() else {
        return Vec::new();
    };

    let version = |entity: &[u8]| {
        let snap = e.snapshot(entity);
        format!("{}.{}", snap.generation(), snap.version())
    };

    let mut sig = e.folder().as_os_str().as_bytes().to_vec();
    for entity in [USER_ENTITY, ACL_ENTITY, SCHEMA_ENTITY] {
        sig.push(b'|');
        sig.extend_from_slice(version(entity.as_bytes()).as_bytes());
    }
    sig.push(b'|');
    for (i, t) in e.types().iter().enumerate() {
        if i > 0 {
            sig.push(0);
        }
        sig.extend_from_slice(t);
    }

    sig
}

pub fn init_acl() -> Result<(), AclError> {
    if !security::user_authentication_is_enabled() {
        return Ok(());
    }

    let sig = acl_signature();
    if !sig.is_empty() && LAST_SIGNATURE.lock().as_deref() == Some(sig.as_slice()) {
        return Ok(());
    }

    init_acl_storage();

    if let Err(err) = save_acl_list(&generate_acl_fo_allr_entities()) {
        log::error(
            "cannot create the default ACLs",
            kv!["component" => "acl", "err" => err.message()],
        );
        return Err(err);
    }

    *LAST_SIGNATURE.lock() = Some(sig);

    Ok(())
}

fn save_acl_list(acls: &[Acl]) -> Result<(), AclError> {
    let mut errs = Vec::new();
    for acl in acls {
        let id = get_acl_entity_id(&acl.username, &acl.entity);
        if engine::entity_exists(ACL_ENTITY.as_bytes(), &id) {
            continue;
        }

        if let Err(err) = write_acl(acl) {
            errs.push(err.0);
        }
    }

    if errs.is_empty() {
        return Ok(());
    }

    Err(AclError(errs.join(&b'\n')))
}

fn write_acl(a: &Acl) -> Result<(), AclError> {
    let validation_errors = engine::write_entity(ACL_ENTITY.as_bytes(), &mut a.to_data_map());
    if validation_errors.is_empty() {
        return Ok(());
    }

    let messages: Vec<String> = validation_errors
        .iter()
        .map(|e| e.to_error().to_string())
        .collect();

    Err(AclError(concat(&[
        b"unable to write the ACL of ",
        &a.username,
        b" on ",
        &a.entity,
        b": ",
        messages.join("; ").as_bytes(),
    ])))
}

fn string_of<'a>(data: &'a Map, key: &str) -> Option<&'a str> {
    match data.get(key) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

pub fn generate_acl_fo_allr_entities() -> Vec<Acl> {
    let mut acls = Vec::new();

    let Ok(users) = security::list_basic_users() else {
        return acls;
    };

    let entities = engine::list_public_entity_types();

    for user in &users {
        let (Some(username), Some(role)) = (string_of(user, "username"), string_of(user, "role"))
        else {
            continue;
        };

        let role = Role::new(role);

        for entity in &entities {
            acls.push(Acl {
                username: username.as_bytes().to_vec(),
                entity: entity.clone(),
                permissions: default_permissions_for_role(&role),
            });
        }
    }

    acls
}

pub fn get_acl_entity_id(username: &[u8], entity: &[u8]) -> Vec<u8> {
    let mut id = Vec::with_capacity(username.len() + 2 + entity.len());
    id.extend_from_slice(username);
    id.extend_from_slice(b"::");
    id.extend_from_slice(entity);
    id
}

pub fn get_acl_entity_for_username(entity: &[u8], username: &[u8]) -> Option<Acl> {
    let data =
        engine::read_entity_by_id(ACL_ENTITY.as_bytes(), &get_acl_entity_id(username, entity))?;

    let mut acl = Acl {
        username: string_of(&data, "username")
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
        entity: string_of(&data, "entity")
            .unwrap_or_default()
            .as_bytes()
            .to_vec(),
        permissions: new_permissions(),
    };

    if let Some(Value::Object(perms)) = data.get("permissions") {
        for (k, v) in perms {
            if let (Some(p), Value::Bool(allowed)) = (string_to_permission(k), v) {
                acl.permissions[p as usize] = *allowed;
            }
        }
    }

    Some(acl)
}

pub fn update_acl_entity_for_username(
    entity: &[u8],
    username: &[u8],
    permissions: Permissions,
) -> Result<(), AclError> {
    let Some(mut existing) = get_acl_entity_for_username(entity, username) else {
        return Err(AclError(concat(&[
            b"ACL does not exist for username ",
            username,
            b" and entity ",
            entity,
        ])));
    };

    existing.permissions = permissions;

    write_acl(&existing)
}

pub fn string_to_permission(s: &str) -> Option<Permission> {
    Permission::ALL.into_iter().find(|p| p.name() == s)
}

pub fn delete_user_acls(username: &[u8]) -> Result<(), AclError> {
    for entity in engine::list_public_entity_types() {
        let id = get_acl_entity_id(username, &entity);
        if engine::entity_exists(ACL_ENTITY.as_bytes(), &id) {
            engine::delete_entity_by_id(ACL_ENTITY.as_bytes(), &id);
        }
    }

    Ok(())
}

pub fn delete_acl_for_entity_type(entity: &[u8]) -> Result<(), AclError> {
    let users = security::list_basic_users().map_err(|e| AclError(e.message()))?;

    for user in &users {
        let Some(username) = string_of(user, "username") else {
            continue;
        };

        let id = get_acl_entity_id(username.as_bytes(), entity);
        if engine::entity_exists(ACL_ENTITY.as_bytes(), &id) {
            engine::delete_entity_by_id(ACL_ENTITY.as_bytes(), &id);
        }
    }

    Ok(())
}

pub fn reset_acl_entity_to_default(entity: &[u8], username: &[u8]) -> Result<(), AclError> {
    let users = security::list_basic_users().map_err(|e| AclError(e.message()))?;

    let role = users.iter().find_map(|user| {
        match (string_of(user, "username"), string_of(user, "role")) {
            (Some(u), Some(r)) if u.as_bytes() == username => Some(Role::new(r)),
            _ => None,
        }
    });

    let Some(role) = role else {
        return Err(AclError(concat(&[b"user ", username, b" not found"])));
    };

    let permissions = default_permissions_for_role(&role);

    let existing = match get_acl_entity_for_username(entity, username) {
        Some(mut existing) => {
            existing.permissions = permissions;
            existing
        }
        None => Acl {
            username: username.to_vec(),
            entity: entity.to_vec(),
            permissions,
        },
    };

    write_acl(&existing)
}
