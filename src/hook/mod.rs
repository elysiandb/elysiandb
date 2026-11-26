mod default;
mod executor;
mod foldeval;
mod foldplan;
#[allow(clippy::module_inception)]
mod hook;
mod memo;
mod natives;
mod prepare;
mod registry;
mod results;
mod rows;
mod storage;
mod view;

pub use default::{
    get_default_hook_script_js_for_post_read, get_default_hook_script_js_for_pre_read,
};
pub use executor::{Request, apply_post_read_script, apply_pre_read_script};
pub use foldplan::{PlanInfo, plan_info};
pub use hook::{
    HOOK_ENTITY, HOOK_EVENT_POST_READ, HOOK_EVENT_PRE_READ, Hook, HookError, PHASE_POST_READ,
    PHASE_PRE_READ, apply_post_read_hooks_for_entity, apply_pre_read_hooks_for_entity,
    create_hook_entity, hook_entity_schema, init_hooks,
};
pub use memo::{OutputMemoStats, output_memo, reset_output_memo};
pub use registry::prepare_indexes;
pub use results::{QueryCacheStats, query_cache, reset_query_cache};
pub use storage::{
    create_hook, delete_hook, entity_has_hooks, entity_has_post_read_hooks,
    entity_has_pre_read_hooks, get_hook_by_id, get_hooks_for_entity,
    get_post_read_hooks_for_entity, get_pre_read_hooks_for_entity, update_hook,
};
pub use view::{FoldStats, folds, set_folds_enabled, views_ready};

pub fn document_from_json(doc: &crate::gocompat::json::Map) -> talos::host::MapRef {
    match &talos::host::json::from_json(&crate::gocompat::json::Value::Object(doc.clone())) {
        talos::host::DocValue::Map(m) => m.clone(),
        _ => talos::host::MapRef::default(),
    }
}
