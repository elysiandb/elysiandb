use std::fmt;

use crate::gocompat::json::{Map, Value, quote_text};
use crate::gocompat::strings;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NotAnObject {
    pub property: String,
    pub prefix: String,
    pub kind: &'static str,
}

impl fmt::Display for NotAnObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot set {}: {} is {}, not an object",
            quote_text(self.property.as_bytes()),
            quote_text(self.prefix.as_bytes()),
            self.kind
        )
    }
}

impl std::error::Error for NotAnObject {}

fn kind_of(v: &Value) -> &'static str {
    match v {
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) | Value::Int(_) => "a number",
        Value::String(_) => "a string",
        Value::Array(_) | Value::DocList(_) => "an array",
        Value::Object(_) => "an object",
    }
}

fn map_get<'a>(m: &'a Map, key: &[u8]) -> Option<&'a Value> {
    std::str::from_utf8(key).ok().and_then(|k| m.get(k))
}

pub fn get_nested_value<'a>(data: &'a Map, path: &[u8]) -> Option<&'a Value> {
    let mut parts = path.split(|&c| c == b'.');
    let first = parts.next()?;
    let mut current = map_get(data, first)?;
    for part in parts {
        let Value::Object(m) = current else {
            return None;
        };
        current = map_get(m, part)?;
    }

    Some(current)
}

pub fn set_nested_field(dest: &mut Map, path: &str, value: Value) -> Result<(), NotAnObject> {
    let parts: Vec<&str> = path.split('.').collect();
    let mut current = dest;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            current.insert((*part).to_string(), value);
            return Ok(());
        }

        let slot = current
            .entry((*part).to_string())
            .or_insert_with(|| Value::Object(Map::new()));
        match slot {
            Value::Object(m) => current = m,
            other => {
                return Err(NotAnObject {
                    property: path.to_string(),
                    prefix: parts[..=i].join("."),
                    kind: kind_of(other),
                });
            }
        }
    }

    Ok(())
}

pub fn filter_fields(data: &Map, fields: &[Vec<u8>]) -> Map {
    let mut filtered = Map::new();
    for field in fields {
        if let Some(value) = get_nested_value(data, field)
            && let Ok(path) = std::str::from_utf8(field)
        {
            let _ = set_nested_field(&mut filtered, path, value.clone());
        }
    }

    filtered
}

pub fn parse_fields_param(param: &[u8]) -> Vec<Vec<u8>> {
    if param.is_empty() {
        return Vec::new();
    }

    param
        .split(|&c| c == b',')
        .map(strings::trim_space)
        .filter(|p| !p.is_empty())
        .map(<[u8]>::to_vec)
        .collect()
}
