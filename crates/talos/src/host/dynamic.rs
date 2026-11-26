use super::{DocValue, key_str};
use crate::heap::{ObjRef, SymRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ARRAY, CLASS_OBJECT, EmptyIter, EnumerableFlag, Flag,
    IterNextFunc, ObjClass, ObjData, ObjExt, PrivateEnvType, PropIter, PropIterItem,
    PropertyDescriptor, UStr,
};
use crate::runtime::{JsResult, Runtime, Throw, str_to_int};
use crate::string::{ascii_string, new_string_value};
use crate::value::{Value, ValueProperty, int_to_value};
use std::any::Any;
use std::rc::Rc;
use std::sync::Arc;

pub trait DynamicObject: Any {
    fn get(&self, rt: &mut Runtime, key: &str) -> JsResult<Option<Value>>;
    fn set(&self, rt: &mut Runtime, key: &str, val: Value) -> JsResult<bool>;
    fn has(&self, rt: &mut Runtime, key: &str) -> JsResult<bool>;
    fn delete(&self, rt: &mut Runtime, key: &str) -> JsResult<bool>;
    fn keys(&self, rt: &mut Runtime) -> JsResult<Vec<String>>;
    fn export(self: Rc<Self>) -> DocValue;
    fn trace(&self, _t: &mut Tracer) {}
}

pub trait DynamicArray: Any {
    fn length(&self, rt: &mut Runtime) -> JsResult<i64>;
    fn get(&self, rt: &mut Runtime, idx: i64) -> JsResult<Option<Value>>;
    fn set(&self, rt: &mut Runtime, idx: i64, val: Value) -> JsResult<bool>;
    fn set_length(&self, rt: &mut Runtime, len: i64) -> JsResult<bool>;
    fn export(self: Rc<Self>) -> DocValue;
    fn trace(&self, _t: &mut Tracer) {}
}

pub struct DynamicObjectData {
    pub d: Rc<dyn DynamicObject>,
}

pub struct DynamicArrayObject {
    pub a: Rc<dyn DynamicArray>,
}

