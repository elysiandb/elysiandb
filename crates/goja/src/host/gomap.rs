use super::{DocValue, MapRef, check_host_object_property_descr, key_str};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_OBJECT, EnumerableFlag, IterNextFunc, ObjClass, ObjData, ObjExt,
    PropIter, PropIterItem, PropertyDescriptor, UStr,
};
use crate::runtime::{JsResult, Runtime};
use crate::string::new_string_value;
use crate::value::Value;
use std::any::Any;

pub const NIL_DESCRIPTOR_VALUE_PANIC: &str = "invalid memory address or nil pointer dereference";

pub struct GoMapSimple {
    pub data: MapRef,
}

impl ObjData for GoMapSimple {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub static GO_MAP_SIMPLE_CLASS: ObjClass = ObjClass {
    get_str: GoMapSimple::get_str,
    get_own_prop_str: GoMapSimple::get_own_prop_str,
    set_own_str: GoMapSimple::set_own_str,
    set_foreign_str: GoMapSimple::set_foreign_str,
    has_own_property_str: GoMapSimple::has_own_property_str,
    define_own_property_str: GoMapSimple::define_own_property_str,
    delete_str: GoMapSimple::delete_str,
    iterate_string_keys: GoMapSimple::iterate_string_keys,
    string_keys: GoMapSimple::string_keys,
    equal: GoMapSimple::equal,
    ..BASE_CLASS
};

impl Runtime {
    pub fn new_go_map_simple(&mut self, data: MapRef) -> ObjRef {
        let proto = self.global.object_prototype;
        self.alloc_object(
            &GO_MAP_SIMPLE_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(GoMapSimple { data })),
        )
    }
}

impl GoMapSimple {
    pub fn data(rt: &Runtime, o: ObjRef) -> MapRef {
        match rt.ext_dyn::<GoMapSimple>(o) {
            Some(m) => m.data.clone(),
            None => unreachable!("not a Go map"),
        }
    }

    fn _get_str(rt: &mut Runtime, o: ObjRef, name: &str) -> JsResult<Option<Value>> {
        let dv = match rt.ext_dyn::<GoMapSimple>(o) {
            Some(m) => match m.data.borrow().get(name) {
                None => return Ok(None),
                Some(v) => {
                    if let Some(x) = crate::export::scalar_value(v) {
                        return Ok(Some(x));
                    }
                    v.clone()
                }
            },
            None => unreachable!("not a Go map"),
        };
        rt.to_value(&dv).map(Some)
    }

    fn _has_str(rt: &Runtime, o: ObjRef, name: &str) -> bool {
        match rt.ext_dyn::<GoMapSimple>(o) {
            Some(m) => m.data.borrow().contains_key(name),
            None => false,
        }
    }

    fn put(rt: &Runtime, o: ObjRef, name: &str, v: DocValue) {
        let data = GoMapSimple::data(rt, o);
        let old = {
            let mut m = data.borrow_mut();
            match m.get_mut(name) {
                Some(slot) => std::mem::replace(slot, v),
                None => m.insert(name.to_string(), v).unwrap_or_default(),
            }
        };
        drop(old);
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if let Some(v) = GoMapSimple::_get_str(rt, o, &key_str(name))? {
            return Ok(Some(v));
        }
        BaseObject::get_str(rt, o, name, receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        GoMapSimple::_get_str(rt, o, &key_str(name))
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let n = key_str(name);
        if GoMapSimple::_has_str(rt, o, &n) {
            let v = rt.export(&val)?;
            GoMapSimple::put(rt, o, &n, v);
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            let (res, ok) =
                proto.set_foreign_str(rt, name, val.clone(), &Value::Object(o), throw)?;
            if ok {
                return Ok(res);
            }
        }
        if !rt.obj(o).base.extensible {
            rt.type_error_result(
                throw,
                &format!("Cannot add property {name}, object is not extensible"),
            )?;
            return Ok(false);
        }
        let v = rt.export(&val)?;
        GoMapSimple::put(rt, o, &n, v);
        Ok(true)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let present = GoMapSimple::_has_str(rt, o, &key_str(name));
        let prop = if present {
            Some(Value::Bool(true))
        } else {
            None
        };
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        Ok(GoMapSimple::_has_str(rt, o, &key_str(name)))
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if !check_host_object_property_descr(rt, name, &descr, throw)? {
            return Ok(false);
        }
        let n = key_str(name);
        if rt.obj(o).base.extensible || GoMapSimple::_has_str(rt, o, &n) {
            let Some(value) = &descr.value else {
                panic!("{}", NIL_DESCRIPTOR_VALUE_PANIC);
            };
            let v = rt.export(value)?;
            GoMapSimple::put(rt, o, &n, v);
            return Ok(true);
        }
        rt.type_error_result(
            throw,
            &format!("Cannot define property {n}, object is not extensible"),
        )?;
        Ok(false)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, _throw: bool) -> JsResult<bool> {
        let data = GoMapSimple::data(rt, o);
        let old = data.borrow_mut().remove(&*key_str(name));
        drop(old);
        Ok(true)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let data = GoMapSimple::data(rt, o);
        let names: Vec<String> = data.borrow().keys().cloned().collect();
        Ok(Box::new(GoMapPropIter { o, names, idx: 0 }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        _all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let data = GoMapSimple::data(rt, o);
        for key in data.borrow().keys() {
            accum.push(Value::String(new_string_value(key.as_bytes())));
        }
        Ok(accum)
    }

    pub fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        rt.ext_dyn::<GoMapSimple>(other).is_some() && o == other
    }
}

struct GoMapPropIter {
    o: ObjRef,
    names: Vec<String>,
    idx: usize,
}

impl PropIter for GoMapPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        while self.idx < self.names.len() {
            let name = &self.names[self.idx];
            self.idx += 1;
            if GoMapSimple::_has_str(rt, self.o, name) {
                return Ok(Some(PropIterItem {
                    name: Value::String(new_string_value(name.as_bytes())),
                    value: None,
                    enumerable: EnumerableFlag::True,
                }));
            }
        }
        Ok(None)
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
    }
}
