use crate::array::go_append;
use crate::builtin_symbol::{SYM_HAS_INSTANCE, SYM_TO_PRIMITIVE};
use crate::heap::{FxHashMap, FxHashSet, ObjRef, Object, SymRef, Trace, Tracer};
use crate::map::OrderedMap;
use crate::runtime::{JsResult, Runtime, Throw, str_to_array_idx};
use crate::string::{JsString, STRING_OBJECT_C, string_value_from_raw};
use crate::unistring;
use crate::value::{
    Value, ValueProperty, nil_safe, prop_getter, prop_setter, property_get, property_set,
    value_prop,
};
use std::rc::Rc;
use std::sync::Arc;

pub const CLASS_OBJECT: &str = "Object";
pub const CLASS_ARRAY: &str = "Array";
pub const CLASS_WEAK_SET: &str = "WeakSet";
pub const CLASS_WEAK_MAP: &str = "WeakMap";
pub const CLASS_MAP: &str = "Map";
pub const CLASS_MATH: &str = "Math";
pub const CLASS_SET: &str = "Set";
pub const CLASS_FUNCTION: &str = "Function";
pub const CLASS_ASYNC_FUNCTION: &str = "AsyncFunction";
pub const CLASS_NUMBER: &str = "Number";
pub const CLASS_STRING: &str = "String";
pub const CLASS_BOOLEAN: &str = "Boolean";
pub const CLASS_ERROR: &str = "Error";
pub const CLASS_REG_EXP: &str = "RegExp";
pub const CLASS_DATE: &str = "Date";
pub const CLASS_JSON: &str = "JSON";
pub const CLASS_GLOBAL: &str = "global";
pub const CLASS_PROMISE: &str = "Promise";
pub const CLASS_ARRAY_ITERATOR: &str = "Array Iterator";
pub const CLASS_MAP_ITERATOR: &str = "Map Iterator";
pub const CLASS_SET_ITERATOR: &str = "Set Iterator";
pub const CLASS_STRING_ITERATOR: &str = "String Iterator";
pub const CLASS_REG_EXP_STRING_ITERATOR: &str = "RegExp String Iterator";
pub const CLASS_GENERATOR: &str = "Generator";
pub const CLASS_GENERATOR_FUNCTION: &str = "GeneratorFunction";

pub const HINT_DEFAULT: &str = "default";
pub const HINT_NUMBER: &str = "number";
pub const HINT_STRING: &str = "string";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Flag {
    #[default]
    NotSet,
    False,
    True,
}

pub const FLAG_NOT_SET: Flag = Flag::NotSet;
pub const FLAG_FALSE: Flag = Flag::False;
pub const FLAG_TRUE: Flag = Flag::True;

impl Flag {
    pub fn bool(self) -> bool {
        self == Flag::True
    }
}

pub fn to_flag(b: bool) -> Flag {
    if b { Flag::True } else { Flag::False }
}

#[derive(Clone, Debug, Default)]
pub struct PropertyDescriptor {
    pub js_descriptor: Option<ObjRef>,
    pub value: Option<Value>,
    pub writable: Flag,
    pub configurable: Flag,
    pub enumerable: Flag,
    pub getter: Option<Value>,
    pub setter: Option<Value>,
}

impl PropertyDescriptor {
    pub fn empty(&self) -> bool {
        self.js_descriptor.is_none()
            && self.value.is_none()
            && self.writable == Flag::NotSet
            && self.configurable == Flag::NotSet
            && self.enumerable == Flag::NotSet
            && self.getter.is_none()
            && self.setter.is_none()
    }

    pub fn is_accessor(&self) -> bool {
        self.setter.is_some() || self.getter.is_some()
    }

    pub fn is_data(&self) -> bool {
        self.value.is_some() || self.writable != Flag::NotSet
    }

    pub fn is_generic(&self) -> bool {
        !self.is_accessor() && !self.is_data()
    }

    pub fn to_value(&self, r: &mut Runtime) -> Value {
        if let Some(d) = self.js_descriptor {
            return Value::Object(d);
        }
        if self.empty() {
            return Value::Undefined;
        }
        let o = r.new_object();
        if let Some(v) = &self.value {
            o._put_prop(r, &"value".into(), v.clone(), true, true, true);
        }
        if self.writable != Flag::NotSet {
            o._put_prop(
                r,
                &"writable".into(),
                Value::Bool(self.writable.bool()),
                true,
                true,
                true,
            );
        }
        if self.enumerable != Flag::NotSet {
            o._put_prop(
                r,
                &"enumerable".into(),
                Value::Bool(self.enumerable.bool()),
                true,
                true,
                true,
            );
        }
        if self.configurable != Flag::NotSet {
            o._put_prop(
                r,
                &"configurable".into(),
                Value::Bool(self.configurable.bool()),
                true,
                true,
                true,
            );
        }
        if let Some(g) = &self.getter {
            o._put_prop(r, &"get".into(), g.clone(), true, true, true);
        }
        if let Some(s) = &self.setter {
            o._put_prop(r, &"set".into(), s.clone(), true, true, true);
        }
        Value::Object(o)
    }

    pub fn complete(&mut self) {
        if self.getter.is_none() && self.setter.is_none() {
            if self.value.is_none() {
                self.value = Some(Value::Undefined);
            }
            if self.writable == Flag::NotSet {
                self.writable = Flag::False;
            }
        } else {
            if self.getter.is_none() {
                self.getter = Some(Value::Undefined);
            }
            if self.setter.is_none() {
                self.setter = Some(Value::Undefined);
            }
        }
        if self.enumerable == Flag::NotSet {
            self.enumerable = Flag::False;
        }
        if self.configurable == Flag::NotSet {
            self.configurable = Flag::False;
        }
    }

    pub fn trace(&self, t: &mut Tracer) {
        t.opt_obj(self.js_descriptor);
        if let Some(v) = &self.value {
            t.value(v);
        }
        if let Some(v) = &self.getter {
            t.value(v);
        }
        if let Some(v) = &self.setter {
            t.value(v);
        }
    }
}

#[derive(Clone, Copy)]
pub struct FunctionCall<'a> {
    pub this: &'a Value,
    pub arguments: &'a [Value],
}

impl<'a> FunctionCall<'a> {
    pub fn new(this: &'a Value, arguments: &'a [Value]) -> FunctionCall<'a> {
        FunctionCall { this, arguments }
    }

    pub fn argument(&self, idx: usize) -> Value {
        match self.arguments.get(idx) {
            Some(v) => v.clone(),
            None => Value::Undefined,
        }
    }

    pub fn arg(&self, idx: usize) -> &Value {
        static UNDEFINED: Value = Value::Undefined;
        self.arguments.get(idx).unwrap_or(&UNDEFINED)
    }
}

#[derive(Clone, Copy)]
pub struct ConstructorCall<'a> {
    pub this: ObjRef,
    pub arguments: &'a [Value],
    pub new_target: Option<ObjRef>,
}

impl ConstructorCall<'_> {
    pub fn argument(&self, idx: usize) -> Value {
        match self.arguments.get(idx) {
            Some(v) => v.clone(),
            None => Value::Undefined,
        }
    }
}

pub type CallImpl = fn(&mut Runtime, ObjRef, FunctionCall<'_>) -> JsResult<Value>;
pub type CtorImpl = fn(&mut Runtime, ObjRef, &[Value], Option<ObjRef>) -> JsResult<ObjRef>;

#[derive(Clone, Copy)]
pub struct CallFn {
    pub obj: ObjRef,
    pub f: CallImpl,
}

impl CallFn {
    pub fn call(self, rt: &mut Runtime, this: Value, args: &[Value]) -> JsResult<Value> {
        let r = (self.f)(rt, self.obj, FunctionCall::new(&this, args))?;
        rt.root(&r);
        Ok(r)
    }

    pub fn call_fc(self, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
        let r = (self.f)(rt, self.obj, call)?;
        rt.root(&r);
        Ok(r)
    }
}

#[derive(Clone, Copy)]
pub struct CtorFn {
    pub obj: ObjRef,
    pub f: CtorImpl,
}

impl CtorFn {
    pub fn construct(
        self,
        rt: &mut Runtime,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let r = (self.f)(rt, self.obj, args, new_target)?;
        rt.root_obj(r);
        Ok(r)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EnumerableFlag {
    #[default]
    Unknown,
    False,
    True,
}

#[derive(Clone, Debug, Default)]
pub struct PropIterItem {
    pub name: Value,
    pub value: Option<Value>,
    pub enumerable: EnumerableFlag,
}

pub trait PropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>>;
    fn trace(&self, t: &mut Tracer);
}

pub type IterNextFunc = Box<dyn PropIter>;

pub struct EmptyIter;

impl PropIter for EmptyIter {
    fn next(&mut self, _rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        Ok(None)
    }

    fn trace(&self, _t: &mut Tracer) {}
}

pub struct ObjectPropIter {
    pub o: ObjRef,
    pub prop_names: Rc<[unistring::String]>,
    pub idx: usize,
}

impl PropIter for ObjectPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        while self.idx < self.prop_names.len() {
            let name = &self.prop_names[self.idx];
            self.idx += 1;
            if let Some(prop) = rt.obj(self.o).base.values.get(name)
                && !prop.is_empty()
            {
                return Ok(Some(PropIterItem {
                    name: Value::String(string_value_from_raw(name)),
                    value: Some(prop.clone()),
                    enumerable: EnumerableFlag::Unknown,
                }));
            }
        }
        Ok(None)
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
    }
}

pub struct RecursivePropIter {
    pub o: ObjRef,
    pub cur: Option<IterNextFunc>,
    pub seen: FxHashSet<unistring::String>,
}

