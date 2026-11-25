use std::fmt;
use std::sync::Arc;
use std::time::{Duration, Instant};

use goja::host::MapRef;

use crate::api::GuardError;
use crate::engine;
use crate::globals;
use crate::gocompat::json::{Map, Value};
use crate::log::{self, Level, Logger, Throttle};
use crate::metrics;

use super::executor::Request;
use super::registry::hooks_for_event;
use super::registry::prepare_indexes;

pub const HOOK_ENTITY: &str = "_elysiandb_core_hook";

pub const HOOK_EVENT_POST_READ: &str = "post_read";
pub const HOOK_EVENT_PRE_READ: &str = "pre_read";

pub const PHASE_PRE_READ: &str = "pre_read";
pub const PHASE_POST_READ: &str = "post_read";

pub fn hook_entity_schema() -> Map {
    let field = |name: &str, required: bool, typ: &str| {
        let mut m = Map::new();
        m.insert("name".to_string(), Value::String(name.to_string()));
        m.insert("required".to_string(), Value::Bool(required));
        m.insert("type".to_string(), Value::String(typ.to_string()));
        (name.to_string(), Value::Object(m))
    };

    Map::from([
        field("name", true, "string"),
        field("entity", true, "string"),
        field("script", true, "string"),
        field("event", true, "string"),
        field("language", true, "string"),
        field("priority", false, "number"),
        field("bypass_acl", false, "boolean"),
        field("enabled", false, "boolean"),
    ])
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Hook {
    pub id: String,
    pub name: String,
    pub script: String,
    pub entity: String,
    pub event: String,
    pub language: String,
    pub priority: i64,
    pub bypass_acl: bool,
    pub enabled: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HookError {
    message: String,
    guard: Option<GuardError>,
}

impl HookError {
    pub fn new(message: impl Into<String>) -> HookError {
        HookError {
            message: message.into(),
            guard: None,
        }
    }

    pub fn with_guard(message: impl Into<String>, guard: Option<GuardError>) -> HookError {
        HookError {
            message: message.into(),
            guard,
        }
    }

    pub fn message(&self) -> &str {
        &self.message
    }

    pub fn guard(&self) -> Option<GuardError> {
        self.guard
    }

    pub fn is(&self, err: GuardError) -> bool {
        self.guard == Some(err)
    }
}

impl From<GuardError> for HookError {
    fn from(err: GuardError) -> HookError {
        HookError {
            message: err.to_string(),
            guard: Some(err),
        }
    }
}

impl fmt::Display for HookError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for HookError {}

impl Hook {
    pub fn to_data_map(&self) -> Map {
        Map::from([
            ("id".to_string(), Value::String(self.id.clone())),
            ("name".to_string(), Value::String(self.name.clone())),
            ("script".to_string(), Value::String(self.script.clone())),
            ("entity".to_string(), Value::String(self.entity.clone())),
            ("event".to_string(), Value::String(self.event.clone())),
            ("language".to_string(), Value::String(self.language.clone())),
            ("priority".to_string(), Value::Int(self.priority)),
            ("bypass_acl".to_string(), Value::Bool(self.bypass_acl)),
            ("enabled".to_string(), Value::Bool(self.enabled)),
        ])
    }

    pub fn validate(&self) -> Result<(), HookError> {
        let required = [
            ("name", &self.name),
            ("entity", &self.entity),
            ("event", &self.event),
            ("language", &self.language),
            ("script", &self.script),
        ];

        for (name, value) in required {
            if value.is_empty() {
                return Err(HookError::new(format!(
                    "the hook field '{name}' is required"
                )));
            }
        }

        Ok(())
    }

    pub fn from_data_map(&mut self, data: &Map) {
        let text = |key: &str| match data.get(key) {
            Some(Value::String(s)) => Some(s.clone()),
            _ => None,
        };

        if let Some(v) = text("id") {
            self.id = v;
        }

        if let Some(v) = text("name") {
            self.name = v;
        }

        if let Some(v) = text("script") {
            self.script = v;
        }

        if let Some(v) = text("entity") {
            self.entity = v;
        }

        if let Some(v) = text("event") {
            self.event = v;
        }

        if let Some(v) = text("language") {
            self.language = v;
        }

        match data.get("priority") {
            Some(Value::Int(n)) => self.priority = *n,
            Some(Value::Number(n)) => self.priority = goja::value::go_float_to_int(*n),
            _ => {}
        }

        if let Some(Value::Bool(b)) = data.get("bypass_acl") {
            self.bypass_acl = *b;
        }

        if let Some(Value::Bool(b)) = data.get("enabled") {
            self.enabled = *b;
        }
    }
}

pub(crate) static HOOK_LOG: Logger = log::for_component("hook");
static HOOK_FAIL_THROTTLE: Throttle = Throttle::new(Duration::from_secs(60));

pub(crate) fn hooks_enabled() -> bool {
    globals::config().is_some_and(|c| c.api.hooks.enabled)
}

fn observe_hook(h: &Hook, entity: &str, phase: &str, start: Instant, err: Option<&HookError>) {
    metrics::observe_hook(
        &h.id,
        entity,
        phase,
        start.elapsed(),
        err.map(|e| e as &dyn std::error::Error),
    );

    let Some(err) = err else {
        if log::enabled(Level::DEBUG) {
            HOOK_LOG.debug(
                "hook executed",
                &[
                    ("hook_id", log::Value::Str(h.id.as_bytes())),
                    ("entity", log::Value::Str(entity.as_bytes())),
                    ("phase", log::Value::Str(phase.as_bytes())),
                    ("duration_ms", log::Value::Float(log::since(start))),
                ],
            );
        }
        return;
    };

    let (suppressed, ok) = HOOK_FAIL_THROTTLE.allow(&h.id);
    if ok {
        let attrs = [
            ("hook_id", log::Value::Str(h.id.as_bytes())),
            ("entity", log::Value::Str(entity.as_bytes())),
            ("phase", log::Value::Str(phase.as_bytes())),
            ("err", log::Value::Str(err.message().as_bytes())),
        ];
        HOOK_LOG.warn(
            "hook script failed",
            &log::with_suppressed(&attrs, suppressed),
        );
    }
}

pub fn init_hooks() {
    if !hooks_enabled() {
        return;
    }

    if engine::entity_type_exists(HOOK_ENTITY.as_bytes()) {
        prepare_indexes();
        return;
    }

    if let Err(err) = create_hook_entity() {
        let text = err.to_string();
        HOOK_LOG.error(
            "cannot create the hooks entity type",
            &[("err", log::Value::Str(text.as_bytes()))],
        );
    }
}

pub fn create_hook_entity() -> Result<(), crate::gocompat::errors::BoxError> {
    engine::create_entity_type(HOOK_ENTITY.as_bytes())?;

    engine::update_entity_schema(HOOK_ENTITY.as_bytes(), &hook_entity_schema());

    Ok(())
}

pub fn apply_post_read_hooks_for_entity(username: &str, entity: &str, data: &MapRef) -> MapRef {
    Request::new(username).post_read(entity, data)
}

pub fn apply_pre_read_hooks_for_entity(username: &str, entity: &str, data: &MapRef) -> MapRef {
    Request::new(username).pre_read(entity, data)
}

impl Request {
    pub fn post_read(&self, entity: &str, data: &MapRef) -> MapRef {
        self.apply(
            entity,
            data,
            HOOK_EVENT_POST_READ,
            PHASE_POST_READ,
            "postRead",
        )
    }

    pub fn pre_read(&self, entity: &str, data: &MapRef) -> MapRef {
        self.apply(entity, data, HOOK_EVENT_PRE_READ, PHASE_PRE_READ, "preRead")
    }

    fn apply(
        &self,
        entity: &str,
        data: &MapRef,
        event: &str,
        phase: &str,
        fn_name: &str,
    ) -> MapRef {
        if !hooks_enabled() {
            return data.clone();
        }

        let mut list: Vec<Arc<Hook>> = hooks_for_event(entity, event);
        list.sort_by(|a, b| b.priority.cmp(&a.priority));

        for h in &list {
            if !h.enabled {
                continue;
            }

            if self.over_budget() || self.stopped() {
                break;
            }

            let start = Instant::now();
            let res = self.apply_script(h, fn_name, data);
            self.add_spent(start.elapsed());
            let err = res.err();
            if let Some(e) = &err
                && (e.is(GuardError::Busy) || e.is(GuardError::Abandoned))
            {
                break;
            }
            observe_hook(h, entity, phase, start, err.as_ref());
        }

        data.clone()
    }
}
