mod create_hook;
mod delete_hook_by_id;
mod get_hook_by_id;
mod get_hooks;
mod update_hook_by_id;

pub use create_hook::create_hook_for_entity_controller;
pub use delete_hook_by_id::delete_hook_by_id_controller;
pub use get_hook_by_id::get_hook_by_id_controller;
pub use get_hooks::get_hooks_for_entity_controller;
pub use update_hook_by_id::update_hook_by_id_controller;

use crate::hook::Hook;
use crate::json_struct;

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq, Eq)]
    pub struct HookRecord {
        pub id: String => "id",
        pub name: String => "name",
        pub script: String => "script",
        pub entity: String => "entity",
        pub event: String => "event",
        pub language: String => "language",
        pub priority: i64 => "priority",
        pub bypass_acl: bool => "bypass_acl",
        pub enabled: bool => "enabled",
    }
}

impl From<&Hook> for HookRecord {
    fn from(h: &Hook) -> HookRecord {
        HookRecord {
            id: h.id.clone(),
            name: h.name.clone(),
            script: h.script.clone(),
            entity: h.entity.clone(),
            event: h.event.clone(),
            language: h.language.clone(),
            priority: h.priority,
            bypass_acl: h.bypass_acl,
            enabled: h.enabled,
        }
    }
}

impl From<HookRecord> for Hook {
    fn from(r: HookRecord) -> Hook {
        Hook {
            id: r.id,
            name: r.name,
            script: r.script,
            entity: r.entity,
            event: r.event,
            language: r.language,
            priority: r.priority,
            bypass_acl: r.bypass_acl,
            enabled: r.enabled,
        }
    }
}

const FORBIDDEN: &str = r#"{"error":"forbidden"}"#;
const INTERNAL_ERROR: &str = r#"{"error":"internal server error"}"#;
const INVALID_BODY: &str = r#"{"error":"invalid request body"}"#;
const NOT_FOUND: &str = r#"{"error":"hook not found"}"#;