impl PropIter for RecursivePropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        loop {
            let item = match &mut self.cur {
                Some(cur) => cur.next(rt)?,
                None => None,
            };
            match item {
                None => {
                    if let Some(proto) = self.o.proto(rt)? {
                        self.cur = Some(proto.iterate_string_keys(rt)?);
                        self.o = proto;
                        continue;
                    }
                    self.cur = None;
                    return Ok(None);
                }
                Some(item) => {
                    let name = item.name.string(rt)?;
                    if self.seen.insert(name) {
                        return Ok(Some(item));
                    }
                }
            }
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        if let Some(c) = &self.cur {
            c.trace(t);
        }
    }
}

pub struct EnumerableIter {
    pub o: ObjRef,
    pub wrapped: IterNextFunc,
}

impl PropIter for EnumerableIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        loop {
            let Some(item) = self.wrapped.next(rt)? else {
                return Ok(None);
            };
            if item.enumerable == EnumerableFlag::False {
                continue;
            }
            if item.enumerable == EnumerableFlag::Unknown {
                let prop = match &item.value {
                    None => self.o.get_own_prop(rt, &item.name)?,
                    Some(v) => Some(v.clone()),
                };
                let Some(prop) = prop else {
                    continue;
                };
                if prop.is_empty() {
                    continue;
                }
                if let Value::Property(p) = &prop
                    && !p.enumerable
                {
                    continue;
                }
            }
            return Ok(Some(item));
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        self.wrapped.trace(t);
    }
}

pub fn enumerate_recursive(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
    let cur = o.iterate_string_keys(rt)?;
    Ok(Box::new(EnumerableIter {
        o,
        wrapped: Box::new(RecursivePropIter {
            o,
            cur: Some(cur),
            seen: FxHashSet::default(),
        }),
    }))
}

pub struct ObjectSymbolIter {
    pub o: ObjRef,
    pub it: crate::map::OrderedMapIter,
}

impl PropIter for ObjectSymbolIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        let it = &mut self.it;
        let Some(m) = &mut rt.obj_mut(self.o).base.sym_values else {
            return Ok(None);
        };
        match m.iter_next(it) {
            Some((k, v)) => Ok(Some(PropIterItem {
                name: k,
                value: Some(v),
                enumerable: EnumerableFlag::Unknown,
            })),
            None => Ok(None),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
    }
}

pub struct ObjectAllPropIter {
    pub o: ObjRef,
    pub cur_str: Option<IterNextFunc>,
    pub syms: Option<IterNextFunc>,
}

impl PropIter for ObjectAllPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if let Some(cur) = &mut self.cur_str {
            if let Some(item) = cur.next(rt)? {
                return Ok(Some(item));
            }
            self.cur_str = None;
            self.syms = Some(self.o.iterate_symbols(rt)?);
        }
        match &mut self.syms {
            Some(s) => s.next(rt),
            None => Ok(None),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        if let Some(c) = &self.cur_str {
            c.trace(t);
        }
        if let Some(c) = &self.syms {
            c.trace(t);
        }
    }
}

pub struct EnumPropertiesIter {
    pub o: ObjRef,
    pub wrapped: Option<IterNextFunc>,
}

impl PropIter for EnumPropertiesIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        while let Some(w) = &mut self.wrapped {
            let Some(mut item) = w.next(rt)? else {
                self.wrapped = None;
                break;
            };
            match &item.value {
                None => {
                    let v = self.o.get(rt, &item.name, None)?;
                    match v {
                        None => continue,
                        Some(v) => item.value = Some(v),
                    }
                }
                Some(Value::Property(p)) => {
                    let p = (**p).clone();
                    item.value = Some(p.get(rt, &Value::Object(self.o))?);
                }
                Some(_) => {}
            }
            return Ok(Some(item));
        }
        Ok(None)
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        if let Some(w) = &self.wrapped {
            w.trace(t);
        }
    }
}

pub fn iterate_enumerable_properties(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
    let keys = o.iterate_keys(rt)?;
    Ok(Box::new(EnumPropertiesIter {
        o,
        wrapped: Some(Box::new(EnumerableIter { o, wrapped: keys })),
    }))
}

