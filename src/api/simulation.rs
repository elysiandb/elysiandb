use std::collections::{HashMap, HashSet};

use crate::globals;
use crate::gocompat::json::{Map, Value};
use crate::schema::{self, ValidationError};

use super::relationship::{extract_sub_entities, merge_with_existing};

pub type SchemaLoader<'a> = Box<dyn Fn(&[u8]) -> Option<Map> + 'a>;
pub type EntityLoader<'a> = Box<dyn Fn(&[u8], &[u8]) -> Option<Map> + 'a>;

pub struct WriteSimulation<'a> {
    load_schema: SchemaLoader<'a>,
    load_entity: EntityLoader<'a>,
    schemas: HashMap<Vec<u8>, Option<Map>>,
    entities: HashMap<Vec<u8>, Option<Map>>,
}

impl<'a> WriteSimulation<'a> {
    pub fn new(
        load_schema: SchemaLoader<'a>,
        load_entity: EntityLoader<'a>,
    ) -> WriteSimulation<'a> {
        WriteSimulation {
            load_schema,
            load_entity,
            schemas: HashMap::new(),
            entities: HashMap::new(),
        }
    }

    pub fn write(&mut self, entity: &[u8], data: &Map) -> Vec<ValidationError> {
        self.write_checked(entity, data, true)
    }

    fn write_checked(
        &mut self,
        entity: &[u8],
        data: &Map,
        check_inferred_schema: bool,
    ) -> Vec<ValidationError> {
        let errs = self.validate(entity, data, check_inferred_schema);
        if !errs.is_empty() {
            return errs;
        }

        let mut doc = deep_copy_map(data);
        self.write_sub_entities(entity, &mut doc);
        self.store(entity, doc);

        Vec::new()
    }

    pub fn update(&mut self, entity: &[u8], id: &[u8], updates: &Map) -> bool {
        let Some(existing) = self.read(entity, id) else {
            return false;
        };

        let mut merged = deep_copy_map(&existing);
        merged.extend(deep_copy_map(updates));
        merged.insert(
            "id".to_string(),
            Value::String(String::from_utf8_lossy(id).into_owned()),
        );

        self.write_sub_entities(entity, &mut merged);
        self.store(entity, merged);

        true
    }

    pub fn delete(&mut self, entity: &[u8], id: &[u8]) {
        self.entities.insert(simulation_key(entity, id), None);
    }

    pub fn read(&mut self, entity: &[u8], id: &[u8]) -> Option<Map> {
        let key = simulation_key(entity, id);
        if let Some(doc) = self.entities.get(&key) {
            return doc.clone();
        }

        let doc = (self.load_entity)(entity, id);
        self.entities.insert(key, doc.clone());

        doc
    }

    fn write_sub_entities(&mut self, entity: &[u8], doc: &mut Map) {
        for mut sub in extract_sub_entities(entity, doc) {
            let sub_entity = match sub.remove("@entity").as_ref() {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            let id = match sub.get("id") {
                Some(Value::String(s)) => s.clone(),
                _ => String::new(),
            };
            let existing = self.read(sub_entity.as_bytes(), id.as_bytes());
            let merged = merge_with_existing(sub, existing);
            self.write_checked(sub_entity.as_bytes(), &merged, false);
        }
    }

    fn store(&mut self, entity: &[u8], doc: Map) {
        if let Some(Value::String(id)) = doc.get("id")
            && !id.is_empty()
        {
            self.entities
                .insert(simulation_key(entity, id.as_bytes()), Some(doc.clone()));
        }

        if !schema_applies(entity) {
            return;
        }

        let current = self.schema(entity);
        if globals::config().is_some_and(|c| c.api.schema.strict)
            && current.is_some()
            && schema::is_manual_schema(entity, current.as_ref())
        {
            return;
        }

        let merged = schema::merge_analyzed_schema(entity, current.as_ref(), &doc);
        self.schemas.insert(entity.to_vec(), Some(merged));
    }

    fn validate(
        &mut self,
        entity: &[u8],
        data: &Map,
        check_inferred_schema: bool,
    ) -> Vec<ValidationError> {
        if !schema_applies(entity) {
            return Vec::new();
        }

        let current = self.schema(entity);
        if current.is_none()
            || (!check_inferred_schema && !schema::is_manual_schema(entity, current.as_ref()))
        {
            return Vec::new();
        }

        schema::validate_entity(entity, data, current.as_ref())
    }

    fn schema(&mut self, entity: &[u8]) -> Option<Map> {
        if let Some(current) = self.schemas.get(entity) {
            return current.clone();
        }

        let current = (self.load_schema)(entity);
        self.schemas.insert(entity.to_vec(), current.clone());

        current
    }
}

fn schema_applies(entity: &[u8]) -> bool {
    globals::config().is_some_and(|c| c.api.schema.enabled)
        && entity != schema::SCHEMA_ENTITY.as_bytes()
}

fn simulation_key(entity: &[u8], id: &[u8]) -> Vec<u8> {
    let mut key = Vec::with_capacity(entity.len() + 1 + id.len());
    key.extend_from_slice(entity);
    key.push(0);
    key.extend_from_slice(id);
    key
}

fn deep_copy_map(m: &Map) -> Map {
    m.clone()
}

enum Walk<'a> {
    Value(&'a Value),
    Map(&'a Map),
}

fn nested_types(root: Walk<'_>) -> Vec<Vec<u8>> {
    let mut seen: HashSet<&str> = HashSet::new();
    let mut types: Vec<Vec<u8>> = Vec::new();
    let mut stack = vec![root];
    while let Some(item) = stack.pop() {
        match item {
            Walk::Map(m) | Walk::Value(Value::Object(m)) => {
                if let Some(Value::String(name)) = m.get("@entity")
                    && !name.is_empty()
                    && seen.insert(name)
                {
                    types.push(name.as_bytes().to_vec());
                }

                for child in m.values().rev() {
                    stack.push(Walk::Value(child));
                }
            }
            Walk::Value(Value::Array(items)) => {
                for child in items.iter().rev() {
                    stack.push(Walk::Value(child));
                }
            }
            Walk::Value(_) => {}
        }
    }

    types
}

pub fn nested_entity_types(data: &Value) -> Vec<Vec<u8>> {
    nested_types(Walk::Value(data))
}

pub fn nested_entity_types_of_map(data: &Map) -> Vec<Vec<u8>> {
    nested_types(Walk::Map(data))
}
