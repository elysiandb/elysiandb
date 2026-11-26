use crate::array::go_append;
use crate::heap::{ObjRef, SymRef, Tracer};
use crate::object::{
    BaseObject, CallFn, CtorFn, EnumerableIter, IterNextFunc, ObjClass, ObjData, ObjExt,
    PrivateEnvType, PropIter, PropIterItem, PropNameSet, PropertyDescriptor, UStr,
};
use crate::runtime::{JsResult, Runtime};
use crate::string::{JsString, STRING_OBJECT_C};
use crate::unistring;
use crate::value::Value;
use std::sync::Arc;

pub struct DestructKeyedSource {
    pub wrapped: Value,
    pub used_keys: PropNameSet,
}

impl ObjData for DestructKeyedSource {
    fn trace(&self, t: &mut Tracer) {
        t.value(&self.wrapped);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

enum UsedKey {
    Str(unistring::String),
    Sym(SymRef),
}

impl DestructKeyedSource {
    fn data(rt: &Runtime, o: ObjRef) -> &DestructKeyedSource {
        rt.ext_dyn::<DestructKeyedSource>(o)
            .expect("destructKeyedSource")
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut DestructKeyedSource {
        rt.ext_dyn_mut::<DestructKeyedSource>(o)
            .expect("destructKeyedSource")
    }

    fn w(rt: &mut Runtime, o: ObjRef) -> JsResult<ObjRef> {
        let wrapped = DestructKeyedSource::data(rt, o).wrapped.clone();
        wrapped.to_object(rt)
    }

    fn wrapped_obj(rt: &Runtime, o: ObjRef) -> Option<ObjRef> {
        DestructKeyedSource::data(rt, o).wrapped.as_object()
    }

    fn record_str(rt: &mut Runtime, o: ObjRef, p: &UStr) {
        DestructKeyedSource::data_mut(rt, o)
            .used_keys
            .string_props
            .insert(p.clone());
    }

    fn record_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) {
        DestructKeyedSource::data_mut(rt, o)
            .used_keys
            .symbol_props
            .insert(s);
    }

    fn used_key(rt: &mut Runtime, key: &Value) -> JsResult<UsedKey> {
        Ok(match key {
            Value::Symbol(s) => UsedKey::Sym(*s),
            v => UsedKey::Str(v.string(rt)?),
        })
    }

    fn is_used(rt: &mut Runtime, o: ObjRef, key: &Value) -> JsResult<bool> {
        let k = DestructKeyedSource::used_key(rt, key)?;
        let used = &DestructKeyedSource::data(rt, o).used_keys;
        Ok(match k {
            UsedKey::Str(s) => used.string_props.contains(&s),
            UsedKey::Sym(s) => used.symbol_props.contains(&s),
        })
    }

    pub fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        DestructKeyedSource::w(rt, o)?.sort_len(rt)
    }

    pub fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        DestructKeyedSource::w(rt, o)?.sort_get(rt, i)
    }