pub fn iterate_enumerable_string_properties(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
    let keys = o.iterate_string_keys(rt)?;
    Ok(Box::new(EnumPropertiesIter {
        o,
        wrapped: Some(Box::new(EnumerableIter { o, wrapped: keys })),
    }))
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct PrivateEnvType {
    pub num_fields: u32,
    pub num_methods: u32,
}

#[derive(Clone, Debug)]
pub struct PrivateId {
    pub typ: Arc<PrivateEnvType>,
    pub name: unistring::String,
    pub idx: u32,
    pub is_method: bool,
}

impl PrivateId {
    pub fn go_string(&self) -> String {
        format!("#{}", self.name)
    }

    pub fn string(&self) -> unistring::String {
        crate::compiler::private_id_string(&self.name)
    }
}

pub type PrivateNames = FxHashMap<unistring::String, Rc<PrivateId>>;

#[derive(Debug, Default)]
pub struct PrivateEnv {
    pub instance_type: std::cell::RefCell<Option<Arc<PrivateEnvType>>>,
    pub static_type: std::cell::RefCell<Option<Arc<PrivateEnvType>>>,
    pub names: std::cell::RefCell<PrivateNames>,
    pub outer: Option<Rc<PrivateEnv>>,
}

#[derive(Clone, Debug, Default)]
pub struct PrivateElements {
    pub methods: Vec<Value>,
    pub fields: Vec<Value>,
}

pub type PrivateEnvKey = usize;

pub fn private_env_key(typ: &Arc<PrivateEnvType>) -> PrivateEnvKey {
    Arc::as_ptr(typ) as usize
}

#[derive(Default)]
pub struct PropNameSet {
    pub string_props: FxHashSet<unistring::String>,
    pub symbol_props: FxHashSet<SymRef>,
}

impl PropNameSet {
    pub fn add(&mut self, rt: &mut Runtime, prop: &Value) -> JsResult<()> {
        if let Value::Symbol(s) = prop {
            self.symbol_props.insert(*s);
        } else {
            let s = prop.string(rt)?;
            self.string_props.insert(s);
        }
        Ok(())
    }

    pub fn has(&self, rt: &mut Runtime, prop: &Value) -> JsResult<bool> {
        if let Value::Symbol(s) = prop {
            return Ok(self.symbol_props.contains(s));
        }
        let s = prop.string(rt)?;
        Ok(self.string_props.contains(&s))
    }

    pub fn delete(&mut self, rt: &mut Runtime, prop: &Value) -> JsResult<()> {
        if let Value::Symbol(s) = prop {
            self.symbol_props.remove(s);
        } else {
            let s = prop.string(rt)?;
            self.string_props.remove(&s);
        }
        Ok(())
    }

    pub fn size(&self) -> usize {
        self.string_props.len() + self.symbol_props.len()
    }
}

pub type UStr = unistring::String;

pub type GetStrImpl = fn(&mut Runtime, ObjRef, &UStr, Option<&Value>) -> JsResult<Option<Value>>;
pub type GetIdxImpl = fn(&mut Runtime, ObjRef, i64, Option<&Value>) -> JsResult<Option<Value>>;
pub type GetSymImpl = fn(&mut Runtime, ObjRef, SymRef, Option<&Value>) -> JsResult<Option<Value>>;
pub type SetForeignStrImpl =
    fn(&mut Runtime, ObjRef, &UStr, Value, &Value, bool) -> JsResult<(bool, bool)>;
pub type SetForeignIdxImpl =
    fn(&mut Runtime, ObjRef, i64, Value, &Value, bool) -> JsResult<(bool, bool)>;
pub type SetForeignSymImpl =
    fn(&mut Runtime, ObjRef, SymRef, Value, &Value, bool) -> JsResult<(bool, bool)>;
pub type KeysImpl = fn(&mut Runtime, ObjRef, bool, Vec<Value>) -> JsResult<Vec<Value>>;

pub struct ObjClass {
    pub sort_len: fn(&mut Runtime, ObjRef) -> JsResult<i64>,
    pub sort_get: fn(&mut Runtime, ObjRef, i64) -> JsResult<Value>,
    pub swap: fn(&mut Runtime, ObjRef, i64, i64) -> JsResult<()>,
    pub class_name: fn(&Runtime, ObjRef) -> &'static str,
    pub type_of: fn(&Runtime, ObjRef) -> JsString,
    pub get_str: GetStrImpl,
    pub get_idx: GetIdxImpl,
    pub get_sym: GetSymImpl,
    pub get_own_prop_str: fn(&mut Runtime, ObjRef, &UStr) -> JsResult<Option<Value>>,
    pub get_own_prop_idx: fn(&mut Runtime, ObjRef, i64) -> JsResult<Option<Value>>,
    pub get_own_prop_sym: fn(&mut Runtime, ObjRef, SymRef) -> JsResult<Option<Value>>,
    pub set_own_str: fn(&mut Runtime, ObjRef, &UStr, Value, bool) -> JsResult<bool>,
    pub set_own_idx: fn(&mut Runtime, ObjRef, i64, Value, bool) -> JsResult<bool>,
    pub set_own_sym: fn(&mut Runtime, ObjRef, SymRef, Value, bool) -> JsResult<bool>,
    pub set_foreign_str: SetForeignStrImpl,
    pub set_foreign_idx: SetForeignIdxImpl,
    pub set_foreign_sym: SetForeignSymImpl,
    pub has_property_str: fn(&mut Runtime, ObjRef, &UStr) -> JsResult<bool>,
    pub has_property_idx: fn(&mut Runtime, ObjRef, i64) -> JsResult<bool>,
    pub has_property_sym: fn(&mut Runtime, ObjRef, SymRef) -> JsResult<bool>,
    pub has_own_property_str: fn(&mut Runtime, ObjRef, &UStr) -> JsResult<bool>,
    pub has_own_property_idx: fn(&mut Runtime, ObjRef, i64) -> JsResult<bool>,
    pub has_own_property_sym: fn(&mut Runtime, ObjRef, SymRef) -> JsResult<bool>,
    pub define_own_property_str:
        fn(&mut Runtime, ObjRef, &UStr, PropertyDescriptor, bool) -> JsResult<bool>,
    pub define_own_property_idx:
        fn(&mut Runtime, ObjRef, i64, PropertyDescriptor, bool) -> JsResult<bool>,
    pub define_own_property_sym:
        fn(&mut Runtime, ObjRef, SymRef, PropertyDescriptor, bool) -> JsResult<bool>,
    pub delete_str: fn(&mut Runtime, ObjRef, &UStr, bool) -> JsResult<bool>,
    pub delete_idx: fn(&mut Runtime, ObjRef, i64, bool) -> JsResult<bool>,
    pub delete_sym: fn(&mut Runtime, ObjRef, SymRef, bool) -> JsResult<bool>,
    pub assert_callable: fn(&Runtime, ObjRef) -> Option<CallFn>,
    pub vm_call: fn(&mut Runtime, ObjRef, usize) -> JsResult<()>,
    pub assert_constructor: fn(&Runtime, ObjRef) -> Option<CtorFn>,
    pub proto: fn(&mut Runtime, ObjRef) -> JsResult<Option<ObjRef>>,
    pub set_proto: fn(&mut Runtime, ObjRef, Option<ObjRef>, bool) -> JsResult<bool>,
    pub has_instance: fn(&mut Runtime, ObjRef, &Value) -> JsResult<bool>,
    pub is_extensible: fn(&mut Runtime, ObjRef) -> JsResult<bool>,
    pub prevent_extensions: fn(&mut Runtime, ObjRef, bool) -> JsResult<bool>,
    pub equal: fn(&Runtime, ObjRef, ObjRef) -> bool,
    pub iterate_string_keys: fn(&mut Runtime, ObjRef) -> JsResult<IterNextFunc>,
    pub iterate_symbols: fn(&mut Runtime, ObjRef) -> JsResult<IterNextFunc>,
    pub iterate_keys: fn(&mut Runtime, ObjRef) -> JsResult<IterNextFunc>,
    pub string_keys: KeysImpl,
    pub symbols: KeysImpl,
    pub keys: KeysImpl,
    pub _put_prop: fn(&mut Runtime, ObjRef, &UStr, Value, bool, bool, bool) -> Value,
    pub _put_sym: fn(&mut Runtime, ObjRef, SymRef, Value),
    pub get_private_env: fn(&mut Runtime, ObjRef, &Arc<PrivateEnvType>, bool) -> JsResult<bool>,
}

pub const BASE_CLASS: ObjClass = ObjClass {
    sort_len: BaseObject::sort_len,
    sort_get: BaseObject::sort_get,
    swap: BaseObject::swap,
    class_name: BaseObject::class_name,
    type_of: BaseObject::type_of,
    get_str: BaseObject::get_str,
    get_idx: BaseObject::get_idx,
    get_sym: BaseObject::get_sym,
    get_own_prop_str: BaseObject::get_own_prop_str,
    get_own_prop_idx: BaseObject::get_own_prop_idx,
    get_own_prop_sym: BaseObject::get_own_prop_sym,
    set_own_str: BaseObject::set_own_str,
    set_own_idx: BaseObject::set_own_idx,
    set_own_sym: BaseObject::set_own_sym,
    set_foreign_str: BaseObject::set_foreign_str,
    set_foreign_idx: BaseObject::set_foreign_idx,
    set_foreign_sym: BaseObject::set_foreign_sym,
    has_property_str: BaseObject::has_property_str,
    has_property_idx: BaseObject::has_property_idx,
    has_property_sym: BaseObject::has_property_sym,
    has_own_property_str: BaseObject::has_own_property_str,
    has_own_property_idx: BaseObject::has_own_property_idx,
    has_own_property_sym: BaseObject::has_own_property_sym,
    define_own_property_str: BaseObject::define_own_property_str,
    define_own_property_idx: BaseObject::define_own_property_idx,
    define_own_property_sym: BaseObject::define_own_property_sym,
    delete_str: BaseObject::delete_str,
    delete_idx: BaseObject::delete_idx,
    delete_sym: BaseObject::delete_sym,
    assert_callable: BaseObject::assert_callable,
    vm_call: BaseObject::vm_call,
    assert_constructor: BaseObject::assert_constructor,
    proto: BaseObject::proto,
    set_proto: BaseObject::set_proto,
    has_instance: BaseObject::has_instance,
    is_extensible: BaseObject::is_extensible,
    prevent_extensions: BaseObject::prevent_extensions,
    equal: BaseObject::equal,
    iterate_string_keys: BaseObject::iterate_string_keys,
    iterate_symbols: BaseObject::iterate_symbols,
    iterate_keys: BaseObject::iterate_keys,
    string_keys: BaseObject::string_keys,
    symbols: BaseObject::symbols,
    keys: BaseObject::keys,
    _put_prop: BaseObject::_put_prop,
    _put_sym: BaseObject::_put_sym,
    get_private_env: BaseObject::get_private_env,
};

pub static BASE_OBJECT_CLASS: ObjClass = BASE_CLASS;

pub static GUARDED_OBJECT_CLASS: ObjClass = ObjClass {
    set_own_str: GuardedObject::set_own_str,
    define_own_property_str: GuardedObject::define_own_property_str,
    delete_str: GuardedObject::delete_str,
    ..BASE_CLASS
};

pub static PRIMITIVE_VALUE_OBJECT_CLASS: ObjClass = BASE_CLASS;

pub trait ObjData: std::any::Any {
    fn trace(&self, t: &mut Tracer);
    fn as_any(&self) -> &dyn std::any::Any;
    fn as_any_mut(&mut self) -> &mut dyn std::any::Any;
}

#[derive(Default)]
pub enum ObjExt {
    #[default]
    None,
    Primitive(Value),
    Guarded(FxHashSet<unistring::String>),
    Template(Box<crate::object_template::TemplatedObject>),
    JsFunc(Box<crate::func::BaseJsFuncObject>),
    NativeFunc(Box<crate::func::NativeFuncObject>),
    Generator(Box<crate::func::GeneratorObject>),
    Array(Box<crate::array::ArrayObject>),
    SparseArray(Box<crate::array_sparse::SparseArrayObject>),
    Arguments(Box<crate::object_args::ArgumentsObject>),
    StringObj(Box<crate::string::StringObject>),
    StringIter(Box<crate::string::StringIterObject>),
    Error(Box<crate::builtin_error::ErrorObject>),
    Dyn(Box<dyn ObjData>),
}

impl ObjExt {
    pub fn trace(&self, t: &mut Tracer) {
        match self {
            ObjExt::None | ObjExt::Guarded(_) => {}
            ObjExt::Primitive(v) => t.value(v),
            ObjExt::Template(d) => d.trace(t),
            ObjExt::JsFunc(d) => d.trace(t),
            ObjExt::NativeFunc(d) => d.trace(t),
            ObjExt::Generator(d) => d.trace(t),
            ObjExt::Array(d) => d.trace(t),
            ObjExt::SparseArray(d) => d.trace(t),
            ObjExt::Arguments(d) => d.trace(t),
            ObjExt::StringObj(d) => d.trace(t),
            ObjExt::StringIter(d) => d.trace(t),
            ObjExt::Error(d) => d.trace(t),
            ObjExt::Dyn(d) => d.trace(t),
        }
    }
}

#[derive(Clone, Default)]
pub struct BaseObject {
    pub class: &'static str,
    pub prototype: Option<ObjRef>,
    pub extensible: bool,
    pub values: FxHashMap<unistring::String, Value>,
    pub prop_names: Vec<unistring::String>,
    pub last_sorted_prop_len: usize,
    pub idx_prop_count: usize,
    pub sym_values: Option<OrderedMap>,
    pub private_elements: Option<FxHashMap<PrivateEnvKey, PrivateElements>>,
}

impl Trace for BaseObject {
    fn trace(&self, t: &mut Tracer) {
        t.opt_obj(self.prototype);
        for v in self.values.values() {
            t.value(v);
        }
        if let Some(m) = &self.sym_values {
            m.trace(t);
        }
        if let Some(p) = &self.private_elements {
            for e in p.values() {
                t.values(&e.methods);
                t.values(&e.fields);
            }
        }
    }
}

pub fn idx_string(idx: i64) -> unistring::String {
    let mut buf = [0u8; 20];
    let mut i = buf.len();
    let mut n = idx.unsigned_abs();
    loop {
        i -= 1;
        buf[i] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if idx < 0 {
        i -= 1;
        buf[i] = b'-';
    }
    unistring::String::from_bytes(&buf[i..])
}

pub fn to_idx(v: i64) -> u32 {
    if v >= 0 && v < u32::MAX as i64 {
        return v as u32;
    }
    u32::MAX
}

impl BaseObject {
    pub fn new(class: &'static str, proto: Option<ObjRef>) -> BaseObject {
        BaseObject {
            class,
            prototype: proto,
            extensible: true,
            ..BaseObject::default()
        }
    }

    pub fn class_name(rt: &Runtime, o: ObjRef) -> &'static str {
        rt.obj(o).base.class
    }

    pub fn type_of(_rt: &Runtime, _o: ObjRef) -> JsString {
        STRING_OBJECT_C.clone()
    }

    pub fn has_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        if o.has_own_property_str(rt, name)? {
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            return proto.has_property_str(rt, name);
        }
        Ok(false)
    }

    pub fn has_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        o.has_property_str(rt, &idx_string(idx))
    }

    pub fn has_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
        if BaseObject::has_own_property_sym(rt, o, s)? {
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            return proto.has_property_sym(rt, s);
        }
        Ok(false)
    }

    pub fn get_with_own_prop(
        rt: &mut Runtime,
        o: ObjRef,
        prop: Option<Value>,
        p: &Value,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let prop = prop.filter(|v| !v.is_empty());
        let Some(prop) = prop else {
            if let Some(proto) = rt.obj(o).base.prototype {
                return match receiver {
                    None => proto.get(rt, p, Some(&Value::Object(o))),
                    Some(r) => proto.get(rt, p, Some(r)),
                };
            }
            return Ok(None);
        };
        if let Value::Property(prop) = &prop {
            return match receiver {
                None => prop.get(rt, &Value::Object(o)).map(Some),
                Some(r) => prop.get(rt, r).map(Some),
            };
        }
        Ok(Some(prop))
    }

    pub fn get_str_with_own_prop(
        rt: &mut Runtime,
        o: ObjRef,
        prop: Option<Value>,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let prop = prop.filter(|v| !v.is_empty());
        let Some(prop) = prop else {
            if let Some(proto) = rt.obj(o).base.prototype {
                return match receiver {
                    None => proto.get_str(rt, name, Some(&Value::Object(o))),
                    Some(r) => proto.get_str(rt, name, Some(r)),
                };
            }
            return Ok(None);
        };
        if let Value::Property(prop) = &prop {
            return match receiver {
                None => prop.get(rt, &Value::Object(o)).map(Some),
                Some(r) => prop.get(rt, r).map(Some),
            };
        }
        Ok(Some(prop))
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        o.get_str(rt, &idx_string(idx), receiver)
    }

    pub fn get_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let prop = BaseObject::get_own_prop_sym(rt, o, s)?;
        BaseObject::get_with_own_prop(rt, o, prop, &Value::Symbol(s), receiver)
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let obj = rt.obj(o);
        match obj.base.values.get(name) {
            None | Some(Value::Empty) => {
                if let Some(proto) = obj.base.prototype {
                    return match receiver {
                        None => proto.get_str(rt, name, Some(&Value::Object(o))),
                        Some(r) => proto.get_str(rt, name, Some(r)),
                    };
                }
                Ok(None)
            }
            Some(Value::Property(prop)) => {
                if let Some(g) = prop.getter_func {
                    let this = match receiver {
                        None => Value::Object(o),
                        Some(r) => r.clone(),
                    };
                    return property_get(rt, g, &this).map(Some);
                }
                if prop.value.is_empty() {
                    return Ok(Some(Value::Undefined));
                }
                Ok(Some(prop.value.clone()))
            }
            Some(v) => Ok(Some(v.clone())),
        }
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        o.get_own_prop_str(rt, &idx_string(idx))
    }

    pub fn get_own_prop_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<Option<Value>> {
        Ok(match &rt.obj(o).base.sym_values {
            Some(m) => m.get(&Value::Symbol(s)).cloned(),
            None => None,
        })
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        Ok(rt
            .obj(o)
            .base
            .values
            .get(name)
            .filter(|v| !v.is_empty())
            .cloned())
    }

    pub fn check_delete_prop(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        configurable: bool,
        throw: bool,
    ) -> JsResult<bool> {
        if !configurable {
            if throw {
                let s = rt.objectproto_to_string(FunctionCall::new(&Value::Object(o), &[]))?;
                let s = s.go_string(rt)?;
                return Err(Throw::Value(rt.new_type_error(&format!(
                    "Cannot delete property '{name}' of {s}"
                ))));
            }
            return Ok(false);
        }
        Ok(true)
    }

    pub fn check_delete(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: &Value,
        throw: bool,
    ) -> JsResult<bool> {
        if let Value::Property(p) = val {
            return BaseObject::check_delete_prop(rt, o, name, p.configurable, throw);
        }
        Ok(true)
    }

    pub fn _delete(&mut self, name: &UStr) {
        self.values.remove(name);
        if let Some(i) = self.prop_names.iter().position(|n| n == name) {
            self.prop_names.remove(i);
            if i < self.last_sorted_prop_len {
                self.last_sorted_prop_len -= 1;
                if i < self.idx_prop_count {
                    self.idx_prop_count -= 1;
                }
            }
        }
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        o.delete_str(rt, &idx_string(idx), throw)
    }

    pub fn delete_sym(rt: &mut Runtime, o: ObjRef, s: SymRef, throw: bool) -> JsResult<bool> {
        let key = Value::Symbol(s);
        let val = match &rt.obj(o).base.sym_values {
            Some(m) => m.get(&key).cloned(),
            None => None,
        };
        if let Some(val) = val {
            let name = rt.symbol_descriptive_string(s).string();
            if !BaseObject::check_delete(rt, o, &name, &val, throw)? {
                return Ok(false);
            }
            if let Some(m) = &mut rt.obj_mut(o).base.sym_values {
                m.remove(&key);
            }
        }
        Ok(true)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let val = rt.obj(o).base.values.get(name).cloned();
        if let Some(val) = val {
            if !BaseObject::check_delete(rt, o, name, &val, throw)? {
                return Ok(false);
            }
            rt.obj_mut(o).base._delete(name);
        }
        Ok(true)
    }

    pub fn set_proto(
        rt: &mut Runtime,
        o: ObjRef,
        proto: Option<ObjRef>,
        throw: bool,
    ) -> JsResult<bool> {
        let current = rt.obj(o).base.prototype;
        if current == proto {
            return Ok(true);
        }
        if !rt.obj(o).base.extensible {
            if throw {
                let s = Value::Object(o).go_string(rt)?;
                return Err(Throw::Value(
                    rt.new_type_error(&format!("{s} is not extensible")),
                ));
            }
            return Ok(false);
        }
        let mut p = proto;
        while let Some(pp) = p {
            if pp == o {
                return rt
                    .type_error_result(throw, "Cyclic __proto__ value")
                    .map(|_| false);
            }
            if rt.is_proxy(pp) {
                break;
            }
            p = pp.proto(rt)?;
        }
        rt.obj_mut(o).base.prototype = proto;
        Ok(true)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        match rt.obj_mut(o).base.values.get_mut(name) {
            Some(Value::Property(p)) if p.writable && p.setter_func.is_none() => {
                p.value = val;
                return Ok(true);
            }
            Some(Value::Property(_)) | Some(Value::Empty) | None => {}
            Some(slot) => {
                *slot = val;
                return Ok(true);
            }
        }
        let obj = rt.obj(o);
        match obj.base.values.get(name) {
            None | Some(Value::Empty) => {
                if let Some(proto) = obj.base.prototype {
                    let (res, handled) =
                        proto.set_foreign_str(rt, name, val.clone(), &Value::Object(o), throw)?;
                    if handled {
                        return Ok(res);
                    }
                }
                if !rt.obj(o).base.extensible {
                    if throw {
                        return Err(Throw::Value(rt.new_type_error(&format!(
                            "Cannot add property {name}, object is not extensible"
                        ))));
                    }
                    return Ok(false);
                }
                rt.obj_mut(o).base._put_new(name, val);
                Ok(true)
            }
            Some(Value::Property(prop)) => {
                if !prop.is_writable() {
                    if throw {
                        return Err(Throw::Value(rt.new_type_error(&format!(
                            "Cannot assign to read only property '{name}'"
                        ))));
                    }
                    return Ok(false);
                }
                if let Some(s) = prop.setter_func {
                    property_set(rt, s, &Value::Object(o), val)?;
                } else if let Some(Value::Property(p)) = rt.obj_mut(o).base.values.get_mut(name) {
                    p.value = val;
                }
                Ok(true)
            }
            Some(_) => {
                if let Some(slot) = rt.obj_mut(o).base.values.get_mut(name) {
                    *slot = val;
                }
                Ok(true)
            }
        }
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        o.set_own_str(rt, &idx_string(idx), val, throw)
    }

    pub fn set_own_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let key = Value::Symbol(name);
        let own_desc = match &rt.obj(o).base.sym_values {
            Some(m) => m.get(&key).cloned(),
            None => None,
        };
        match own_desc {
            None => {
                if let Some(proto) = rt.obj(o).base.prototype {
                    let (res, handled) =
                        proto.set_foreign_sym(rt, name, val.clone(), &Value::Object(o), throw)?;
                    if handled {
                        return Ok(res);
                    }
                }
                if !rt.obj(o).base.extensible {
                    if throw {
                        let s = Value::Symbol(name).go_string(rt)?;
                        return Err(Throw::Value(rt.new_type_error(&format!(
                            "Cannot add property {s}, object is not extensible"
                        ))));
                    }
                    return Ok(false);
                }
                rt.obj_mut(o)
                    .base
                    .sym_values
                    .get_or_insert_with(OrderedMap::new)
                    .set(key, val);
                Ok(true)
            }
            Some(Value::Property(prop)) => {
                if !prop.is_writable() {
                    if throw {
                        let s = Value::Symbol(name).go_string(rt)?;
                        return Err(Throw::Value(rt.new_type_error(&format!(
                            "Cannot assign to read only property '{s}'"
                        ))));
                    }
                    return Ok(false);
                }
                if let Some(s) = prop.setter_func {
                    property_set(rt, s, &Value::Object(o), val)?;
                } else if let Some(m) = &mut rt.obj_mut(o).base.sym_values
                    && let Some(Value::Property(p)) = m.get_mut(&key)
                {
                    p.value = val;
                }
                Ok(true)
            }
            Some(_) => {
                if let Some(m) = &mut rt.obj_mut(o).base.sym_values {
                    m.set(key, val);
                }
                Ok(true)
            }
        }
    }

    pub fn _set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        prop: Option<&Value>,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        match prop.filter(|p| !p.is_empty()) {
            Some(prop) => {
                if let Value::Property(prop) = prop {
                    if !prop.is_writable() {
                        if throw {
                            return Err(Throw::Value(rt.new_type_error(&format!(
                                "Cannot assign to read only property '{name}'"
                            ))));
                        }
                        return Ok((false, true));
                    }
                    if let Some(s) = prop.setter_func {
                        property_set(rt, s, receiver, val)?;
                        return Ok((true, true));
                    }
                }
            }
            None => {
                if let Some(proto) = rt.obj(o).base.prototype {
                    if !matches!(receiver, Value::Object(r) if *r == proto) {
                        return proto.set_foreign_str(rt, name, val, receiver, throw);
                    }
                    return Ok((proto.set_own_str(rt, name, val, throw)?, true));
                }
            }
        }
        Ok((false, false))
    }

    pub fn _set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        prop: Option<&Value>,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        match prop.filter(|p| !p.is_empty()) {
            Some(prop) => {
                if let Value::Property(prop) = prop {
                    if !prop.is_writable() {
                        if throw {
                            return Err(Throw::Value(rt.new_type_error(&format!(
                                "Cannot assign to read only property '{idx}'"
                            ))));
                        }
                        return Ok((false, true));
                    }
                    if let Some(s) = prop.setter_func {
                        property_set(rt, s, receiver, val)?;
                        return Ok((true, true));
                    }
                }
            }
            None => {
                if let Some(proto) = rt.obj(o).base.prototype {
                    if !matches!(receiver, Value::Object(r) if *r == proto) {
                        return proto.set_foreign_idx(rt, idx, val, receiver, throw);
                    }
                    return Ok((proto.set_own_idx(rt, idx, val, throw)?, true));
                }
            }
        }
        Ok((false, false))
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = rt.obj(o).base.values.get(name).cloned();
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        name: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        if to_idx(name) != u32::MAX {
            let obj = rt.obj_mut(o);
            obj.base.ensure_prop_order();
            if obj.base.idx_prop_count == 0 {
                let prop = Value::Int(name);
                return BaseObject::_set_foreign_idx(
                    rt,
                    o,
                    name,
                    Some(&prop),
                    Value::Undefined,
                    receiver,
                    throw,
                );
            }
        }
        o.set_foreign_str(rt, &idx_string(name), val, receiver, throw)
    }

    pub fn set_foreign_sym(
        rt: &mut Runtime,
        o: ObjRef,
        name: SymRef,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let key = Value::Symbol(name);
        let prop = match &rt.obj(o).base.sym_values {
            Some(m) => m.get(&key).cloned(),
            None => None,
        };
        match prop {
            Some(prop) => {
                if let Value::Property(prop) = &prop {
                    if !prop.is_writable() {
                        if throw {
                            let s = key.go_string(rt)?;
                            return Err(Throw::Value(rt.new_type_error(&format!(
                                "Cannot assign to read only property '{s}'"
                            ))));
                        }
                        return Ok((false, true));
                    }
                    if let Some(s) = prop.setter_func {
                        property_set(rt, s, receiver, val)?;
                        return Ok((true, true));
                    }
                }
            }
            None => {
                if let Some(proto) = rt.obj(o).base.prototype {
                    if !matches!(receiver, Value::Object(r) if *r == o) {
                        return proto.set_foreign_sym(rt, name, val, receiver, throw);
                    }
                    return Ok((proto.set_own_sym(rt, name, val, throw)?, true));
                }
            }
        }
        Ok((false, false))
    }

    pub fn has_own_property_sym(rt: &mut Runtime, o: ObjRef, s: SymRef) -> JsResult<bool> {
        Ok(match &rt.obj(o).base.sym_values {
            Some(m) => m.has(&Value::Symbol(s)),
            None => false,
        })
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        Ok(rt.obj(o).base.values.contains_key(name))
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        o.has_own_property_str(rt, &idx_string(idx))
    }

    pub fn _define_own_property(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        existing_value: Option<&Value>,
        descr: &PropertyDescriptor,
        throw: bool,
    ) -> JsResult<Option<Value>> {
        let getter_obj = descr.getter.as_ref().and_then(|g| g.as_object());
        let setter_obj = descr.setter.as_ref().and_then(|s| s.as_object());
        let mut existing: ValueProperty;
        let reject: bool;
        'check: {
            match existing_value.filter(|v| !v.is_empty()) {
                None => {
                    if !rt.obj(o).base.extensible {
                        if throw {
                            return Err(Throw::Value(rt.new_type_error(&format!(
                                "Cannot define property {name}, object is not extensible"
                            ))));
                        }
                        return Ok(None);
                    }
                    existing = ValueProperty::default();
                }
                Some(ev) => {
                    existing = match ev {
                        Value::Property(p) => (**p).clone(),
                        v => ValueProperty {
                            writable: true,
                            enumerable: true,
                            configurable: true,
                            value: v.clone(),
                            ..ValueProperty::default()
                        },
                    };
                    if !existing.configurable {
                        if descr.configurable == Flag::True {
                            reject = true;
                            break 'check;
                        }
                        if descr.enumerable != Flag::NotSet
                            && descr.enumerable.bool() != existing.enumerable
                        {
                            reject = true;
                            break 'check;
                        }
                    }
                    if existing.accessor && descr.value.is_some()
                        || !existing.accessor && (getter_obj.is_some() || setter_obj.is_some())
                    {
                        if !existing.configurable {
                            reject = true;
                            break 'check;
                        }
                    } else if !existing.accessor {
                        if !existing.configurable && !existing.writable {
                            if descr.writable == Flag::True {
                                reject = true;
                                break 'check;
                            }
                            if let Some(v) = &descr.value
                                && !v.same_as(&existing.value)
                            {
                                reject = true;
                                break 'check;
                            }
                        }
                    } else if !existing.configurable
                        && (descr.getter.is_some() && existing.getter_func != getter_obj
                            || descr.setter.is_some() && existing.setter_func != setter_obj)
                    {
                        reject = true;
                        break 'check;
                    }
                }
            }
            reject = false;
        }
        if reject {
            if throw {
                return Err(Throw::Value(
                    rt.new_type_error(&format!("Cannot redefine property: {name}")),
                ));
            }
            return Ok(None);
        }

        if descr.writable == Flag::True
            && descr.enumerable == Flag::True
            && descr.configurable == Flag::True
            && let Some(v) = &descr.value
        {
            return Ok(Some(v.clone()));
        }

        if descr.writable != Flag::NotSet {
            existing.writable = descr.writable.bool();
        }
        if descr.enumerable != Flag::NotSet {
            existing.enumerable = descr.enumerable.bool();
        }
        if descr.configurable != Flag::NotSet {
            existing.configurable = descr.configurable.bool();
        }

        if let Some(v) = &descr.value {
            existing.value = v.clone();
            existing.getter_func = None;
            existing.setter_func = None;
        }

        if descr.value.is_some() || descr.writable != Flag::NotSet {
            existing.accessor = false;
        }

        if let Some(g) = &descr.getter {
            existing.getter_func = prop_getter(rt, g)?;
            existing.value = Value::Empty;
            existing.accessor = true;
        }

        if let Some(s) = &descr.setter {
            existing.setter_func = prop_setter(rt, s)?;
            existing.value = Value::Empty;
            existing.accessor = true;
        }

        if !existing.accessor && existing.value.is_empty() {
            existing.value = Value::Undefined;
        }

        Ok(Some(Value::Property(Box::new(existing))))
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let existing_val = rt.obj(o).base.values.get(name).cloned();
        if let Some(v) =
            BaseObject::_define_own_property(rt, o, name, existing_val.as_ref(), &descr, throw)?
        {
            let base = &mut rt.obj_mut(o).base;
            if existing_val.as_ref().is_none_or(|v| v.is_empty()) {
                base.prop_names.push(name.clone());
            }
            base.values.insert(name.clone(), v);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        o.define_own_property_str(rt, &idx_string(idx), desc, throw)
    }

    pub fn define_own_property_sym(
        rt: &mut Runtime,
        o: ObjRef,
        s: SymRef,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let key = Value::Symbol(s);
        let existing_val = match &rt.obj(o).base.sym_values {
            Some(m) => m.get(&key).cloned(),
            None => None,
        };
        let name = rt.symbol_descriptive_string(s).string();
        if let Some(v) =
            BaseObject::_define_own_property(rt, o, &name, existing_val.as_ref(), &descr, throw)?
        {
            rt.obj_mut(o)
                .base
                .sym_values
                .get_or_insert_with(OrderedMap::new)
                .set(key, v);
            return Ok(true);
        }
        Ok(false)
    }

    pub fn _put(&mut self, name: &UStr, v: Value) {
        if !self.values.contains_key(name) {
            self.prop_names.push(name.clone());
        }
        self.values.insert(name.clone(), v);
    }

    fn _put_new(&mut self, name: &UStr, v: Value) {
        if !self.values.contains_key(name) {
            self.prop_names.push(name.clone());
        }
        self.values.insert(name.clone(), v);
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
        let prop = value_prop(value, writable, enumerable, configurable);
        rt.obj_mut(o).base._put(name, prop.clone());
        prop
    }

    pub fn _put_sym(rt: &mut Runtime, o: ObjRef, s: SymRef, prop: Value) {
        rt.obj_mut(o)
            .base
            .sym_values
            .get_or_insert_with(OrderedMap::new)
            .set(Value::Symbol(s), prop);
    }

    pub fn get_private_env(
        rt: &mut Runtime,
        o: ObjRef,
        typ: &Arc<PrivateEnvType>,
        create: bool,
    ) -> JsResult<bool> {
        let key = private_env_key(typ);
        let exists = rt
            .obj(o)
            .base
            .private_elements
            .as_ref()
            .is_some_and(|m| m.contains_key(&key));
        if exists && create {
            return Err(Throw::Value(rt.new_type_error(
                "Private fields for the class have already been set",
            )));
        }
        if !exists && create {
            let env = PrivateElements {
                methods: Vec::new(),
                fields: vec![Value::Empty; typ.num_fields as usize],
            };
            rt.obj_mut(o)
                .base
                .private_elements
                .get_or_insert_with(FxHashMap::default)
                .insert(key, env);
            return Ok(true);
        }
        Ok(exists)
    }

    pub fn assert_callable(_rt: &Runtime, _o: ObjRef) -> Option<CallFn> {
        None
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, _n: usize) -> JsResult<()> {
        let s = Value::Object(o).to_string(rt)?.to_rust_string();
        Err(Throw::Value(
            rt.new_type_error(&format!("Not a function: {s}")),
        ))
    }

    pub fn assert_constructor(_rt: &Runtime, _o: ObjRef) -> Option<CtorFn> {
        None
    }

    pub fn proto(rt: &mut Runtime, o: ObjRef) -> JsResult<Option<ObjRef>> {
        Ok(rt.obj(o).base.prototype)
    }

    pub fn is_extensible(rt: &mut Runtime, o: ObjRef) -> JsResult<bool> {
        Ok(rt.obj(o).base.extensible)
    }

    pub fn prevent_extensions(rt: &mut Runtime, o: ObjRef, _throw: bool) -> JsResult<bool> {
        rt.obj_mut(o).base.extensible = false;
        Ok(true)
    }

    pub fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        let v = nil_safe(o.get_str(rt, &"length".into(), None)?);
        let l = crate::runtime::to_length(rt, &v)?;
        Ok(crate::runtime::to_int_strict(l))
    }

    pub fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        Ok(nil_safe(o.get_idx(rt, i, None)?))
    }

    pub fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        let x = nil_safe(o.get_idx(rt, i, None)?);
        let y = nil_safe(o.get_idx(rt, j, None)?);
        o.set_own_idx(rt, i, y, false)?;
        o.set_own_idx(rt, j, x, false)?;
        Ok(())
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let base = &mut rt.obj_mut(o).base;
        base.ensure_prop_order();
        let prop_names: Rc<[unistring::String]> = base.prop_names.clone().into();
        Ok(Box::new(ObjectPropIter {
            o,
            prop_names,
            idx: 0,
        }))
    }

    pub fn iterate_symbols(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        if rt.obj(o).base.sym_values.is_some() {
            return Ok(Box::new(ObjectSymbolIter {
                o,
                it: crate::map::OrderedMapIter::new(),
            }));
        }
        Ok(Box::new(EmptyIter))
    }

    pub fn iterate_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let cur = o.iterate_string_keys(rt)?;
        Ok(Box::new(ObjectAllPropIter {
            o,
            cur_str: Some(cur),
            syms: None,
        }))
    }

    pub fn equal(_rt: &Runtime, _o: ObjRef, _other: ObjRef) -> bool {
        false
    }

    pub fn ensure_prop_order(&mut self) {
        if self.last_sorted_prop_len < self.prop_names.len() {
            self.fix_prop_order();
        }
    }

    pub fn fix_prop_order(&mut self) {
        let names = &mut self.prop_names;
        for i in self.last_sorted_prop_len..names.len() {
            let idx = str_to_array_idx(&names[i]);
            if idx != u32::MAX {
                let count = self.idx_prop_count;
                let k = names[..count].partition_point(|n| str_to_array_idx(n) < idx);
                if k < i {
                    let name = names.remove(i);
                    names.insert(k, name);
                }
                self.idx_prop_count += 1;
            }
        }
        self.last_sorted_prop_len = names.len();
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut keys: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let base = &mut rt.obj_mut(o).base;
        base.ensure_prop_order();
        if all {
            for k in &base.prop_names {
                go_append(&mut keys, Value::String(string_value_from_raw(k)));
            }
        } else {
            for k in &base.prop_names {
                if let Some(Value::Property(p)) = base.values.get(k)
                    && !p.enumerable
                {
                    continue;
                }
                go_append(&mut keys, Value::String(string_value_from_raw(k)));
            }
        }
        Ok(keys)
    }

    pub fn symbols(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        if let Some(m) = &rt.obj(o).base.sym_values {
            for (k, v) in m.entries() {
                if !all
                    && let Value::Property(p) = v
                    && !p.enumerable
                {
                    continue;
                }
                go_append(&mut accum, k.clone());
            }
        }
        Ok(accum)
    }

    pub fn keys(rt: &mut Runtime, o: ObjRef, all: bool, accum: Vec<Value>) -> JsResult<Vec<Value>> {
        let accum = o.string_keys(rt, all, accum)?;
        BaseObject::symbols(rt, o, all, accum)
    }

    pub fn has_instance(rt: &mut Runtime, o: ObjRef, _v: &Value) -> JsResult<bool> {
        let s = Value::Object(o).to_string(rt)?.to_rust_string();
        Err(Throw::Value(rt.new_type_error(&format!(
            "Expecting a function in instanceof check, but got {s}"
        ))))
    }
}

