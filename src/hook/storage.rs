use crate::docpath;
use crate::engine;
use crate::gocompat::json;
use crate::uuid;

use super::default::{
    get_default_hook_script_js_for_post_read, get_default_hook_script_js_for_pre_read,
};
use super::hook::{
    HOOK_ENTITY, HOOK_EVENT_POST_READ, HOOK_EVENT_PRE_READ, Hook, HookError, hooks_enabled,
};
use super::registry::{hooks_for_entity, hooks_for_event, prepare_indexes};

pub fn get_post_read_hooks_for_entity(entity: &str) -> Vec<Hook> {
    hooks_for_event(entity, HOOK_EVENT_POST_READ)
        .iter()
        .map(|h| (**h).clone())
        .collect()
}

pub fn get_pre_read_hooks_for_entity(entity: &str) -> Vec<Hook> {
    hooks_for_event(entity, HOOK_EVENT_PRE_READ)
        .iter()
        .map(|h| (**h).clone())
        .collect()
}

pub fn create_hook(hook: &mut Hook) -> Result<(), HookError> {
    if hook.id.is_empty() {
        hook.id = uuid::new_v4();
    }

    if engine::entity_exists(HOOK_ENTITY.as_bytes(), hook.id.as_bytes()) {
        return Err(HookError::new(format!(
            "the hook '{}' already exists",
            hook.name
        )));
    }

    if hook.script.is_empty() {
        hook.script = default_script_for_event(&hook.event);
    }

    let mut data = hook.to_data_map();
    let errs = engine::write_entity(HOOK_ENTITY.as_bytes(), &mut data);
    if !errs.is_empty() {
        let list = json::marshal(&errs).unwrap_or_default();
        return Err(HookError::new(format!(
            "an error occurred while creating the hook: {}",
            String::from_utf8_lossy(&list)
        )));
    }

    prepare_indexes();

    Ok(())
}

pub fn update_hook(hook: &mut Hook) -> Result<(), HookError> {
    if hook.script.is_empty() {
        hook.script = default_script_for_event(&hook.event);
    }

    hook.validate()?;

    if engine::update_entity_by_id(
        HOOK_ENTITY.as_bytes(),
        hook.id.as_bytes(),
        &hook.to_data_map(),
    )
    .is_none()
    {
        return Err(HookError::new(format!(
            "the hook '{}' does not exist",
            hook.name
        )));
    }

    prepare_indexes();

    Ok(())
}

fn default_script_for_event(event: &str) -> String {
    match event {
        HOOK_EVENT_PRE_READ => get_default_hook_script_js_for_pre_read(),
        HOOK_EVENT_POST_READ => get_default_hook_script_js_for_post_read(),
        _ => String::new(),
    }
}

pub fn delete_hook(hook_id: &str) -> Result<(), HookError> {
    if !engine::entity_exists(HOOK_ENTITY.as_bytes(), hook_id.as_bytes()) {
        return Err(HookError::new(format!(
            "the hook with id '{hook_id}' does not exist"
        )));
    }

    engine::delete_entity_by_id(HOOK_ENTITY.as_bytes(), hook_id.as_bytes());
    prepare_indexes();

    Ok(())
}

pub fn get_hook_by_id(hook_id: &str) -> Result<Hook, HookError> {
    if !engine::entity_exists(HOOK_ENTITY.as_bytes(), hook_id.as_bytes()) {
        return Err(HookError::new(format!(
            "the hook with id '{hook_id}' does not exist"
        )));
    }

    let Some(data) = engine::read_entity_by_id(HOOK_ENTITY.as_bytes(), hook_id.as_bytes()) else {
        return Err(HookError::new(format!(
            "an error occurred while retrieving the hook with id '{hook_id}'"
        )));
    };

    let mut hook = Hook::default();
    hook.from_data_map(&data);

    Ok(hook)
}

pub fn entity_has_hooks(entity: &str) -> bool {
    if !hooks_enabled() {
        return false;
    }

    !hooks_for_entity(entity).is_empty()
}

fn entity_has_hooks_for(entity: &str, event: &str) -> bool {
    hooks_for_entity(entity)
        .iter()
        .any(|h| docpath::match_glob(event.as_bytes(), h.event.as_bytes()))
}

pub fn entity_has_post_read_hooks(entity: &str) -> bool {
    entity_has_hooks_for(entity, HOOK_EVENT_POST_READ)
}

pub fn entity_has_pre_read_hooks(entity: &str) -> bool {
    entity_has_hooks_for(entity, HOOK_EVENT_PRE_READ)
}

pub fn get_hooks_for_entity(entity: &str) -> Result<Vec<Hook>, HookError> {
    Ok(hooks_for_entity(entity)
        .iter()
        .map(|h| (**h).clone())
        .collect())
}
