use super::{DocValue, HostRef, check_host_object_property_descr, key_str};
use crate::date::DateObject;
use crate::func::{NativeCallable, NativeFn};
use crate::heap::{ObjRef, Tracer};
use crate::host::goslice_reflect::GoTypedSlice;
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_NUMBER, CLASS_OBJECT, EnumerableFlag, FunctionCall, IterNextFunc,
    ObjClass, ObjData, ObjExt, PropIter, PropIterItem, PropertyDescriptor, UStr,
};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::new_string_value;
use crate::value::{Value, ValueProperty, go_float_to_int};
use gocompat::time::{Location, Time};
use std::any::Any;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoKind {
    Struct,
    Int,
    Ptr,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GoIntKind {
    Int,
    Int64,
}

#[derive(Clone, Copy, Debug)]
pub struct GoMethod {
    pub name: &'static str,
    pub num_in: i64,
}

pub trait GoMethods: Any {
    fn kind(&self) -> GoKind {
        GoKind::Ptr
    }
    fn methods(&self) -> &'static [GoMethod];
    fn call(&self, rt: &mut Runtime, idx: usize, call: FunctionCall<'_>) -> JsResult<Value>;
    fn stringer(&self) -> Option<String> {
        None
    }
    fn value_of(&self) -> Option<Value> {
        None
    }
    fn go_int(&self) -> Option<(GoIntKind, i64)> {
        None
    }
    fn export_value(&self, this: &HostRef) -> DocValue {
        DocValue::Host(this.clone())
    }
    fn same(&self, _other: &dyn GoMethods) -> bool {
        false
    }
    fn marshal_json(&self, out: &mut Vec<u8>) -> Result<(), String> {
        out.extend_from_slice(b"{}");
        Ok(())
    }
    fn format_v(&self, out: &mut Vec<u8>) {
        match self.stringer() {
            Some(s) => out.extend_from_slice(s.as_bytes()),
            None => out.extend_from_slice(b"{}"),
        }
    }
    fn trace(&self, _t: &mut Tracer) {}
    fn as_any(&self) -> &dyn Any;
}

pub struct GoReflect<T: GoMethods + ?Sized = dyn GoMethods> {
    pub value: Rc<T>,
}

impl ObjData for GoReflect {
    fn trace(&self, t: &mut Tracer) {
        self.value.trace(t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub static GO_REFLECT_CLASS: ObjClass = ObjClass {
    get_str: GoReflect::get_str,
    get_own_prop_str: GoReflect::get_own_prop_str,
    set_own_str: GoReflect::set_own_str,
    set_foreign_str: GoReflect::set_foreign_str,
    set_foreign_idx: GoReflect::set_foreign_idx,
    has_own_property_str: GoReflect::has_own_property_str,
    define_own_property_str: GoReflect::define_own_property_str,
    delete_str: GoReflect::delete_str,
    iterate_string_keys: GoReflect::iterate_string_keys,
    string_keys: GoReflect::string_keys,
    equal: GoReflect::equal,
    ..BASE_CLASS
};

pub struct MethodCall {
    pub recv: HostRef,
    pub idx: usize,
}

impl NativeCallable for MethodCall {
    fn call(&self, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
        self.recv.call(rt, self.idx, call)
    }

    fn trace(&self, t: &mut Tracer) {
        self.recv.trace(t);
    }

    fn as_any(&self) -> Option<&dyn Any> {
        Some(self)
    }
}

impl Runtime {
    pub fn new_go_reflect(&mut self, value: HostRef) -> ObjRef {
        let (class, proto) = match value.kind() {
            GoKind::Int => (CLASS_NUMBER, Some(self.get_number_prototype())),
            _ => (CLASS_OBJECT, self.global.object_prototype),
        };
        self.alloc_object(
            &GO_REFLECT_CLASS,
            BaseObject::new(class, proto),
            ObjExt::Dyn(Box::new(GoReflect { value })),
        )
    }
}

pub fn reflect_value(rt: &Runtime, o: ObjRef) -> Option<HostRef> {
    rt.ext_dyn::<GoReflect>(o).map(|r| r.value.clone())
}

pub fn reflect_to_string(rt: &Runtime, o: ObjRef) -> Option<String> {
    reflect_value(rt, o)?.stringer()
}

pub fn reflect_value_of(rt: &Runtime, o: ObjRef) -> Option<Value> {
    reflect_value(rt, o)?.value_of()
}

pub fn reflect_number_value(rt: &Runtime, o: ObjRef) -> Option<Value> {
    let v = reflect_value(rt, o)?;
    if v.kind() != GoKind::Int {
        return None;
    }
    v.value_of()
}

pub fn reflect_number_to_string(rt: &Runtime, o: ObjRef) -> Option<Result<String, Value>> {
    let v = reflect_value(rt, o)?;
    if v.kind() != GoKind::Int {
        return None;
    }
    if let Some(s) = v.stringer() {
        return Some(Ok(s));
    }
    v.value_of().map(Err)
}

pub fn reflect_number_class(rt: &Runtime, o: ObjRef) -> bool {
    reflect_value(rt, o).is_some_and(|v| v.kind() == GoKind::Int)
}

pub fn reflect_marshal_json(rt: &Runtime, o: ObjRef) -> Option<Result<Vec<u8>, String>> {
    let v = reflect_value(rt, o)?;
    if v.kind() == GoKind::Int {
        return None;
    }
    let mut out = Vec::new();
    Some(v.marshal_json(&mut out).map(|_| out))
}

impl GoReflect {
    fn this(rt: &Runtime, o: ObjRef) -> HostRef {
        match rt.ext_dyn::<GoReflect>(o) {
            Some(r) => r.value.clone(),
            None => unreachable!("not a Go reflect object"),
        }
    }

    fn method_idx(rt: &Runtime, o: ObjRef, name: &str) -> Option<usize> {
        let v = GoReflect::this(rt, o);
        v.methods().binary_search_by(|m| m.name.cmp(name)).ok()
    }

    fn method_value(rt: &mut Runtime, o: ObjRef, idx: usize) -> Value {
        let recv = GoReflect::this(rt, o);
        let m = recv.methods()[idx];
        let f = rt.new_native_func(
            NativeFn::Dyn(Rc::new(MethodCall { recv, idx })),
            m.name,
            m.num_in,
        );
        Value::Object(f)
    }

    fn _has(rt: &Runtime, o: ObjRef, name: &str) -> bool {
        GoReflect::method_idx(rt, o, name).is_some()
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if let Some(idx) = GoReflect::method_idx(rt, o, &key_str(name)) {
            return Ok(Some(GoReflect::method_value(rt, o, idx)));
        }
        BaseObject::get_str(rt, o, name, receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        if let Some(idx) = GoReflect::method_idx(rt, o, &key_str(name)) {
            let value = GoReflect::method_value(rt, o, idx);
            return Ok(Some(Value::Property(Box::new(ValueProperty {
                value,
                writable: false,
                enumerable: true,
                configurable: false,
                accessor: false,
                getter_func: None,
                setter_func: None,
            }))));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let (res, ok) =
            BaseObject::_set_foreign_str(rt, o, name, None, val, &Value::Object(o), throw)?;
        if !ok {
            rt.type_error_result(
                throw,
                &format!("Cannot assign to property {name} of a host object"),
            )?;
            return Ok(false);
        }
        Ok(res)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = if GoReflect::_has(rt, o, &key_str(name)) {
            Some(Value::Bool(true))
        } else {
            None
        };
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        BaseObject::_set_foreign_idx(rt, o, idx, None, val, receiver, throw)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        Ok(
            GoReflect::_has(rt, o, &key_str(name))
                || BaseObject::has_own_property_str(rt, o, name)?,
        )
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        _o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if check_host_object_property_descr(rt, name, &descr, throw)? {
            rt.type_error_result(
                throw,
                &format!("Cannot define property '{name}' on a host object"),
            )?;
        }
        Ok(false)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        if GoReflect::_has(rt, o, &key_str(name)) {
            rt.type_error_result(
                throw,
                &format!("Cannot delete property {name} from a host object"),
            )?;
            return Ok(false);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let names = GoReflect::this(rt, o).methods();
        Ok(Box::new(GoReflectPropIter { o, names, idx: 0 }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        _all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        for m in GoReflect::this(rt, o).methods() {
            accum.push(Value::String(new_string_value(m.name.as_bytes())));
        }
        Ok(accum)
    }

    pub fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        let Some(b) = reflect_value(rt, other) else {
            return false;
        };
        let a = GoReflect::this(rt, o);
        if a.kind() != b.kind() {
            return false;
        }
        Rc::ptr_eq(&a, &b) || a.same(&*b)
    }
}

struct GoReflectPropIter {
    o: ObjRef,
    names: &'static [GoMethod],
    idx: usize,
}

impl PropIter for GoReflectPropIter {
    fn next(&mut self, _rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.idx < self.names.len() {
            let name = self.names[self.idx].name;
            self.idx += 1;
            return Ok(Some(PropIterItem {
                name: Value::String(new_string_value(name.as_bytes())),
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

pub const KIND_NUMBER: &str = "a number";
pub const KIND_STRING: &str = "a string";
pub const KIND_BOOLEAN: &str = "a boolean";
pub const KIND_OBJECT: &str = "an object";
pub const KIND_ARRAY: &str = "an array";
pub const KIND_FUNCTION: &str = "a function";

pub fn describe_value(rt: &Runtime, v: &Value) -> String {
    match v {
        Value::Undefined | Value::Empty => "undefined".to_string(),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Int(_) | Value::Float(_) | Value::BigInt(_) => v.prim_to_string().to_rust_string(),
        Value::String(s) => {
            let mut out = Vec::new();
            gocompat::json::append_string(&mut out, &s.go_string(), false);
            String::from_utf8_lossy(&out).into_owned()
        }
        Value::Symbol(s) => {
            let d = rt
                .symbol_desc(*s)
                .map(|d| d.go_string())
                .unwrap_or_default();
            let mut out = Vec::new();
            gocompat::json::append_string(&mut out, &d, false);
            String::from_utf8_lossy(&out).into_owned()
        }
        Value::Object(o) => {
            if o.assert_callable(rt).is_some() {
                return "a function".to_string();
            }
            if o.class_name(rt) == crate::object::CLASS_ARRAY {
                return "an array".to_string();
            }
            "an object".to_string()
        }
        _ => "undefined".to_string(),
    }
}

pub fn conversion_error(rt: &mut Runtime, i: usize, v: &Value, kind: &str) -> Throw {
    let d = describe_value(rt, v);
    Throw::Value(rt.new_type_error(&format!(
        "could not convert function call parameter {i}: could not convert {d} to {kind}"
    )))
}

pub fn arg_present(call: &FunctionCall<'_>, i: usize) -> Option<Value> {
    match call.arguments.get(i) {
        None | Some(Value::Undefined) | Some(Value::Null) | Some(Value::Empty) => None,
        Some(v) => Some(v.clone()),
    }
}

pub fn to_int64(rt: &mut Runtime, v: &Value) -> JsResult<i64> {
    match v.to_number(rt)? {
        Value::Int(i) => Ok(i),
        Value::Float(f) if f.is_finite() => Ok(go_float_to_int(f)),
        _ => Ok(0),
    }
}

pub fn reflect_int(rt: &Runtime, v: &Value, kind: GoIntKind) -> Option<i64> {
    let Value::Object(o) = v else {
        return None;
    };
    match reflect_value(rt, *o)?.go_int()? {
        (k, n) if k == kind => Some(n),
        _ => None,
    }
}

fn arg_int_kind(
    rt: &mut Runtime,
    call: &FunctionCall<'_>,
    i: usize,
    kind: GoIntKind,
) -> JsResult<i64> {
    let Some(v) = arg_present(call, i) else {
        return Ok(0);
    };
    if let Some(n) = reflect_int(rt, &v, kind) {
        return Ok(n);
    }
    to_int64(rt, &v)
}

pub fn arg_int(rt: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<i64> {
    arg_int_kind(rt, call, i, GoIntKind::Int)
}

pub fn arg_int64(rt: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<i64> {
    arg_int_kind(rt, call, i, GoIntKind::Int64)
}

pub fn arg_string(rt: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<String> {
    let Some(v) = arg_present(call, i) else {
        return Ok(String::new());
    };
    match &v {
        Value::String(s) => Ok(crate::export::go_string_of(s)),
        Value::Symbol(s) => Ok(rt
            .symbol_desc(*s)
            .map(|d| crate::export::go_string_of(&d))
            .unwrap_or_default()),
        other => {
            let s = other.to_string(rt)?;
            Ok(crate::export::go_string_of(&s))
        }
    }
}

pub fn arg_time(rt: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<Time> {
    let Some(v) = arg_present(call, i) else {
        return Ok(Time::default());
    };
    if let Value::Object(o) = &v {
        if let Some(h) = reflect_value(rt, *o)
            && let Some(t) = h.as_any().downcast_ref::<crate::host::gotime::GoTime>()
        {
            return Ok(t.0.borrow().clone());
        }
        if let Some(d) = rt.ext_dyn::<DateObject>(*o) {
            return Ok(d.time());
        }
        return Ok(Time::default());
    }
    if let Value::String(s) = &v {
        let (t, ok) = crate::date::date_parse(&s.go_string());
        if ok {
            return Ok(t);
        }
        let d = describe_value(rt, &v);
        return Err(Throw::Value(rt.new_type_error(&format!(
            "could not convert function call parameter {i}: could not convert string {d} to {KIND_OBJECT}"
        ))));
    }
    Err(conversion_error(rt, i, &v, KIND_OBJECT))
}

pub fn arg_location(
    rt: &mut Runtime,
    call: &FunctionCall<'_>,
    i: usize,
) -> JsResult<Option<Location>> {
    let Some(v) = arg_present(call, i) else {
        return Ok(None);
    };
    if let Value::Object(o) = &v {
        if let Some(h) = reflect_value(rt, *o)
            && let Some(l) = h.as_any().downcast_ref::<crate::host::gotime::GoLocation>()
        {
            return Ok(Some(l.0.clone()));
        }
        return Ok(Some(Location::fixed("UTC", 0)));
    }
    Err(conversion_error(rt, i, &v, KIND_OBJECT))
}

pub fn arg_bytes(rt: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<Vec<u8>> {
    let Some(v) = arg_present(call, i) else {
        return Ok(Vec::new());
    };
    match &v {
        Value::String(s) => Ok(s.go_string()),
        Value::Object(o) => {
            if let Some(b) = rt.ext_dyn::<GoTypedSlice<u8>>(*o) {
                return Ok(b.data.to_vec());
            }
            if let Some(b) = rt.export_to_bytes(&v)? {
                return Ok(b);
            }
            let o = *o;
            let len_v = o.get_str(rt, &"length".into(), None)?;
            let Some(len_v) = len_v.filter(|_| o.assert_callable(rt).is_none()) else {
                let d = describe_value(rt, &v);
                return Err(Throw::Value(rt.new_type_error(&format!(
                    "could not convert function call parameter {i}: cannot convert {d} to {KIND_ARRAY}"
                ))));
            };
            let n = crate::runtime::to_length(rt, &len_v)?;
            let mut out = Vec::with_capacity(n as usize);
            for j in 0..n {
                let e = o.get_idx(rt, j, None)?.unwrap_or(Value::Undefined);
                out.push(crate::runtime::to_uint8(rt, &e)?);
            }
            Ok(out)
        }
        _ => Err(conversion_error(rt, i, &v, KIND_ARRAY)),
    }
}

pub fn go_error(rt: &mut Runtime, msg: &str) -> Throw {
    Throw::Value(rt.new_go_error(msg))
}