impl ObjData for DynamicObjectData {
    fn trace(&self, t: &mut Tracer) {
        self.d.trace(t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl ObjData for DynamicArrayObject {
    fn trace(&self, t: &mut Tracer) {
        self.a.trace(t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

const BASE_DYNAMIC_CLASS: ObjClass = ObjClass {
    get_sym: base_get_sym,
    get_own_prop_sym: base_get_own_prop_sym,
    set_own_sym: base_set_own_sym,
    set_foreign_sym: base_set_foreign_sym,
    has_property_sym: base_has_property_sym,
    has_own_property_sym: base_has_own_property_sym,
    define_own_property_sym: base_define_own_property_sym,
    delete_sym: base_delete_sym,
    vm_call: base_vm_call,
    set_proto: base_set_proto,
    has_instance: base_has_instance,
    is_extensible: base_is_extensible,
    prevent_extensions: base_prevent_extensions,
    iterate_symbols: base_iterate_symbols,
    symbols: base_symbols,
    _put_prop: base_put_prop,
    _put_sym: base_put_sym,
    get_private_env: base_get_private_env,
    ..BASE_CLASS
};

pub static DYNAMIC_OBJECT_CLASS: ObjClass = ObjClass {
    sort_len: |_, _| Ok(0),
    sort_get: |_, _, _| Ok(Value::Undefined),
    swap: |_, _, _, _| Ok(()),
    get_str: DynObj::get_str,
    get_idx: DynObj::get_idx,
    get_own_prop_str: DynObj::get_own_prop_str,
    get_own_prop_idx: DynObj::get_own_prop_idx,
    set_own_str: DynObj::set_own_str,
    set_own_idx: DynObj::set_own_idx,
    set_foreign_str: DynObj::set_foreign_str,
    set_foreign_idx: DynObj::set_foreign_idx,
    has_property_str: DynObj::has_property_str,
    has_property_idx: DynObj::has_property_idx,
    has_own_property_str: DynObj::has_own_property_str,
    has_own_property_idx: DynObj::has_own_property_idx,
    define_own_property_str: DynObj::define_own_property_str,
    define_own_property_idx: DynObj::define_own_property_idx,
    delete_str: DynObj::delete_str,
    delete_idx: DynObj::delete_idx,
    iterate_string_keys: DynObj::iterate_string_keys,
    iterate_keys: DynObj::iterate_string_keys,
    string_keys: DynObj::string_keys,
    keys: DynObj::string_keys,
    equal: DynObj::equal,
    ..BASE_DYNAMIC_CLASS
};

pub static DYNAMIC_ARRAY_CLASS: ObjClass = ObjClass {
    sort_len: DynArr::sort_len,
    sort_get: DynArr::sort_get,
    swap: DynArr::swap,
    get_str: DynArr::get_str,
    get_idx: DynArr::get_idx,
    get_own_prop_str: DynArr::get_own_prop_str,
    get_own_prop_idx: DynArr::get_own_prop_idx,
    set_own_str: DynArr::set_own_str,
    set_own_idx: DynArr::set_own_idx,
    set_foreign_str: DynArr::set_foreign_str,
    set_foreign_idx: DynArr::set_foreign_idx,
    has_property_str: DynArr::has_property_str,
    has_property_idx: DynArr::has_property_idx,
    has_own_property_str: DynArr::has_own_property_str,
    has_own_property_idx: DynArr::has_own_property_idx,
    define_own_property_str: DynArr::define_own_property_str,
    define_own_property_idx: DynArr::define_own_property_idx,
    delete_str: DynArr::delete_str,
    delete_idx: DynArr::delete_idx,
    iterate_string_keys: DynArr::iterate_string_keys,
    iterate_keys: DynArr::iterate_string_keys,
    string_keys: DynArr::string_keys,
    keys: DynArr::string_keys,
    equal: |_, o, other| o == other,
    ..BASE_DYNAMIC_CLASS
};

impl Runtime {
    pub fn new_dynamic_object(&mut self, d: Rc<dyn DynamicObject>) -> ObjRef {
        let proto = self.global.object_prototype;
        self.alloc_object(
            &DYNAMIC_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(DynamicObjectData { d })),
        )
    }

    pub fn new_dynamic_array(&mut self, a: Rc<dyn DynamicArray>) -> ObjRef {
        let proto = self.get_array_prototype();
        self.alloc_object(
            &DYNAMIC_ARRAY_CLASS,
            BaseObject::new(CLASS_ARRAY, Some(proto)),
            ObjExt::Dyn(Box::new(DynamicArrayObject { a })),
        )
    }
}

fn proto(rt: &Runtime, o: ObjRef) -> Option<ObjRef> {
    rt.obj(o).base.prototype
}

fn get_parent_str(
    rt: &mut Runtime,
    o: ObjRef,
    p: &UStr,
    receiver: Option<&Value>,
) -> JsResult<Option<Value>> {
    if let Some(pr) = proto(rt, o) {
        return match receiver {
            None => pr.get_str(rt, p, Some(&Value::Object(o))),
            Some(r) => pr.get_str(rt, p, Some(r)),
        };
    }
    Ok(None)
}

fn get_parent_idx(
    rt: &mut Runtime,
    o: ObjRef,
    p: i64,
    receiver: Option<&Value>,
) -> JsResult<Option<Value>> {
    if let Some(pr) = proto(rt, o) {
        return match receiver {
            None => pr.get_idx(rt, p, Some(&Value::Object(o))),
            Some(r) => pr.get_idx(rt, p, Some(r)),
        };
    }
    Ok(None)
}

fn base_get_sym(
    rt: &mut Runtime,
    o: ObjRef,
    p: SymRef,
    receiver: Option<&Value>,
) -> JsResult<Option<Value>> {
    if let Some(pr) = proto(rt, o) {
        return match receiver {
            None => pr.get_sym(rt, p, Some(&Value::Object(o))),
            Some(r) => pr.get_sym(rt, p, Some(r)),
        };
    }
    Ok(None)
}

fn base_get_own_prop_sym(_rt: &mut Runtime, _o: ObjRef, _p: SymRef) -> JsResult<Option<Value>> {
    Ok(None)
}

fn set_sym_error(throw: bool) -> JsResult<()> {
    if throw {
        return Err(Throw::TypeError(
            "Dynamic objects do not support Symbol properties".to_string(),
        ));
    }
    Ok(())
}

fn base_set_own_sym(
    rt: &mut Runtime,
    o: ObjRef,
    s: SymRef,
    v: Value,
    throw: bool,
) -> JsResult<bool> {
    if let Some(pr) = proto(rt, o) {
        let (res, handled) = pr.set_foreign_sym(rt, s, v, &Value::Object(o), throw)?;
        if handled {
            return Ok(res);
        }
    }
    set_sym_error(throw)?;
    Ok(false)
}

fn base_set_foreign_sym(
    rt: &mut Runtime,
    o: ObjRef,
    p: SymRef,
    v: Value,
    receiver: &Value,
    throw: bool,
) -> JsResult<(bool, bool)> {
    if let Some(pr) = proto(rt, o) {
        if !matches!(receiver, Value::Object(r) if *r == pr) {
            return pr.set_foreign_sym(rt, p, v, receiver, throw);
        }
        return Ok((pr.set_own_sym(rt, p, v, throw)?, true));
    }
    Ok((false, false))
}

fn base_has_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
    if let Some(pr) = proto(rt, o) {
        return pr.has_property_sym(rt, s);
    }
    Ok(false)
}

fn base_has_own_property_sym(_rt: &mut Runtime, _o: ObjRef, _s: SymRef) -> JsResult<bool> {
    Ok(false)
}

fn base_define_own_property_sym(
    _rt: &mut Runtime,
    _o: ObjRef,
    _s: SymRef,
    _d: PropertyDescriptor,
    throw: bool,
) -> JsResult<bool> {
    set_sym_error(throw)?;
    Ok(false)
}

fn base_delete_sym(_rt: &mut Runtime, _o: ObjRef, _s: SymRef, _throw: bool) -> JsResult<bool> {
    Ok(true)
}

fn base_vm_call(rt: &mut Runtime, _o: ObjRef, _n: usize) -> JsResult<()> {
    Err(Throw::Value(
        rt.new_type_error("Dynamic object is not callable"),
    ))
}

fn base_set_proto(rt: &mut Runtime, o: ObjRef, p: Option<ObjRef>, _throw: bool) -> JsResult<bool> {
    rt.obj_mut(o).base.prototype = p;
    Ok(true)
}

fn base_has_instance(_rt: &mut Runtime, _o: ObjRef, _v: &Value) -> JsResult<bool> {
    Err(Throw::TypeError(
        "Expecting a function in instanceof check, but got a dynamic object".to_string(),
    ))
}

fn base_is_extensible(_rt: &mut Runtime, _o: ObjRef) -> JsResult<bool> {
    Ok(true)
}

fn base_prevent_extensions(rt: &mut Runtime, _o: ObjRef, throw: bool) -> JsResult<bool> {
    rt.type_error_result(throw, "Cannot make a dynamic object non-extensible")?;
    Ok(false)
}

fn base_iterate_symbols(_rt: &mut Runtime, _o: ObjRef) -> JsResult<IterNextFunc> {
    Ok(Box::new(EmptyIter))
}

fn base_symbols(
    _rt: &mut Runtime,
    _o: ObjRef,
    _all: bool,
    accum: Vec<Value>,
) -> JsResult<Vec<Value>> {
    Ok(accum)
}

fn base_put_prop(
    _rt: &mut Runtime,
    _o: ObjRef,
    _name: &UStr,
    _value: Value,
    _w: bool,
    _e: bool,
    _c: bool,
) -> Value {
    Value::Undefined
}

fn base_put_sym(_rt: &mut Runtime, _o: ObjRef, _s: SymRef, _prop: Value) {}

fn base_get_private_env(
    _rt: &mut Runtime,
    _o: ObjRef,
    _typ: &Arc<PrivateEnvType>,
    _create: bool,
) -> JsResult<bool> {
    Err(Throw::TypeError(
        "Dynamic objects cannot have private elements".to_string(),
    ))
}

fn check_descr(
    rt: &mut Runtime,
    name: &str,
    descr: &PropertyDescriptor,
    throw: bool,
) -> JsResult<bool> {
    let q = gocompat::strconv::quote(name.as_bytes());
    if descr.getter.is_some() || descr.setter.is_some() {
        rt.type_error_result(throw, "Dynamic objects do not support accessor properties")?;
        return Ok(false);
    }
    if descr.writable == Flag::False {
        rt.type_error_result(
            throw,
            &format!("Dynamic object field {q} cannot be made read-only"),
        )?;
        return Ok(false);
    }
    if descr.enumerable == Flag::False {
        rt.type_error_result(
            throw,
            &format!("Dynamic object field {q} cannot be made non-enumerable"),
        )?;
        return Ok(false);
    }
    if descr.configurable == Flag::False {
        rt.type_error_result(
            throw,
            &format!("Dynamic object field {q} cannot be made non-configurable"),
        )?;
        return Ok(false);
    }
    Ok(true)
}

fn set_parent_foreign_str(
    rt: &mut Runtime,
    o: ObjRef,
    p: &UStr,
    v: Value,
    receiver: &Value,
    throw: bool,
) -> JsResult<(bool, bool)> {
    if let Some(pr) = proto(rt, o) {
        if !matches!(receiver, Value::Object(r) if *r == pr) {
            return pr.set_foreign_str(rt, p, v, receiver, throw);
        }
        return Ok((pr.set_own_str(rt, p, v, throw)?, true));
    }
    Ok((false, false))
}

fn set_parent_foreign_idx(
    rt: &mut Runtime,
    o: ObjRef,
    p: i64,
    v: Value,
    receiver: &Value,
    throw: bool,
) -> JsResult<(bool, bool)> {
    if let Some(pr) = proto(rt, o) {
        if !matches!(receiver, Value::Object(r) if *r == pr) {
            return pr.set_foreign_idx(rt, p, v, receiver, throw);
        }
        return Ok((pr.set_own_idx(rt, p, v, throw)?, true));
    }
    Ok((false, false))
}

struct DynObj;

impl DynObj {
    fn d(rt: &Runtime, o: ObjRef) -> Rc<dyn DynamicObject> {
        match rt.ext_dyn::<DynamicObjectData>(o) {
            Some(d) => d.d.clone(),
            None => unreachable!("not a dynamic object"),
        }
    }

    fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let d = DynObj::d(rt, o);
        match d.get(rt, &key_str(p))? {
            Some(v) => Ok(Some(v)),
            None => get_parent_str(rt, o, p, receiver),
        }
    }

    fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let d = DynObj::d(rt, o);
        match d.get(rt, &p.to_string())? {
            Some(v) => Ok(Some(v)),
            None => get_parent_idx(rt, o, p, receiver),
        }
    }

    fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<Option<Value>> {
        let d = DynObj::d(rt, o);
        d.get(rt, &key_str(p))
    }

    fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, p: i64) -> JsResult<Option<Value>> {
        let d = DynObj::d(rt, o);
        d.get(rt, &p.to_string())
    }

    fn _set(rt: &mut Runtime, o: ObjRef, prop: &str, v: Value, throw: bool) -> JsResult<bool> {
        let d = DynObj::d(rt, o);
        if d.set(rt, prop, v)? {
            return Ok(true);
        }
        rt.type_error_result(throw, "'Set' on a dynamic object returned false")?;
        Ok(false)
    }

    fn set_own_str(rt: &mut Runtime, o: ObjRef, p: &UStr, v: Value, throw: bool) -> JsResult<bool> {
        let prop = key_str(p).into_owned();
        let d = DynObj::d(rt, o);
        if !d.has(rt, &prop)?
            && let Some(pr) = proto(rt, o)
        {
            let (res, handled) = pr.set_foreign_str(rt, p, v.clone(), &Value::Object(o), throw)?;
            if handled {
                return Ok(res);
            }
        }
        DynObj::_set(rt, o, &prop, v, throw)
    }

    fn set_own_idx(rt: &mut Runtime, o: ObjRef, p: i64, v: Value, throw: bool) -> JsResult<bool> {
        let prop = p.to_string();
        let d = DynObj::d(rt, o);
        if !d.has(rt, &prop)?
            && let Some(pr) = proto(rt, o)
        {
            let (res, handled) = pr.set_foreign_idx(rt, p, v.clone(), &Value::Object(o), throw)?;
            if handled {
                return Ok(res);
            }
        }
        DynObj::_set(rt, o, &prop, v, throw)
    }

    fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let d = DynObj::d(rt, o);
        if !d.has(rt, &key_str(p))? {
            return set_parent_foreign_str(rt, o, p, v, receiver, throw);
        }
        Ok((false, false))
    }

    fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let d = DynObj::d(rt, o);
        if !d.has(rt, &p.to_string())? {
            return set_parent_foreign_idx(rt, o, p, v, receiver, throw);
        }
        Ok((false, false))
    }

    fn has_property_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<bool> {
        if DynObj::has_own_property_str(rt, o, p)? {
            return Ok(true);
        }
        match proto(rt, o) {
            Some(pr) => pr.has_property_str(rt, p),
            None => Ok(false),
        }
    }

    fn has_property_idx(rt: &mut Runtime, o: ObjRef, p: i64) -> JsResult<bool> {
        if DynObj::has_own_property_idx(rt, o, p)? {
            return Ok(true);
        }
        match proto(rt, o) {
            Some(pr) => pr.has_property_idx(rt, p),
            None => Ok(false),
        }
    }

    fn has_own_property_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<bool> {
        let d = DynObj::d(rt, o);
        d.has(rt, &key_str(p))
    }

    fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, p: i64) -> JsResult<bool> {
        let d = DynObj::d(rt, o);
        d.has(rt, &p.to_string())
    }

    fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let n = key_str(name).into_owned();
        if check_descr(rt, &n, &desc, throw)? {
            let v = desc.value.unwrap_or(Value::Undefined);
            return DynObj::_set(rt, o, &n, v, throw);
        }
        Ok(false)
    }

    fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        name: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let n = name.to_string();
        if check_descr(rt, &n, &desc, throw)? {
            let v = desc.value.unwrap_or(Value::Undefined);
            return DynObj::_set(rt, o, &n, v, throw);
        }
        Ok(false)
    }

    fn _delete(rt: &mut Runtime, o: ObjRef, prop: &str, throw: bool) -> JsResult<bool> {
        let d = DynObj::d(rt, o);
        if d.delete(rt, prop)? {
            return Ok(true);
        }
        let q = gocompat::strconv::quote(prop.as_bytes());
        rt.type_error_result(
            throw,
            &format!("Could not delete property {q} of a dynamic object"),
        )?;
        Ok(false)
    }

    fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        DynObj::_delete(rt, o, &key_str(name), throw)
    }

    fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        DynObj::_delete(rt, o, &idx.to_string(), throw)
    }

    fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let d = DynObj::d(rt, o);
        let names = d.keys(rt)?;
        Ok(Box::new(DynamicObjectPropIter { o, names, idx: 0 }))
    }

    fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        _all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let d = DynObj::d(rt, o);
        for k in d.keys(rt)? {
            accum.push(Value::String(new_string_value(k.as_bytes())));
        }
        Ok(accum)
    }

    fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        match rt.ext_dyn::<DynamicObjectData>(other) {
            Some(d) => {
                let a = DynObj::d(rt, o);
                std::ptr::eq(Rc::as_ptr(&a) as *const (), Rc::as_ptr(&d.d) as *const ())
            }
            None => false,
        }
    }
}

