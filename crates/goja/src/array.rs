use crate::heap::{ObjRef, Tracer};
use crate::object::{BASE_CLASS, BaseObject, CLASS_ARRAY, ObjClass, ObjExt, PropertyDescriptor};
use crate::runtime::{JsResult, Runtime};
use crate::value::{Value, ValueProperty};

pub struct ArrayObject {
    pub values: Vec<Value>,
    pub length: u32,
    pub obj_count: usize,
    pub prop_value_count: usize,
    pub length_prop: ValueProperty,
}

impl ArrayObject {
    pub fn trace(&self, t: &mut Tracer) {
        t.values(&self.values);
    }
}

pub static ARRAY_OBJECT_CLASS: ObjClass = BASE_CLASS;

pub fn with_own_value_property(
    _rt: &mut Runtime,
    _o: ObjRef,
    _name: &Value,
    _f: &dyn Fn(&mut ValueProperty),
) -> bool {
    false
}

pub fn check_std_array_obj_values(_rt: &mut Runtime, _o: ObjRef) -> Option<Vec<Value>> {
    None
}

pub fn define_array_length_templated(
    _rt: &mut Runtime,
    _o: ObjRef,
    _descr: PropertyDescriptor,
    _throw: bool,
) -> JsResult<bool> {
    todo!("defineArrayLength on a templated array")
}

impl Runtime {
    pub fn new_array_object(&mut self) -> ObjRef {
        let proto = self.get_array_prototype();
        self.alloc_object(
            &ARRAY_OBJECT_CLASS,
            BaseObject::new(CLASS_ARRAY, Some(proto)),
            ObjExt::Array(Box::new(ArrayObject {
                values: Vec::new(),
                length: 0,
                obj_count: 0,
                prop_value_count: 0,
                length_prop: ValueProperty {
                    writable: true,
                    ..ValueProperty::default()
                },
            })),
        )
    }

    pub fn new_array_values(&mut self, values: Vec<Value>) -> ObjRef {
        let o = self.new_array_object();
        if let ObjExt::Array(a) = &mut self.obj_mut(o).ext {
            a.length = values.len() as u32;
            a.obj_count = values.len();
            a.values = values;
        }
        o
    }
}