pub struct GuardedObject;

impl GuardedObject {
    pub fn guard(rt: &mut Runtime, o: ObjRef, props: &[&str]) {
        let obj = rt.obj_mut(o);
        if let ObjExt::Guarded(g) = &mut obj.ext {
            for p in props {
                g.insert(unistring::String::new(p));
            }
        }
    }

    fn check(rt: &mut Runtime, o: ObjRef, p: &UStr) {
        let obj = rt.obj_mut(o);
        let hit = matches!(&obj.ext, ObjExt::Guarded(g) if g.contains(p));
        if hit {
            obj.class = &BASE_OBJECT_CLASS;
            obj.ext = ObjExt::None;
        }
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::set_own_str(rt, o, p, v, throw)?;
        if res {
            GuardedObject::check(rt, o, p);
        }
        Ok(res)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let res = BaseObject::define_own_property_str(rt, o, name, desc, throw)?;
        if res {
            GuardedObject::check(rt, o, name);
        }
        Ok(res)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let res = BaseObject::delete_str(rt, o, name, throw)?;
        if res {
            GuardedObject::check(rt, o, name);
        }
        Ok(res)
    }
}

pub fn to_method(rt: &mut Runtime, v: Option<Value>) -> JsResult<Option<CallFn>> {
    let Some(v) = v else {
        return Ok(None);
    };
    if v.is_undefined() || v.is_null() || v.is_empty() {
        return Ok(None);
    }
    if let Value::Object(obj) = &v
        && let Some(call) = obj.assert_callable(rt)
    {
        return Ok(Some(call));
    }
    let s = v.go_string(rt)?;
    Err(Throw::TypeError(format!("{s} is not a method")))
}