    pub fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        DestructKeyedSource::w(rt, o)?.swap(rt, i, j)
    }

    pub fn class_name(rt: &Runtime, o: ObjRef) -> &'static str {
        match &DestructKeyedSource::data(rt, o).wrapped {
            Value::Object(w) => w.class_name(rt),
            Value::Bool(_) => "Boolean",
            Value::Int(_) | Value::Float(_) => "Number",
            Value::String(_) => "String",
            Value::BigInt(_) => "BigInt",
            _ => "Object",
        }
    }

    pub fn type_of(rt: &Runtime, o: ObjRef) -> JsString {
        match &DestructKeyedSource::data(rt, o).wrapped {
            Value::Object(w) => w.type_of(rt),
            _ => STRING_OBJECT_C.clone(),
        }
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        DestructKeyedSource::record_str(rt, o, p);
        DestructKeyedSource::w(rt, o)?.get_str(rt, p, receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        DestructKeyedSource::record_str(rt, o, &crate::object::idx_string(p));
        DestructKeyedSource::w(rt, o)?.get_idx(rt, p, receiver)
    }

    pub fn get_sym(
        rt: &mut Runtime,
        o: ObjRef,
        p: SymRef,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        DestructKeyedSource::record_sym(rt, o, p);
        DestructKeyedSource::w(rt, o)?.get_sym(rt, p, receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<Option<Value>> {
        DestructKeyedSource::record_str(rt, o, p);
        DestructKeyedSource::w(rt, o)?.get_own_prop_str(rt, p)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, p: i64) -> JsResult<Option<Value>> {
        DestructKeyedSource::record_str(rt, o, &crate::object::idx_string(p));
        DestructKeyedSource::w(rt, o)?.get_own_prop_idx(rt, p)
    }

    pub fn get_own_prop_sym(rt: &mut Runtime, o: ObjRef, p: SymRef) -> JsResult<Option<Value>> {
        DestructKeyedSource::record_sym(rt, o, p);
        DestructKeyedSource::w(rt, o)?.get_own_prop_sym(rt, p)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.set_own_str(rt, p, v, throw)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.set_own_idx(rt, p, v, throw)
    }

    pub fn set_own_sym(
        rt: &mut Runtime,
        o: ObjRef,
        p: SymRef,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.set_own_sym(rt, p, v, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        DestructKeyedSource::w(rt, o)?.set_foreign_str(rt, p, v, receiver, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        DestructKeyedSource::w(rt, o)?.set_foreign_idx(rt, p, v, receiver, throw)
    }

    pub fn set_foreign_sym(
        rt: &mut Runtime,
        o: ObjRef,
        p: SymRef,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        DestructKeyedSource::w(rt, o)?.set_foreign_sym(rt, p, v, receiver, throw)
    }

    pub fn has_property_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_property_str(rt, p)
    }

    pub fn has_property_idx(rt: &mut Runtime, o: ObjRef, p: i64) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_property_idx(rt, p)
    }

    pub fn has_property_sym(rt: &mut Runtime, o: ObjRef, p: SymRef) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_property_sym(rt, p)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, p: &UStr) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_own_property_str(rt, p)
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, p: i64) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_own_property_idx(rt, p)
    }

    pub fn has_own_property_sym(rt: &mut Runtime, o: ObjRef, p: SymRef) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_own_property_sym(rt, p)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.define_own_property_str(rt, name, desc, throw)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        name: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.define_own_property_idx(rt, name, desc, throw)
    }

    pub fn define_own_property_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.define_own_property_sym(rt, name, desc, throw)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.delete_str(rt, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.delete_idx(rt, idx, throw)
    }

    pub fn delete_sym(rt: &mut Runtime, o: ObjRef, s: SymRef, throw: bool) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.delete_sym(rt, s, throw)
    }

    pub fn assert_callable(rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        DestructKeyedSource::wrapped_obj(rt, o)?.assert_callable(rt)
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        DestructKeyedSource::w(rt, o)?.vm_call(rt, n)
    }

    pub fn assert_constructor(rt: &Runtime, o: ObjRef) -> Option<CtorFn> {
        DestructKeyedSource::wrapped_obj(rt, o)?.assert_constructor(rt)
    }

    pub fn proto(rt: &mut Runtime, o: ObjRef) -> JsResult<Option<ObjRef>> {
        DestructKeyedSource::w(rt, o)?.proto(rt)
    }

    pub fn set_proto(
        rt: &mut Runtime,
        o: ObjRef,
        proto: Option<ObjRef>,
        throw: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.set_proto(rt, proto, throw)
    }

    pub fn has_instance(rt: &mut Runtime, o: ObjRef, v: &Value) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.has_instance(rt, v)
    }

    pub fn is_extensible(rt: &mut Runtime, o: ObjRef) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.is_extensible(rt)
    }

    pub fn prevent_extensions(rt: &mut Runtime, o: ObjRef, throw: bool) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.prevent_extensions(rt, throw)
    }

    pub fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        match DestructKeyedSource::wrapped_obj(rt, o) {
            Some(w) => w.equal(rt, other),
            None => false,
        }
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let wrapped = DestructKeyedSource::w(rt, o)?.iterate_string_keys(rt)?;
        Ok(Box::new(DestructKeyedSourceIter { d: o, wrapped }))
    }

    pub fn iterate_symbols(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let wrapped = DestructKeyedSource::w(rt, o)?.iterate_symbols(rt)?;
        Ok(Box::new(DestructKeyedSourceIter { d: o, wrapped }))
    }

    pub fn iterate_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let wrapped = DestructKeyedSource::w(rt, o)?.iterate_keys(rt)?;
        Ok(Box::new(DestructKeyedSourceIter { d: o, wrapped }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let mut next: IterNextFunc = if all {
            DestructKeyedSource::iterate_string_keys(rt, o)?
        } else {
            let w = DestructKeyedSource::w(rt, o)?;
            let wrapped = DestructKeyedSource::iterate_string_keys(rt, o)?;
            Box::new(EnumerableIter { o: w, wrapped })
        };
        while let Some(item) = next.next(rt)? {
            go_append(&mut accum, item.name);
        }
        Ok(accum)
    }

    fn filter_used_keys(rt: &mut Runtime, o: ObjRef, keys: Vec<Value>) -> JsResult<Vec<Value>> {
        let mut out = Vec::with_capacity(keys.len());
        for key in keys {
            if DestructKeyedSource::is_used(rt, o, &key)? {
                continue;
            }
            out.push(key);
        }
        Ok(out)
    }

    pub fn symbols(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let keys = DestructKeyedSource::w(rt, o)?.symbols(rt, all, accum)?;
        DestructKeyedSource::filter_used_keys(rt, o, keys)
    }

    pub fn keys(rt: &mut Runtime, o: ObjRef, all: bool, accum: Vec<Value>) -> JsResult<Vec<Value>> {
        let keys = DestructKeyedSource::w(rt, o)?.keys(rt, all, accum)?;
        DestructKeyedSource::filter_used_keys(rt, o, keys)
    }

    pub fn _put_prop(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Value {
        match DestructKeyedSource::w(rt, o) {
            Ok(w) => w._put_prop(rt, name, value, writable, enumerable, configurable),
            Err(_) => Value::Undefined,
        }
    }

    pub fn _put_sym(rt: &mut Runtime, o: ObjRef, s: SymRef, prop: Value) {
        if let Ok(w) = DestructKeyedSource::w(rt, o) {
            w._put_sym(rt, s, prop);
        }
    }

    pub fn get_private_env(
        rt: &mut Runtime,
        o: ObjRef,
        typ: &Arc<PrivateEnvType>,
        create: bool,
    ) -> JsResult<bool> {
        DestructKeyedSource::w(rt, o)?.get_private_env(rt, typ, create)
    }
}

pub struct DestructKeyedSourceIter {
    pub d: ObjRef,
    pub wrapped: IterNextFunc,
}

impl PropIter for DestructKeyedSourceIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        loop {
            let Some(item) = self.wrapped.next(rt)? else {
                return Ok(None);
            };
            if !DestructKeyedSource::is_used(rt, self.d, &item.name)? {
                return Ok(Some(item));
            }
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.d);
        self.wrapped.trace(t);
    }
}