struct DynamicObjectPropIter {
    o: ObjRef,
    names: Vec<String>,
    idx: usize,
}

impl PropIter for DynamicObjectPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        while self.idx < self.names.len() {
            let name = self.names[self.idx].clone();
            self.idx += 1;
            let d = DynObj::d(rt, self.o);
            if d.has(rt, &name)? {
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

struct DynArr;

impl DynArr {
    fn a(rt: &Runtime, o: ObjRef) -> Rc<dyn DynamicArray> {
        match rt.ext_dyn::<DynamicArrayObject>(o) {
            Some(a) => a.a.clone(),
            None => unreachable!("not a dynamic array"),
        }
    }

    fn len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        let a = DynArr::a(rt, o);
        a.length(rt)
    }

    fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        DynArr::len(rt, o)
    }

    fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        let a = DynArr::a(rt, o);
        Ok(a.get(rt, i)?.unwrap_or(Value::Undefined))
    }

    fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        let a = DynArr::a(rt, o);
        let x = a.get(rt, i)?.unwrap_or(Value::Undefined);
        let y = a.get(rt, j)?.unwrap_or(Value::Undefined);
        a.set(rt, i, y)?;
        a.set(rt, j, x)?;
        Ok(())
    }

    fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if p == "length" {
            return Ok(Some(int_to_value(DynArr::len(rt, o)?)));
        }
        let (idx, ok) = str_to_int(p);
        if ok {
            let a = DynArr::a(rt, o);
            return a.get(rt, idx);
        }
        get_parent_str(rt, o, p, receiver)
    }

    fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if let Some(v) = DynArr::get_own_prop_idx(rt, o, p)? {
            return Ok(Some(v));
        }
        get_parent_idx(rt, o, p, receiver)
    }

    fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, u: &UStr) -> JsResult<Option<Value>> {
        if u == "length" {
            let l = DynArr::len(rt, o)?;
            return Ok(Some(Value::Property(Box::new(ValueProperty {
                value: int_to_value(l),
                writable: true,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter_func: None,
                setter_func: None,
            }))));
        }
        let (idx, ok) = str_to_int(u);
        if ok {
            let a = DynArr::a(rt, o);
            return a.get(rt, idx);
        }
        Ok(None)
    }

    fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, v: i64) -> JsResult<Option<Value>> {
        let a = DynArr::a(rt, o);
        a.get(rt, v)
    }

    fn _set_len(rt: &mut Runtime, o: ObjRef, v: &Value, throw: bool) -> JsResult<bool> {
        let n = v.to_integer(rt)?;
        let a = DynArr::a(rt, o);
        if a.set_length(rt, n)? {
            return Ok(true);
        }
        rt.type_error_result(throw, "'SetLen' on a dynamic array returned false")?;
        Ok(false)
    }

    fn _set_idx(rt: &mut Runtime, o: ObjRef, idx: i64, v: Value, throw: bool) -> JsResult<bool> {
        let a = DynArr::a(rt, o);
        if a.set(rt, idx, v)? {
            return Ok(true);
        }
        rt.type_error_result(throw, "'Set' on a dynamic array returned false")?;
        Ok(false)
    }

    fn set_own_str(rt: &mut Runtime, o: ObjRef, p: &UStr, v: Value, throw: bool) -> JsResult<bool> {
        if p == "length" {
            return DynArr::_set_len(rt, o, &v, throw);
        }
        let (idx, ok) = str_to_int(p);
        if ok {
            return DynArr::_set_idx(rt, o, idx, v, throw);
        }
        let q = gocompat::strconv::quote(p.string().as_bytes());
        rt.type_error_result(
            throw,
            &format!("Cannot set property {q} on a dynamic array"),
        )?;
        Ok(false)
    }

    fn set_own_idx(rt: &mut Runtime, o: ObjRef, p: i64, v: Value, throw: bool) -> JsResult<bool> {
        DynArr::_set_idx(rt, o, p, v, throw)
    }

    fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        set_parent_foreign_str(rt, o, p, v, receiver, throw)
    }

    fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        set_parent_foreign_idx(rt, o, p, v, receiver, throw)
    }

    fn has_property_str(rt: &mut Runtime, o: ObjRef, u: &UStr) -> JsResult<bool> {
        if DynArr::has_own_property_str(rt, o, u)? {
            return Ok(true);
        }
        match proto(rt, o) {
            Some(pr) => pr.has_property_str(rt, u),
            None => Ok(false),
        }
    }

    fn has_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        if DynArr::has_own_property_idx(rt, o, idx)? {
            return Ok(true);
        }
        match proto(rt, o) {
            Some(pr) => pr.has_property_idx(rt, idx),
            None => Ok(false),
        }
    }

    fn _has(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        Ok(idx >= 0 && idx < DynArr::len(rt, o)?)
    }

    fn has_own_property_str(rt: &mut Runtime, o: ObjRef, u: &UStr) -> JsResult<bool> {
        if u == "length" {
            return Ok(true);
        }
        let (idx, ok) = str_to_int(u);
        if ok {
            return DynArr::_has(rt, o, idx);
        }
        Ok(false)
    }

    fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, v: i64) -> JsResult<bool> {
        DynArr::_has(rt, o, v)
    }

    fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let n = name.string();
        if check_descr(rt, &n, &desc, throw)? {
            let (idx, ok) = str_to_int(name);
            if ok {
                let v = desc.value.unwrap_or(Value::Undefined);
                return DynArr::_set_idx(rt, o, idx, v, throw);
            }
            let q = gocompat::strconv::quote(n.as_bytes());
            rt.type_error_result(
                throw,
                &format!("Cannot define property {q} on a dynamic array"),
            )?;
        }
        Ok(false)
    }

    fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        name: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if check_descr(rt, &name.to_string(), &desc, throw)? {
            let v = desc.value.unwrap_or(Value::Undefined);
            return DynArr::_set_idx(rt, o, name, v, throw);
        }
        Ok(false)
    }

    fn _delete(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        if DynArr::_has(rt, o, idx)? {
            DynArr::_set_idx(rt, o, idx, Value::Undefined, throw)?;
        }
        Ok(true)
    }

    fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let (idx, ok) = str_to_int(name);
        if ok {
            return DynArr::_delete(rt, o, idx, throw);
        }
        if DynArr::has_own_property_str(rt, o, name)? {
            let q = gocompat::strconv::quote(name.string().as_bytes());
            rt.type_error_result(
                throw,
                &format!("Cannot delete property {q} on a dynamic array"),
            )?;
            return Ok(false);
        }
        Ok(true)
    }

    fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        DynArr::_delete(rt, o, idx, throw)
    }

    fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let limit = DynArr::len(rt, o)?;
        Ok(Box::new(DynArrayPropIter { o, idx: 0, limit }))
    }

    fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let al = DynArr::len(rt, o)?;
        for i in 0..al {
            accum.push(Value::String(ascii_string(&i.to_string())));
        }
        if all {
            accum.push(Value::String(ascii_string("length")));
        }
        Ok(accum)
    }
}

struct DynArrayPropIter {
    o: ObjRef,
    idx: i64,
    limit: i64,
}

impl PropIter for DynArrayPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.idx < self.limit && self.idx < DynArr::len(rt, self.o)? {
            let name = self.idx.to_string();
            self.idx += 1;
            return Ok(Some(PropIterItem {
                name: Value::String(ascii_string(&name)),
                value: None,
                enumerable: EnumerableFlag::True,
            }));
        }
        Ok(None)
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
    }
}