pub fn instance_of_operator(rt: &mut Runtime, o: &Value, c: ObjRef) -> JsResult<bool> {
    let h = c.get_sym(rt, SYM_HAS_INSTANCE, Some(&Value::Object(c)))?;
    if let Some(inst_of_handler) = to_method(rt, h)? {
        let r = inst_of_handler.call(rt, Value::Object(c), std::slice::from_ref(o))?;
        return Ok(r.to_boolean());
    }
    c.has_instance(rt, o)
}

impl ObjRef {
    #[inline]
    pub fn class(self, rt: &Runtime) -> &'static ObjClass {
        rt.obj(self).class
    }

    pub fn sort_len(self, rt: &mut Runtime) -> JsResult<i64> {
        (self.class(rt).sort_len)(rt, self)
    }

    pub fn sort_get(self, rt: &mut Runtime, i: i64) -> JsResult<Value> {
        let v = (self.class(rt).sort_get)(rt, self, i)?;
        rt.root(&v);
        Ok(v)
    }

    pub fn swap(self, rt: &mut Runtime, i: i64, j: i64) -> JsResult<()> {
        (self.class(rt).swap)(rt, self, i, j)
    }

    pub fn class_name(self, rt: &Runtime) -> &'static str {
        (self.class(rt).class_name)(rt, self)
    }

    pub fn type_of(self, rt: &Runtime) -> JsString {
        (self.class(rt).type_of)(rt, self)
    }

    pub fn get_str(
        self,
        rt: &mut Runtime,
        p: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let v = (self.class(rt).get_str)(rt, self, p, receiver)?;
        if let Some(v) = &v {
            rt.root(v);
        }
        Ok(v)
    }

    pub fn get_idx(
        self,
        rt: &mut Runtime,
        p: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let v = (self.class(rt).get_idx)(rt, self, p, receiver)?;
        if let Some(v) = &v {
            rt.root(v);
        }
        Ok(v)
    }

    pub fn get_sym(
        self,
        rt: &mut Runtime,
        p: SymRef,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let v = (self.class(rt).get_sym)(rt, self, p, receiver)?;
        if let Some(v) = &v {
            rt.root(v);
        }
        Ok(v)
    }

    pub fn get_own_prop_str(self, rt: &mut Runtime, p: &UStr) -> JsResult<Option<Value>> {
        (self.class(rt).get_own_prop_str)(rt, self, p)
    }

    pub fn get_own_prop_idx(self, rt: &mut Runtime, p: i64) -> JsResult<Option<Value>> {
        (self.class(rt).get_own_prop_idx)(rt, self, p)
    }

    pub fn get_own_prop_sym(self, rt: &mut Runtime, p: SymRef) -> JsResult<Option<Value>> {
        (self.class(rt).get_own_prop_sym)(rt, self, p)
    }

    pub fn set_own_str(self, rt: &mut Runtime, p: &UStr, v: Value, throw: bool) -> JsResult<bool> {
        (self.class(rt).set_own_str)(rt, self, p, v, throw)
    }

    pub fn set_own_idx(self, rt: &mut Runtime, p: i64, v: Value, throw: bool) -> JsResult<bool> {
        (self.class(rt).set_own_idx)(rt, self, p, v, throw)
    }

    pub fn set_own_sym(self, rt: &mut Runtime, p: SymRef, v: Value, throw: bool) -> JsResult<bool> {
        (self.class(rt).set_own_sym)(rt, self, p, v, throw)
    }

    pub fn set_foreign_str(
        self,
        rt: &mut Runtime,
        p: &UStr,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        (self.class(rt).set_foreign_str)(rt, self, p, v, receiver, throw)
    }

    pub fn set_foreign_idx(
        self,
        rt: &mut Runtime,
        p: i64,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        (self.class(rt).set_foreign_idx)(rt, self, p, v, receiver, throw)
    }

    pub fn set_foreign_sym(
        self,
        rt: &mut Runtime,
        p: SymRef,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        (self.class(rt).set_foreign_sym)(rt, self, p, v, receiver, throw)
    }

    pub fn has_property_str(self, rt: &mut Runtime, p: &UStr) -> JsResult<bool> {
        (self.class(rt).has_property_str)(rt, self, p)
    }

    pub fn has_property_idx(self, rt: &mut Runtime, p: i64) -> JsResult<bool> {
        (self.class(rt).has_property_idx)(rt, self, p)
    }

    pub fn has_property_sym(self, rt: &mut Runtime, p: SymRef) -> JsResult<bool> {
        (self.class(rt).has_property_sym)(rt, self, p)
    }

    pub fn has_own_property_str(self, rt: &mut Runtime, p: &UStr) -> JsResult<bool> {
        (self.class(rt).has_own_property_str)(rt, self, p)
    }

    pub fn has_own_property_idx(self, rt: &mut Runtime, p: i64) -> JsResult<bool> {
        (self.class(rt).has_own_property_idx)(rt, self, p)
    }

    pub fn has_own_property_sym(self, rt: &mut Runtime, p: SymRef) -> JsResult<bool> {
        (self.class(rt).has_own_property_sym)(rt, self, p)
    }

    pub fn define_own_property_str(
        self,
        rt: &mut Runtime,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        (self.class(rt).define_own_property_str)(rt, self, name, desc, throw)
    }

    pub fn define_own_property_idx(
        self,
        rt: &mut Runtime,
        name: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        (self.class(rt).define_own_property_idx)(rt, self, name, desc, throw)
    }

    pub fn define_own_property_sym(
        self,
        rt: &mut Runtime,
        name: SymRef,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        (self.class(rt).define_own_property_sym)(rt, self, name, desc, throw)
    }

    pub fn delete_str(self, rt: &mut Runtime, name: &UStr, throw: bool) -> JsResult<bool> {
        (self.class(rt).delete_str)(rt, self, name, throw)
    }

    pub fn delete_idx(self, rt: &mut Runtime, idx: i64, throw: bool) -> JsResult<bool> {
        (self.class(rt).delete_idx)(rt, self, idx, throw)
    }

    pub fn delete_sym(self, rt: &mut Runtime, s: SymRef, throw: bool) -> JsResult<bool> {
        (self.class(rt).delete_sym)(rt, self, s, throw)
    }

    pub fn assert_callable(self, rt: &Runtime) -> Option<CallFn> {
        (self.class(rt).assert_callable)(rt, self)
    }

    pub fn vm_call(self, rt: &mut Runtime, n: usize) -> JsResult<()> {
        (self.class(rt).vm_call)(rt, self, n)
    }

    pub fn assert_constructor(self, rt: &Runtime) -> Option<CtorFn> {
        (self.class(rt).assert_constructor)(rt, self)
    }

    pub fn proto(self, rt: &mut Runtime) -> JsResult<Option<ObjRef>> {
        (self.class(rt).proto)(rt, self)
    }

    pub fn set_proto(self, rt: &mut Runtime, proto: Option<ObjRef>, throw: bool) -> JsResult<bool> {
        (self.class(rt).set_proto)(rt, self, proto, throw)
    }

    pub fn has_instance(self, rt: &mut Runtime, v: &Value) -> JsResult<bool> {
        (self.class(rt).has_instance)(rt, self, v)
    }

    pub fn is_extensible(self, rt: &mut Runtime) -> JsResult<bool> {
        (self.class(rt).is_extensible)(rt, self)
    }

    pub fn prevent_extensions(self, rt: &mut Runtime, throw: bool) -> JsResult<bool> {
        (self.class(rt).prevent_extensions)(rt, self, throw)
    }

    pub fn equal(self, rt: &Runtime, other: ObjRef) -> bool {
        (self.class(rt).equal)(rt, self, other)
    }

    pub fn iterate_string_keys(self, rt: &mut Runtime) -> JsResult<IterNextFunc> {
        (self.class(rt).iterate_string_keys)(rt, self)
    }

    pub fn iterate_symbols(self, rt: &mut Runtime) -> JsResult<IterNextFunc> {
        (self.class(rt).iterate_symbols)(rt, self)
    }

    pub fn iterate_keys(self, rt: &mut Runtime) -> JsResult<IterNextFunc> {
        (self.class(rt).iterate_keys)(rt, self)
    }

    pub fn string_keys(
        self,
        rt: &mut Runtime,
        all: bool,
        accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        (self.class(rt).string_keys)(rt, self, all, accum)
    }

    pub fn symbols(self, rt: &mut Runtime, all: bool, accum: Vec<Value>) -> JsResult<Vec<Value>> {
        (self.class(rt).symbols)(rt, self, all, accum)
    }

    pub fn keys(self, rt: &mut Runtime, all: bool, accum: Vec<Value>) -> JsResult<Vec<Value>> {
        (self.class(rt).keys)(rt, self, all, accum)
    }

    pub fn _put_prop(
        self,
        rt: &mut Runtime,
        name: &UStr,
        value: Value,
        writable: bool,
        enumerable: bool,
        configurable: bool,
    ) -> Value {
        (self.class(rt)._put_prop)(rt, self, name, value, writable, enumerable, configurable)
    }

    pub fn _put_sym(self, rt: &mut Runtime, s: SymRef, prop: Value) {
        (self.class(rt)._put_sym)(rt, self, s, prop)
    }

    pub fn get_private_env(
        self,
        rt: &mut Runtime,
        typ: &Arc<PrivateEnvType>,
        create: bool,
    ) -> JsResult<bool> {
        (self.class(rt).get_private_env)(rt, self, typ, create)
    }

    pub fn try_primitive(self, rt: &mut Runtime, method_name: &str) -> JsResult<Option<Value>> {
        if let Some(Value::Object(method)) = self.get_str(rt, &method_name.into(), None)?
            && let Some(call) = method.assert_callable(rt)
        {
            let v = call.call(rt, Value::Object(self), &[])?;
            if !v.is_object() {
                return Ok(Some(v));
            }
        }
        Ok(None)
    }

    pub fn ordinary_to_primitive_number(self, rt: &mut Runtime) -> JsResult<Value> {
        if let Some(v) = self.try_primitive(rt, "valueOf")? {
            return Ok(v);
        }
        if let Some(v) = self.try_primitive(rt, "toString")? {
            return Ok(v);
        }
        let s = rt.go_struct_dump(self);
        Err(Throw::Value(rt.new_type_error(&format!(
            "Could not convert {s} to primitive"
        ))))
    }

    pub fn ordinary_to_primitive_string(self, rt: &mut Runtime) -> JsResult<Value> {
        if let Some(v) = self.try_primitive(rt, "toString")? {
            return Ok(v);
        }
        if let Some(v) = self.try_primitive(rt, "valueOf")? {
            return Ok(v);
        }
        let s = rt.go_struct_dump(self);
        let t = rt.go_type_name(self);
        Err(Throw::Value(rt.new_type_error(&format!(
            "Could not convert {s} ({t}) to primitive"
        ))))
    }

    pub fn try_exotic_to_primitive(self, rt: &mut Runtime, hint: &str) -> JsResult<Option<Value>> {
        let m = self.get_sym(rt, SYM_TO_PRIMITIVE, None)?;
        if let Some(exotic_to_primitive) = to_method(rt, m)? {
            let ret = exotic_to_primitive.call(
                rt,
                Value::Object(self),
                &[Value::String(crate::string::ascii_string(hint))],
            )?;
            if !ret.is_object() {
                return Ok(Some(ret));
            }
            return Err(Throw::Value(
                rt.new_type_error("Cannot convert object to primitive value"),
            ));
        }
        Ok(None)
    }

    pub fn to_primitive_number(self, rt: &mut Runtime) -> JsResult<Value> {
        if let Some(v) = self.try_exotic_to_primitive(rt, HINT_NUMBER)? {
            return Ok(v);
        }
        self.ordinary_to_primitive_number(rt)
    }

    pub fn to_primitive_string(self, rt: &mut Runtime) -> JsResult<Value> {
        if let Some(v) = self.try_exotic_to_primitive(rt, HINT_STRING)? {
            return Ok(v);
        }
        self.ordinary_to_primitive_string(rt)
    }

    pub fn to_primitive(self, rt: &mut Runtime) -> JsResult<Value> {
        if let Some(v) = self.try_exotic_to_primitive(rt, HINT_DEFAULT)? {
            return Ok(v);
        }
        self.ordinary_to_primitive_number(rt)
    }

    pub fn get(
        self,
        rt: &mut Runtime,
        p: &Value,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        match p {
            Value::Int(i) => self.get_idx(rt, *i, receiver),
            Value::Symbol(s) => self.get_sym(rt, *s, receiver),
            _ => {
                let s = p.string(rt)?;
                self.get_str(rt, &s, receiver)
            }
        }
    }

    pub fn get_own_prop(self, rt: &mut Runtime, p: &Value) -> JsResult<Option<Value>> {
        match p {
            Value::Int(i) => self.get_own_prop_idx(rt, *i),
            Value::Symbol(s) => self.get_own_prop_sym(rt, *s),
            _ => {
                let s = p.string(rt)?;
                self.get_own_prop_str(rt, &s)
            }
        }
    }

    pub fn has_own_property(self, rt: &mut Runtime, p: &Value) -> JsResult<bool> {
        match p {
            Value::Int(i) => self.has_own_property_idx(rt, *i),
            Value::Symbol(s) => self.has_own_property_sym(rt, *s),
            _ => {
                let s = p.string(rt)?;
                self.has_own_property_str(rt, &s)
            }
        }
    }

    pub fn has_property(self, rt: &mut Runtime, p: &Value) -> JsResult<bool> {
        match p {
            Value::Int(i) => self.has_property_idx(rt, *i),
            Value::Symbol(s) => self.has_property_sym(rt, *s),
            _ => {
                let s = p.string(rt)?;
                self.has_property_str(rt, &s)
            }
        }
    }

    fn receiver_check(
        rt: &mut Runtime,
        prop: Option<Value>,
        name: &dyn Fn(&mut Runtime) -> JsResult<String>,
        throw: bool,
    ) -> JsResult<Option<bool>> {
        if let Some(Value::Property(desc)) = &prop {
            if desc.accessor {
                if throw {
                    let n = name(rt)?;
                    return Err(Throw::Value(
                        rt.new_type_error(&format!("Receiver property {n} is an accessor")),
                    ));
                }
                return Ok(Some(false));
            }
            if !desc.writable {
                if throw {
                    let n = name(rt)?;
                    return Err(Throw::Value(rt.new_type_error(&format!(
                        "Cannot assign to read only property '{n}'"
                    ))));
                }
                return Ok(Some(false));
            }
        }
        Ok(None)
    }

    fn receiver_not_object(rt: &mut Runtime, receiver: &Value, throw: bool) -> JsResult<bool> {
        if throw {
            let s = receiver.go_string(rt)?;
            return Err(Throw::Value(
                rt.new_type_error(&format!("Receiver is not an object: {s}")),
            ));
        }
        Ok(false)
    }

    pub fn set_str(
        self,
        rt: &mut Runtime,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<bool> {
        if matches!(receiver, Value::Object(r) if *r == self) {
            return self.set_own_str(rt, name, val, throw);
        }
        let (res, ok) = self.set_foreign_str(rt, name, val.clone(), receiver, throw)?;
        if ok {
            return Ok(res);
        }
        let Value::Object(robj) = receiver else {
            return ObjRef::receiver_not_object(rt, receiver, throw);
        };
        let robj = *robj;
        let prop = robj.get_own_prop_str(rt, name)?.filter(|v| !v.is_empty());
        if let Some(prop) = prop {
            let n = name.clone();
            if let Some(r) = ObjRef::receiver_check(rt, Some(prop), &|_| Ok(n.to_string()), throw)?
            {
                return Ok(r);
            }
            return robj.define_own_property_str(
                rt,
                name,
                PropertyDescriptor {
                    value: Some(val),
                    ..PropertyDescriptor::default()
                },
                throw,
            );
        }
        robj.define_own_property_str(
            rt,
            name,
            PropertyDescriptor {
                value: Some(val),
                writable: Flag::True,
                configurable: Flag::True,
                enumerable: Flag::True,
                ..PropertyDescriptor::default()
            },
            throw,
        )
    }

    pub fn set(
        self,
        rt: &mut Runtime,
        name: &Value,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<bool> {
        match name {
            Value::Int(i) => self.set_idx(rt, *i, val, receiver, throw),
            Value::Symbol(s) => self.set_sym(rt, *s, val, receiver, throw),
            _ => {
                let s = name.string(rt)?;
                self.set_str(rt, &s, val, receiver, throw)
            }
        }
    }

    pub fn set_own(
        self,
        rt: &mut Runtime,
        name: &Value,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        match name {
            Value::Int(i) => self.set_own_idx(rt, *i, val, throw),
            Value::Symbol(s) => self.set_own_sym(rt, *s, val, throw),
            _ => {
                let s = name.string(rt)?;
                self.set_own_str(rt, &s, val, throw)
            }
        }
    }

    pub fn set_idx(
        self,
        rt: &mut Runtime,
        name: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<bool> {
        if matches!(receiver, Value::Object(r) if *r == self) {
            return self.set_own_idx(rt, name, val, throw);
        }
        let (res, ok) = self.set_foreign_idx(rt, name, val.clone(), receiver, throw)?;
        if ok {
            return Ok(res);
        }
        let Value::Object(robj) = receiver else {
            return ObjRef::receiver_not_object(rt, receiver, throw);
        };
        let robj = *robj;
        let prop = robj.get_own_prop_idx(rt, name)?.filter(|v| !v.is_empty());
        if let Some(prop) = prop {
            if let Some(r) =
                ObjRef::receiver_check(rt, Some(prop), &|_| Ok(name.to_string()), throw)?
            {
                return Ok(r);
            }
            robj.define_own_property_idx(
                rt,
                name,
                PropertyDescriptor {
                    value: Some(val),
                    ..PropertyDescriptor::default()
                },
                throw,
            )?;
        } else {
            robj.define_own_property_idx(
                rt,
                name,
                PropertyDescriptor {
                    value: Some(val),
                    writable: Flag::True,
                    configurable: Flag::True,
                    enumerable: Flag::True,
                    ..PropertyDescriptor::default()
                },
                throw,
            )?;
        }
        Ok(true)
    }

    pub fn set_sym(
        self,
        rt: &mut Runtime,
        name: SymRef,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<bool> {
        if matches!(receiver, Value::Object(r) if *r == self) {
            return self.set_own_sym(rt, name, val, throw);
        }
        let (res, ok) = self.set_foreign_sym(rt, name, val.clone(), receiver, throw)?;
        if ok {
            return Ok(res);
        }
        let Value::Object(robj) = receiver else {
            return ObjRef::receiver_not_object(rt, receiver, throw);
        };
        let robj = *robj;
        let prop = robj.get_own_prop_sym(rt, name)?.filter(|v| !v.is_empty());
        if let Some(prop) = prop {
            if let Some(r) = ObjRef::receiver_check(
                rt,
                Some(prop),
                &|rt| Value::Symbol(name).go_string(rt),
                throw,
            )? {
                return Ok(r);
            }
            robj.define_own_property_sym(
                rt,
                name,
                PropertyDescriptor {
                    value: Some(val),
                    ..PropertyDescriptor::default()
                },
                throw,
            )?;
        } else {
            robj.define_own_property_sym(
                rt,
                name,
                PropertyDescriptor {
                    value: Some(val),
                    writable: Flag::True,
                    configurable: Flag::True,
                    enumerable: Flag::True,
                    ..PropertyDescriptor::default()
                },
                throw,
            )?;
        }
        Ok(true)
    }

    pub fn delete(self, rt: &mut Runtime, n: &Value, throw: bool) -> JsResult<bool> {
        match n {
            Value::Int(i) => self.delete_idx(rt, *i, throw),
            Value::Symbol(s) => self.delete_sym(rt, *s, throw),
            _ => {
                let s = n.string(rt)?;
                self.delete_str(rt, &s, throw)
            }
        }
    }

    pub fn define_own_property(
        self,
        rt: &mut Runtime,
        n: &Value,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        match n {
            Value::Int(i) => self.define_own_property_idx(rt, *i, desc, throw),
            Value::Symbol(s) => self.define_own_property_sym(rt, *s, desc, throw),
            _ => {
                let s = n.string(rt)?;
                self.define_own_property_str(rt, &s, desc, throw)
            }
        }
    }

    pub fn get_weak_refs(self, rt: &mut Runtime) -> &mut FxHashMap<u64, Value> {
        rt.obj_mut(self)
            .weak_refs
            .get_or_insert_with(|| Box::new(FxHashMap::default()))
    }

    pub fn get_id(self) -> u64 {
        u64::from(self.0) + 1
    }
}

impl Runtime {
    #[inline]
    pub fn obj(&self, o: ObjRef) -> &Object {
        self.heap.get(o)
    }

    #[inline]
    pub fn obj_mut(&mut self, o: ObjRef) -> &mut Object {
        self.heap.get_mut(o)
    }

    pub fn alloc_object(
        &mut self,
        class: &'static ObjClass,
        base: BaseObject,
        ext: ObjExt,
    ) -> ObjRef {
        let o = self.heap.alloc(Object {
            class,
            base,
            ext,
            weak_refs: None,
        });
        self.root_obj(o);
        o
    }

    pub fn reserve_object(&mut self) -> ObjRef {
        self.alloc_object(&BASE_OBJECT_CLASS, BaseObject::default(), ObjExt::None)
    }

    pub fn set_object_impl(
        &mut self,
        o: ObjRef,
        class: &'static ObjClass,
        base: BaseObject,
        ext: ObjExt,
    ) {
        let obj = self.obj_mut(o);
        obj.class = class;
        obj.base = base;
        obj.ext = ext;
    }

    pub fn ext_dyn<T: 'static>(&self, o: ObjRef) -> Option<&T> {
        match &self.obj(o).ext {
            ObjExt::Dyn(d) => d.as_any().downcast_ref::<T>(),
            _ => None,
        }
    }

    pub fn ext_dyn_mut<T: 'static>(&mut self, o: ObjRef) -> Option<&mut T> {
        match &mut self.obj_mut(o).ext {
            ObjExt::Dyn(d) => d.as_any_mut().downcast_mut::<T>(),
            _ => None,
        }
    }
}
