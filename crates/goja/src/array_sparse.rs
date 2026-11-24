use crate::heap::Tracer;
use crate::object::{BASE_CLASS, ObjClass};
use crate::value::{Value, ValueProperty};

#[derive(Clone)]
pub struct SparseArrayItem {
    pub idx: u32,
    pub value: Value,
}

pub struct SparseArrayObject {
    pub items: Vec<SparseArrayItem>,
    pub length: u32,
    pub length_prop: ValueProperty,
}

impl SparseArrayObject {
    pub fn trace(&self, t: &mut Tracer) {
        for i in &self.items {
            t.value(&i.value);
        }
    }
}

pub static SPARSE_ARRAY_OBJECT_CLASS: ObjClass = BASE_CLASS;
