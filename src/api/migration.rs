use std::fmt;

use crate::gocompat::json::{self, Map, Value};
use crate::json_struct;
use crate::query::Filters;

use super::field::{NotAnObject, set_nested_field};
use super::storage::{list_entities, update_entity_by_id};

json_struct! {
    #[derive(Clone, Debug, Default, PartialEq)]
    pub struct MigrationQuery {
        pub entity: Vec<u8> => "Entity",
        pub action: String => "Action",
        pub properties: Option<Map> => "Properties",
    }
}

pub const ACTION_SET: &str = "set";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MigrationError {
    UnsupportedAction(String),
    MissingId,
    NotAnObject(NotAnObject),
}

impl fmt::Display for MigrationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            MigrationError::UnsupportedAction(action) => write!(f, "unsupported action: {action}"),
            MigrationError::MissingId => f.write_str("missing id"),
            MigrationError::NotAnObject(err) => err.fmt(f),
        }
    }
}

impl std::error::Error for MigrationError {}

fn take_object(v: &mut Value) -> Option<Option<Map>> {
    match v {
        Value::Object(m) => Some(Some(std::mem::take(m))),
        Value::Null => Some(None),
        _ => None,
    }
}

fn object_list(payload: &mut Value) -> Option<Vec<Option<Map>>> {
    match payload {
        Value::Null => Some(Vec::new()),
        Value::Array(items) => {
            if !items
                .iter()
                .all(|i| matches!(i, Value::Object(_) | Value::Null))
            {
                return None;
            }
            items.iter_mut().map(take_object).collect()
        }
        _ => None,
    }
}

pub fn parse_migration_query(s: &[u8], entity: &[u8]) -> Vec<MigrationQuery> {
    let Ok(mut raw) = json::unmarshal_value(s) else {
        return Vec::new();
    };

    let Some(raw_items) = object_list(&mut raw) else {
        return Vec::new();
    };

    let mut out = Vec::with_capacity(raw_items.len());
    for item in raw_items.into_iter().flatten() {
        for (action, mut payload) in item {
            if let Some(props_list) = object_list(&mut payload) {
                for props in props_list {
                    out.push(MigrationQuery {
                        entity: entity.to_vec(),
                        action: action.clone(),
                        properties: props,
                    });
                }

                continue;
            }

            if let Value::Object(props) = &mut payload {
                out.push(MigrationQuery {
                    entity: entity.to_vec(),
                    action: action.clone(),
                    properties: Some(std::mem::take(props)),
                });
            }
        }
    }

    out
}

pub fn execute_migrations(migration_queries: &[MigrationQuery]) -> Result<(), MigrationError> {
    execute_migrations_count(migration_queries).1
}

pub fn execute_migrations_count(
    migration_queries: &[MigrationQuery],
) -> (usize, Result<(), MigrationError>) {
    let mut updated = 0;
    for query in migration_queries {
        match query.action.as_str() {
            ACTION_SET => {
                let (n, err) =
                    execute_migration_set_action(&query.entity, query.properties.as_ref());
                updated += n;
                if err.is_err() {
                    return (updated, err);
                }
            }
            other => {
                return (
                    updated,
                    Err(MigrationError::UnsupportedAction(other.to_string())),
                );
            }
        }
    }

    (updated, Ok(()))
}

fn execute_migration_set_action(
    entity: &[u8],
    properties: Option<&Map>,
) -> (usize, Result<(), MigrationError>) {
    let entities = list_entities(entity, 0, 0, b"", true, Some(&Filters::new()), b"", b"all");
    let mut updated = 0;

    for mut ent in entities {
        if let Some(properties) = properties {
            for (key, value) in properties {
                if let Err(err) = set_nested_field(&mut ent, key, value.clone()) {
                    return (updated, Err(MigrationError::NotAnObject(err)));
                }
            }
        }

        let mut id = match ent.get("id") {
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        if id.is_empty()
            && let Some(Value::String(v)) = ent.get("_id")
        {
            id = v.clone();
        }

        if id.is_empty() {
            return (updated, Err(MigrationError::MissingId));
        }

        update_entity_by_id(entity, id.as_bytes(), &ent);
        updated += 1;
    }

    (updated, Ok(()))
}
