use crate::heap::{StashRef, Tracer};
use crate::object::{BASE_CLASS, ObjClass};

pub struct MappedProperty {
    pub stash: StashRef,
    pub idx: usize,
}

pub struct ArgumentsObject {
    pub length: usize,
    pub mapped: Vec<(crate::unistring::String, MappedProperty)>,
}

impl ArgumentsObject {
    pub fn trace(&self, t: &mut Tracer) {
        for (_, m) in &self.mapped {
            t.stash(m.stash);
        }
    }
}

pub static ARGUMENTS_OBJECT_CLASS: ObjClass = BASE_CLASS;
