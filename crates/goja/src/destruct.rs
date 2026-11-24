use crate::heap::{ObjRef, Tracer};
use crate::object::{BASE_CLASS, ObjClass, PropNameSet};

pub struct DestructKeyedSource {
    pub wrapped: ObjRef,
    pub used_keys: PropNameSet,
}

impl DestructKeyedSource {
    pub fn trace(&self, t: &mut Tracer) {
        t.obj(self.wrapped);
    }
}

pub static DESTRUCT_KEYED_SOURCE_CLASS: ObjClass = BASE_CLASS;