pub static DESTRUCT_KEYED_SOURCE_CLASS: ObjClass = ObjClass {
    sort_len: DestructKeyedSource::sort_len,
    sort_get: DestructKeyedSource::sort_get,
    swap: DestructKeyedSource::swap,
    class_name: DestructKeyedSource::class_name,
    type_of: DestructKeyedSource::type_of,
    get_str: DestructKeyedSource::get_str,
    get_idx: DestructKeyedSource::get_idx,
    get_sym: DestructKeyedSource::get_sym,
    get_own_prop_str: DestructKeyedSource::get_own_prop_str,
    get_own_prop_idx: DestructKeyedSource::get_own_prop_idx,
    get_own_prop_sym: DestructKeyedSource::get_own_prop_sym,
    set_own_str: DestructKeyedSource::set_own_str,
    set_own_idx: DestructKeyedSource::set_own_idx,
    set_own_sym: DestructKeyedSource::set_own_sym,
    set_foreign_str: DestructKeyedSource::set_foreign_str,
    set_foreign_idx: DestructKeyedSource::set_foreign_idx,
    set_foreign_sym: DestructKeyedSource::set_foreign_sym,
    has_property_str: DestructKeyedSource::has_property_str,
    has_property_idx: DestructKeyedSource::has_property_idx,
    has_property_sym: DestructKeyedSource::has_property_sym,
    has_own_property_str: DestructKeyedSource::has_own_property_str,
    has_own_property_idx: DestructKeyedSource::has_own_property_idx,
    has_own_property_sym: DestructKeyedSource::has_own_property_sym,
    define_own_property_str: DestructKeyedSource::define_own_property_str,
    define_own_property_idx: DestructKeyedSource::define_own_property_idx,
    define_own_property_sym: DestructKeyedSource::define_own_property_sym,
    delete_str: DestructKeyedSource::delete_str,
    delete_idx: DestructKeyedSource::delete_idx,
    delete_sym: DestructKeyedSource::delete_sym,
    assert_callable: DestructKeyedSource::assert_callable,
    vm_call: DestructKeyedSource::vm_call,
    assert_constructor: DestructKeyedSource::assert_constructor,
    proto: DestructKeyedSource::proto,
    set_proto: DestructKeyedSource::set_proto,
    has_instance: DestructKeyedSource::has_instance,
    is_extensible: DestructKeyedSource::is_extensible,
    prevent_extensions: DestructKeyedSource::prevent_extensions,
    equal: DestructKeyedSource::equal,
    iterate_string_keys: DestructKeyedSource::iterate_string_keys,
    iterate_symbols: DestructKeyedSource::iterate_symbols,
    iterate_keys: DestructKeyedSource::iterate_keys,
    string_keys: DestructKeyedSource::string_keys,
    symbols: DestructKeyedSource::symbols,
    keys: DestructKeyedSource::keys,
    _put_prop: DestructKeyedSource::_put_prop,
    _put_sym: DestructKeyedSource::_put_sym,
    get_private_env: DestructKeyedSource::get_private_env,
};

impl Runtime {
    pub fn new_destruct_keyed_source(&mut self, wrapped: Value) -> ObjRef {
        self.alloc_object(
            &DESTRUCT_KEYED_SOURCE_CLASS,
            BaseObject::default(),
            ObjExt::Dyn(Box::new(DestructKeyedSource {
                wrapped,
                used_keys: PropNameSet::default(),
            })),
        )
    }
}
