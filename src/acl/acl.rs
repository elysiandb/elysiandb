use crate::engine;
use crate::globals::CORE_FIELDS_PREFIX;
use crate::gocompat::json::{Map, Value};
use crate::security::{self, Role};

use super::init::{
    Acl, Permission, USERNAME_FIELD, default_permissions_for_role, get_acl_entity_for_username,
    init_acl,
};

pub fn can_create_entity(username: &[u8], entity: &[u8]) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    if let Some(acl) = get_acl_entity_for_username(entity, username) {
        return acl.can(Permission::Create);
    }

    if entity.starts_with(CORE_FIELDS_PREFIX.as_bytes()) {
        return false;
    }

    if !engine::entity_type_exists(entity) {
        return role_can_create(username);
    }

    if init_acl().is_err() {
        return false;
    }

    if let Some(acl) = get_acl_entity_for_username(entity, username) {
        return acl.can(Permission::Create);
    }

    false
}

fn role_can_create(username: &[u8]) -> bool {
    let Ok(user) = security::get_basic_user_by_username(username) else {
        return false;
    };

    let role = match user.get("role") {
        Some(Value::String(role)) => role.as_str(),
        _ => "",
    };

    default_permissions_for_role(&Role::new(role))[Permission::Create as usize]
}

fn string_of<'a>(data: &'a Map, key: &str) -> Option<&'a str> {
    match data.get(key) {
        Some(Value::String(s)) => Some(s),
        _ => None,
    }
}

fn owner_of(data: &Map) -> Option<&str> {
    string_of(data, USERNAME_FIELD).filter(|owner| !owner.is_empty())
}

pub fn can_write_entity(username: &[u8], entity: &[u8], data: &Map) -> bool {
    can_write_list_of_entities(username, entity, std::slice::from_ref(data))
}

pub fn can_write_list_of_entities(username: &[u8], entity: &[u8], list: &[Map]) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    if !can_create_entity(username, entity) {
        return false;
    }

    for item in list {
        let id = match string_of(item, "id") {
            Some(id) if !id.is_empty() && engine::entity_exists(entity, id.as_bytes()) => id,
            _ => continue,
        };

        if !can_update_entity(username, entity, id.as_bytes(), item) {
            return false;
        }
    }

    true
}

pub fn can_delete_entity(username: &[u8], entity: &[u8], data: &Map) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    let Some(acl) = get_acl_entity_for_username(entity, username) else {
        return false;
    };

    if acl.can(Permission::Delete) {
        return true;
    }

    let Some(owner) = owner_of(data) else {
        return false;
    };

    acl.can(Permission::OwningDelete) && owner.as_bytes() == username
}

pub fn can_update_entity(username: &[u8], entity: &[u8], id: &[u8], updates: &Map) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    let Some(acl) = get_acl_entity_for_username(entity, username) else {
        return false;
    };

    if acl.can(Permission::Update) {
        return true;
    }

    can_owning_update(&acl, entity, id, username, updates)
}

pub fn can_update_list_of_entities(username: &[u8], entity: &[u8], updates: &[Map]) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    let Some(acl) = get_acl_entity_for_username(entity, username) else {
        return false;
    };

    if acl.can(Permission::Update) {
        return true;
    }

    for item in updates {
        let Some(id) = string_of(item, "id").filter(|id| !id.is_empty()) else {
            return false;
        };

        if !can_owning_update(&acl, entity, id.as_bytes(), username, item) {
            return false;
        }
    }

    true
}

fn can_owning_update(acl: &Acl, entity: &[u8], id: &[u8], username: &[u8], updates: &Map) -> bool {
    if !acl.can(Permission::OwningUpdate) {
        return false;
    }

    let existing = engine::read_entity_by_id(entity, id).unwrap_or_default();
    match owner_of(&existing) {
        Some(owner) if owner.as_bytes() == username => {}
        _ => return false,
    }

    match updates.get(USERNAME_FIELD) {
        None => true,
        Some(Value::String(new_owner)) => new_owner.as_bytes() == username,
        Some(_) => false,
    }
}

pub fn can_read_entity(username: &[u8], entity: &[u8], data: &Map) -> bool {
    if !security::user_authentication_is_enabled() {
        return true;
    }

    let Some(acl) = get_acl_entity_for_username(entity, username) else {
        return false;
    };

    if acl.can(Permission::Read) {
        return true;
    }

    let Some(owner) = owner_of(data) else {
        return false;
    };

    acl.can(Permission::OwningRead) && owner.as_bytes() == username
}

pub fn filter_list_of_entities(username: &[u8], entity: &[u8], data: Vec<Map>) -> Vec<Map> {
    if !security::user_authentication_is_enabled() {
        return data;
    }

    let Some(acl) = get_acl_entity_for_username(entity, username) else {
        return Vec::new();
    };

    if acl.can(Permission::Read) {
        return data;
    }

    let owning_read = acl.can(Permission::OwningRead);
    data.into_iter()
        .filter(|item| owning_read && owner_of(item).is_some_and(|o| o.as_bytes() == username))
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReadMode {
    ReadAll,
    ReadNone,
    ReadOwned,
}

pub fn read_mode_for(username: &[u8], entity: &[u8]) -> ReadMode {
    if !security::user_authentication_is_enabled() {
        return ReadMode::ReadAll;
    }

    let Some(acl) = get_acl_entity_for_username(entity, username) else {
        return ReadMode::ReadNone;
    };

    if acl.can(Permission::Read) {
        return ReadMode::ReadAll;
    }

    if acl.can(Permission::OwningRead) {
        return ReadMode::ReadOwned;
    }

    ReadMode::ReadNone
}

pub fn owned_by(data: &Map, username: &[u8]) -> bool {
    owner_of(data).is_some_and(|owner| owner.as_bytes() == username)
}
