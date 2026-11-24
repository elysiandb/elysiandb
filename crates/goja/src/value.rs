use crate::builtin_bigint::string_to_big_int;
use crate::deps::big;
use crate::ftoa::{self, FToStrMode};
use crate::func::YieldMarker;
pub use crate::heap::{ObjRef, SymRef};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::{
    JsString, STRING_EMPTY, STRING_FALSE, STRING_NULL, STRING_TRUE, STRING_UNDEFINED, ascii_string,
};
use crate::string_ascii::AsciiString;
use crate::unistring;
use std::sync::Arc;

pub const MAX_INT: i64 = 1 << 53;

#[derive(Debug, Default)]
pub enum Value {
    #[default]
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(JsString),
    BigInt(Arc<big::Int>),
    Symbol(SymRef),
    Object(ObjRef),
    #[doc(hidden)]
    Empty,
    #[doc(hidden)]
    Property(Box<ValueProperty>),
    #[doc(hidden)]
    Unresolved(Box<Unresolved>),
    #[doc(hidden)]
    YieldMarker(YieldMarker),
}

impl Clone for Value {
    #[inline(always)]
    fn clone(&self) -> Value {
        match self {
            Value::Undefined => Value::Undefined,
            Value::Null => Value::Null,
            Value::Bool(b) => Value::Bool(*b),
            Value::Int(i) => Value::Int(*i),
            Value::Float(f) => Value::Float(*f),
            Value::String(s) => Value::String(s.clone()),
            Value::BigInt(b) => Value::BigInt(b.clone()),
            Value::Symbol(s) => Value::Symbol(*s),
            Value::Object(o) => Value::Object(*o),
            Value::Empty => Value::Empty,
            Value::Property(p) => Value::Property(p.clone()),
            Value::Unresolved(u) => Value::Unresolved(u.clone()),
            Value::YieldMarker(y) => Value::YieldMarker(*y),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Unresolved {
    pub name: unistring::String,
    pub member: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueError {
    TypeError(String),
    RangeError(String),
    ReferenceError(String),
    SyntaxError(String),
}

impl From<ValueError> for Throw {
    fn from(e: ValueError) -> Throw {
        match e {
            ValueError::TypeError(m) => Throw::TypeError(m),
            ValueError::RangeError(m) => Throw::RangeError(m),
            ValueError::ReferenceError(m) => Throw::ReferenceError(m),
            ValueError::SyntaxError(m) => Throw::SyntaxError(m),
        }
    }
}

pub const ERR_MIX_BIG_INT_TYPE: &str =
    "Cannot mix BigInt and other types, use explicit conversions";
pub const ERR_ACCESS_BEFORE_INIT: &str = "Cannot access a variable before initialization";
pub const ERR_ASSIGN_TO_CONST: &str = "Assignment to constant variable.";

#[derive(Clone, Debug)]
pub struct ValueProperty {
    pub value: Value,
    pub writable: bool,
    pub configurable: bool,
    pub enumerable: bool,
    pub accessor: bool,
    pub getter_func: Option<ObjRef>,
    pub setter_func: Option<ObjRef>,
}

impl Default for ValueProperty {
    fn default() -> ValueProperty {
        ValueProperty {
            value: Value::Empty,
            writable: false,
            configurable: false,
            enumerable: false,
            accessor: false,
            getter_func: None,
            setter_func: None,
        }
    }
}

impl ValueProperty {
    pub fn is_writable(&self) -> bool {
        self.writable || self.setter_func.is_some()
    }

    pub fn get(&self, rt: &mut Runtime, this: &Value) -> JsResult<Value> {
        match self.getter_func {
            None => {
                if self.value.is_empty() {
                    Ok(Value::Undefined)
                } else {
                    Ok(self.value.clone())
                }
            }
            Some(g) => property_get(rt, g, this),
        }
    }
}

#[inline]
fn plain_js_func(rt: &Runtime, f: ObjRef) -> bool {
    matches!(
        &rt.obj(f).ext,
        crate::object::ObjExt::JsFunc(d)
            if matches!(d.kind, crate::func::FuncKind::Func | crate::func::FuncKind::Method)
    )
}

fn plain_js_call(rt: &mut Runtime, f: ObjRef, this: &Value, args: &[Value]) -> JsResult<Value> {
    let this = if this.is_empty() {
        Value::Undefined
    } else {
        this.clone()
    };
    match crate::vm::js_func_call_ex(rt, f, args, Value::Empty, this)? {
        (_, Some(ex)) => Err(Throw::Exception(ex)),
        (r, None) => Ok(r),
    }
}

pub fn property_get(rt: &mut Runtime, getter: ObjRef, this: &Value) -> JsResult<Value> {
    if plain_js_func(rt, getter) {
        let ret = plain_js_call(rt, getter, this, &[])?;
        rt.root(&ret);
        return Ok(ret);
    }
    let Some(call) = getter.assert_callable(rt) else {
        return Ok(Value::Undefined);
    };
    let ret = call.call(rt, this.clone(), &[])?;
    rt.root(&ret);
    Ok(ret)
}

pub fn property_set(rt: &mut Runtime, setter: ObjRef, this: &Value, v: Value) -> JsResult<()> {
    if plain_js_func(rt, setter) {
        plain_js_call(rt, setter, this, std::slice::from_ref(&v))?;
        return Ok(());
    }
    if let Some(call) = setter.assert_callable(rt) {
        call.call(rt, this.clone(), &[v])?;
    }
    Ok(())
}

pub fn prop_getter(rt: &mut Runtime, v: &Value) -> JsResult<Option<ObjRef>> {
    if v.is_undefined() {
        return Ok(None);
    }
    if let Value::Object(obj) = v
        && obj.assert_callable(rt).is_some()
    {
        return Ok(Some(*obj));
    }
    let s = v.to_string(rt)?;
    Err(Throw::Value(rt.new_type_error(&format!(
        "Getter must be a function: {}",
        s.to_rust_string()
    ))))
}

pub fn prop_setter(rt: &mut Runtime, v: &Value) -> JsResult<Option<ObjRef>> {
    if v.is_undefined() {
        return Ok(None);
    }
    if let Value::Object(obj) = v
        && obj.assert_callable(rt).is_some()
    {
        return Ok(Some(*obj));
    }
    let s = v.to_string(rt)?;
    Err(Throw::Value(rt.new_type_error(&format!(
        "Setter must be a function: {}",
        s.to_rust_string()
    ))))
}

pub fn f_to_str(num: f64, mode: FToStrMode, prec: i64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);
    ftoa::f_to_str(num, mode, prec, &mut buf);
    buf
}

pub fn int_to_value(i: i64) -> Value {
    if (-MAX_INT..=MAX_INT).contains(&i) {
        return Value::Int(i);
    }
    Value::Float(i as f64)
}

pub fn float_to_int(f: f64) -> Option<i64> {
    if (f != 0.0 || !f.is_sign_negative())
        && !f.is_infinite()
        && f == f.trunc()
        && f >= -(MAX_INT as f64)
        && f <= MAX_INT as f64
    {
        return Some(f as i64);
    }
    None
}

pub fn float_to_value(f: f64) -> Value {
    if let Some(i) = float_to_int(f) {
        return int_to_value(i);
    }
    if f == 0.0 {
        return Value::Float(-0.0);
    }
    if f.is_nan() {
        return Value::Float(f64::NAN);
    }
    Value::Float(f)
}

pub fn new_big_int(i: big::Int) -> Value {
    Value::BigInt(Arc::new(i))
}

pub fn value_prop(value: Value, writable: bool, enumerable: bool, configurable: bool) -> Value {
    if writable && enumerable && configurable {
        return value;
    }
    Value::Property(Box::new(ValueProperty {
        value,
        writable,
        enumerable,
        configurable,
        accessor: false,
        getter_func: None,
        setter_func: None,
    }))
}

pub fn nil_safe(v: Option<Value>) -> Value {
    match v {
        Some(v) if !v.is_empty() => v,
        _ => Value::Undefined,
    }
}

fn float_string(f: f64) -> Vec<u8> {
    f_to_str(f, ftoa::MODE_STANDARD, 0)
}

impl Value {
    #[inline(always)]
    pub fn is_pod(&self) -> bool {
        !matches!(
            self,
            Value::String(_) | Value::BigInt(_) | Value::Property(_) | Value::Unresolved(_)
        )
    }

    #[inline(always)]
    pub fn set(&mut self, v: Value) {
        let old = std::mem::replace(self, v);
        if old.is_pod() {
            std::mem::forget(old);
        } else {
            drop(old);
        }
    }

    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn is_empty(&self) -> bool {
        matches!(self, Value::Empty)
    }

    pub fn is_object(&self) -> bool {
        matches!(self, Value::Object(_))
    }

    pub fn as_object(&self) -> Option<ObjRef> {
        match self {
            Value::Object(o) => Some(*o),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&JsString> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_property(&self) -> Option<&ValueProperty> {
        match self {
            Value::Property(p) => Some(p),
            _ => None,
        }
    }

    pub fn to_boolean(&self) -> bool {
        match self {
            Value::Undefined | Value::Null | Value::Empty | Value::Property(_) => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0 && !f.is_nan(),
            Value::String(s) => s.to_boolean(),
            Value::BigInt(b) => b.sign() != 0,
            Value::Symbol(_) | Value::Object(_) => true,
            Value::Unresolved(_) | Value::YieldMarker(_) => false,
        }
    }

    pub fn prim_to_number(&self) -> Result<Value, ValueError> {
        Ok(match self {
            Value::Undefined => Value::Float(f64::NAN),
            Value::Null => Value::Int(0),
            Value::Bool(b) => Value::Int(i64::from(*b)),
            Value::Int(_) | Value::Float(_) => self.clone(),
            Value::String(s) => s.to_number(),
            Value::BigInt(_) => {
                return Err(ValueError::TypeError(
                    "Cannot convert a BigInt value to a number".to_string(),
                ));
            }
            Value::Symbol(_) => {
                return Err(ValueError::TypeError(
                    "Cannot convert a Symbol value to a number".to_string(),
                ));
            }
            _ => Value::Float(f64::NAN),
        })
    }

    pub fn prim_to_float(&self) -> f64 {
        match self {
            Value::Undefined => f64::NAN,
            Value::Null => 0.0,
            Value::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            Value::Int(i) => *i as f64,
            Value::Float(f) => *f,
            Value::String(s) => s.to_float(),
            Value::BigInt(b) => b.float64().0,
            _ => f64::NAN,
        }
    }

    pub fn try_prim_to_float(&self) -> Result<f64, ValueError> {
        match self {
            Value::Symbol(_) => self.prim_to_number().map(|_| 0.0),
            _ => Ok(self.prim_to_float()),
        }
    }

    pub fn prim_to_integer(&self) -> i64 {
        match self {
            Value::Bool(b) => i64::from(*b),
            Value::Int(i) => *i,
            Value::Float(f) => float_to_int_clip(*f),
            Value::String(s) => match s.to_number() {
                Value::Int(i) => i,
                Value::Float(f) => float_to_int_clip(f),
                _ => 0,
            },
            Value::BigInt(b) => b.int64(),
            _ => 0,
        }
    }

    pub fn prim_to_string(&self) -> JsString {
        match self {
            Value::Undefined => STRING_UNDEFINED.clone(),
            Value::Null => STRING_NULL.clone(),
            Value::Bool(true) => STRING_TRUE.clone(),
            Value::Bool(false) => STRING_FALSE.clone(),
            Value::Int(i) => JsString::Ascii(int_to_ascii(*i)),
            Value::Float(f) => JsString::Ascii(AsciiString::new(&float_string(*f))),
            Value::String(s) => s.clone(),
            Value::BigInt(b) => JsString::Ascii(AsciiString::new(b.string().as_bytes())),
            _ => STRING_EMPTY.clone(),
        }
    }

    pub fn prim_string(&self) -> unistring::String {
        self.prim_to_string().string()
    }

    pub fn prim_equals(&self, other: &Value) -> bool {
        match self {
            Value::Undefined | Value::Null => matches!(other, Value::Undefined | Value::Null),
            Value::Bool(b) => {
                if let Value::Bool(o) = other {
                    return b == o;
                }
                other.prim_equals(&int_to_value(i64::from(*b)))
            }
            Value::Int(i) => match other {
                Value::Int(o) => i == o,
                Value::BigInt(o) => o.cmp(&big::Int::new_int(*i)) == 0,
                Value::Float(o) => *i as f64 == *o,
                Value::String(o) => o.to_number().prim_equals(self),
                Value::Bool(o) => *i == i64::from(*o),
                _ => false,
            },
            Value::Float(f) => match other {
                Value::Float(o) => f == o,
                Value::Int(o) => *f == *o as f64,
                Value::BigInt(o) => {
                    if f.is_infinite() || f.is_nan() {
                        return false;
                    }
                    match big::float_int(*f) {
                        (Some(i), _) if big::float_is_int(*f) => o.cmp(&i) == 0,
                        _ => false,
                    }
                }
                Value::String(_) | Value::Bool(_) => *f == other.prim_to_float(),
                _ => false,
            },
            Value::String(s) => s.equals(other),
            Value::BigInt(b) => match other {
                Value::BigInt(o) => b.cmp(o) == 0,
                Value::Int(o) => b.cmp(&big::Int::new_int(*o)) == 0,
                Value::Float(_) => other.prim_equals(self),
                Value::String(o) => match string_to_big_int(&o.to_trimmed_utf8()) {
                    Some(n) => n.cmp(b) == 0,
                    None => false,
                },
                Value::Bool(o) => b.int64() == i64::from(*o),
                _ => false,
            },
            Value::Symbol(s) => matches!(other, Value::Symbol(o) if o == s),
            Value::Object(r) => matches!(other, Value::Object(o) if o == r),
            _ => false,
        }
    }

    pub fn strict_equals(&self, other: &Value) -> bool {
        match self {
            Value::Undefined => matches!(other, Value::Undefined),
            Value::Null => matches!(other, Value::Null),
            Value::Bool(b) => matches!(other, Value::Bool(o) if o == b),
            Value::Int(i) => match other {
                Value::Int(o) => i == o,
                Value::Float(o) => *i as f64 == *o,
                _ => false,
            },
            Value::Float(f) => match other {
                Value::Float(o) => f == o,
                Value::Int(o) => *f == *o as f64,
                _ => false,
            },
            Value::String(s) => s.strict_equals(other),
            Value::BigInt(b) => matches!(other, Value::BigInt(o) if b.cmp(o) == 0),
            Value::Symbol(s) => matches!(other, Value::Symbol(o) if o == s),
            Value::Object(r) => matches!(other, Value::Object(o) if o == r),
            _ => false,
        }
    }

    pub fn same_as(&self, other: &Value) -> bool {
        match self {
            Value::Undefined => matches!(other, Value::Undefined),
            Value::Null => matches!(other, Value::Null),
            Value::Bool(b) => matches!(other, Value::Bool(o) if o == b),
            Value::Int(i) => match other {
                Value::Int(o) => i == o,
                Value::Float(o) => {
                    let ret = *i as f64 == *o;
                    if ret && *i == 0 {
                        return !o.is_sign_negative();
                    }
                    ret
                }
                _ => false,
            },
            Value::Float(f) => match other {
                Value::Float(o) => {
                    if f.is_nan() && o.is_nan() {
                        return true;
                    }
                    let mut ret = f == o;
                    if ret && *f == 0.0 {
                        ret = f.is_sign_negative() == o.is_sign_negative();
                    }
                    ret
                }
                Value::Int(o) => {
                    let mut ret = *f == *o as f64;
                    if ret && *f == 0.0 {
                        ret = !f.is_sign_negative();
                    }
                    ret
                }
                _ => false,
            },
            Value::String(s) => s.same_as(other),
            Value::BigInt(b) => matches!(other, Value::BigInt(o) if b.cmp(o) == 0),
            Value::Symbol(s) => matches!(other, Value::Symbol(o) if o == s),
            Value::Object(r) => matches!(other, Value::Object(o) if o == r),
            Value::Property(p) => match other {
                Value::Property(o) => std::ptr::eq(&**p, &**o),
                _ => false,
            },
            _ => false,
        }
    }

    fn unresolved_throw(&self) -> Throw {
        match self {
            Value::Unresolved(u) => Throw::ReferenceErrorName(u.name.clone()),
            _ => Throw::TypeError(String::new()),
        }
    }

    pub fn to_integer(&self, rt: &mut Runtime) -> JsResult<i64> {
        match self {
            Value::Object(o) => o.to_primitive_number(rt)?.to_number(rt)?.to_integer(rt),
            Value::Symbol(_) => Err(Throw::TypeError(
                "Cannot convert a Symbol value to a number".to_string(),
            )),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            _ => Ok(self.prim_to_integer()),
        }
    }

    pub fn to_string(&self, rt: &mut Runtime) -> JsResult<JsString> {
        match self {
            Value::Object(o) => o.to_primitive_string(rt)?.to_string(rt),
            Value::Symbol(_) => Err(Throw::TypeError(
                "Cannot convert a Symbol value to a string".to_string(),
            )),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            _ => Ok(self.prim_to_string()),
        }
    }

    pub fn string(&self, rt: &mut Runtime) -> JsResult<unistring::String> {
        match self {
            Value::Object(o) => o.to_primitive_string(rt)?.string(rt),
            Value::Symbol(s) => Ok(match rt.symbol_desc(*s) {
                Some(d) => d.string(),
                None => unistring::String::default(),
            }),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            _ => Ok(self.prim_string()),
        }
    }

    pub fn to_string_value(&self, rt: &mut Runtime) -> JsResult<Value> {
        match self {
            Value::Object(o) => o.to_primitive_string(rt)?.to_string_value(rt),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            Value::Property(_) => Ok(Value::Undefined),
            _ => Ok(self.clone()),
        }
    }

    pub fn go_string(&self, rt: &mut Runtime) -> JsResult<String> {
        match self {
            Value::Object(o) => o.to_primitive_string(rt)?.go_string(rt),
            Value::Symbol(s) => Ok(match rt.symbol_desc(*s) {
                Some(d) => d.to_rust_string(),
                None => String::new(),
            }),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            Value::Property(_) | Value::Empty => Ok(String::new()),
            _ => Ok(self.prim_to_string().to_rust_string()),
        }
    }

    pub fn to_float(&self, rt: &mut Runtime) -> JsResult<f64> {
        match self {
            Value::Object(o) => o.to_primitive_number(rt)?.to_float(rt),
            Value::Symbol(_) => Err(Throw::TypeError(
                "Cannot convert a Symbol value to a number".to_string(),
            )),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            _ => Ok(self.prim_to_float()),
        }
    }

    pub fn to_number(&self, rt: &mut Runtime) -> JsResult<Value> {
        match self {
            Value::Object(o) => o.to_primitive_number(rt)?.to_number(rt),
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            Value::Property(_) => Ok(Value::Empty),
            _ => Ok(self.prim_to_number()?),
        }
    }

    #[inline]
    pub fn to_object(&self, rt: &mut Runtime) -> JsResult<ObjRef> {
        if let Value::Object(o) = self {
            return Ok(*o);
        }
        self.to_object_slow(rt)
    }

    #[inline(never)]
    fn to_object_slow(&self, rt: &mut Runtime) -> JsResult<ObjRef> {
        match self {
            Value::Object(o) => Ok(*o),
            Value::Undefined | Value::Null | Value::Empty => Err(Throw::Value(
                rt.new_type_error("Cannot convert undefined or null to object"),
            )),
            Value::Bool(_) => {
                let proto = rt.get_boolean_prototype();
                Ok(rt.new_primitive_object(self.clone(), Some(proto), "Boolean"))
            }
            Value::Int(_) | Value::Float(_) => {
                let proto = rt.get_number_prototype();
                Ok(rt.new_primitive_object(self.clone(), Some(proto), "Number"))
            }
            Value::String(s) => Ok(rt.new_string_object(s.clone())),
            Value::BigInt(_) => {
                let proto = rt.get_big_int_prototype();
                Ok(rt.new_primitive_object(self.clone(), Some(proto), "BigInt"))
            }
            Value::Symbol(_) => {
                let proto = rt.get_symbol_prototype();
                Ok(rt.new_primitive_object(self.clone(), Some(proto), "Object"))
            }
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            Value::Property(_) | Value::YieldMarker(_) => Err(Throw::TypeError(
                "BUG: baseObject() is called on valueProperty".to_string(),
            )),
        }
    }

    #[inline]
    pub fn base_object(&self, rt: &mut Runtime) -> JsResult<Option<ObjRef>> {
        if let Value::Object(o) = self {
            return Ok(Some(*o));
        }
        self.base_object_slow(rt)
    }

    #[inline(never)]
    fn base_object_slow(&self, rt: &mut Runtime) -> JsResult<Option<ObjRef>> {
        match self {
            Value::Object(o) => Ok(Some(*o)),
            Value::Bool(_) => Ok(Some(rt.get_boolean_prototype())),
            Value::Int(_) | Value::Float(_) => Ok(Some(rt.get_number_prototype())),
            Value::String(s) => {
                let ss = rt.get_string_singleton()?;
                if let crate::object::ObjExt::StringObj(so) = &mut rt.obj_mut(ss).ext {
                    so.value = Some(s.clone());
                }
                crate::string::StringObject::set_length(rt, ss);
                Ok(Some(ss))
            }
            Value::BigInt(_) => Ok(Some(rt.get_big_int_prototype())),
            Value::Symbol(_) => {
                let proto = rt.get_symbol_prototype();
                Ok(Some(rt.new_primitive_object(
                    self.clone(),
                    Some(proto),
                    "Object",
                )))
            }
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            Value::Property(_) => Err(Throw::Value(
                rt.new_type_error("BUG: baseObject() is called on valueProperty"),
            )),
            _ => Ok(None),
        }
    }

    pub fn equals(&self, rt: &mut Runtime, other: &Value) -> JsResult<bool> {
        match self {
            Value::Object(o) => {
                if let Value::Object(other) = other {
                    return Ok(o == other || o.equal(rt, *other));
                }
                match other {
                    Value::Int(_)
                    | Value::Float(_)
                    | Value::BigInt(_)
                    | Value::String(_)
                    | Value::Symbol(_) => {
                        let p = o.to_primitive(rt)?;
                        p.equals(rt, other)
                    }
                    Value::Bool(b) => self.equals(rt, &Value::Int(i64::from(*b))),
                    _ => Ok(false),
                }
            }
            Value::Unresolved(_) => Err(self.unresolved_throw()),
            _ => {
                if let Value::Object(o) = other {
                    match self {
                        Value::Undefined | Value::Null | Value::Property(_) | Value::Empty => {
                            return Ok(false);
                        }
                        Value::Bool(b) => {
                            return int_to_value(i64::from(*b)).equals(rt, other);
                        }
                        _ => {
                            let p = o.to_primitive(rt)?;
                            return self.equals(rt, &p);
                        }
                    }
                }
                Ok(self.prim_equals(other))
            }
        }
    }

    pub fn to_property_key(&self, rt: &mut Runtime) -> JsResult<Value> {
        self.to_string_value(rt)
    }
}

pub fn int_to_ascii(i: i64) -> AsciiString {
    let mut buf = [0u8; 20];
    let mut pos = buf.len();
    let neg = i < 0;
    let mut n = i.unsigned_abs();
    loop {
        pos -= 1;
        buf[pos] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if neg {
        let mut out = [0u8; 21];
        out[0] = b'-';
        let len = buf.len() - pos;
        out[1..=len].copy_from_slice(&buf[pos..]);
        return AsciiString::new(&out[..=len]);
    }
    AsciiString::new(&buf[pos..])
}

pub fn float_to_int_clip(n: f64) -> i64 {
    if n.is_nan() {
        return 0;
    }
    if n >= i64::MAX as f64 {
        return i64::MAX;
    }
    if n <= i64::MIN as f64 {
        return i64::MIN;
    }
    n as i64
}

pub fn to_numeric(v: &Value) -> Result<Value, ValueError> {
    match v {
        Value::Int(_) | Value::BigInt(_) => Ok(v.clone()),
        Value::Float(f) => Ok(float_to_value(*f)),
        _ => v.prim_to_number(),
    }
}

#[cfg(target_arch = "x86_64")]
pub fn go_float_to_int(f: f64) -> i64 {
    let min = i64::MIN as f64;
    if f.is_nan() || !(min..-min).contains(&f) {
        i64::MIN
    } else {
        f as i64
    }
}

#[cfg(not(target_arch = "x86_64"))]
pub fn go_float_to_int(f: f64) -> i64 {
    f as i64
}

pub fn to_int32(v: &Value) -> Result<i32, ValueError> {
    match v.prim_to_number()? {
        Value::Int(i) => Ok(i as i32),
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => Ok(go_float_to_int(f) as i32),
        _ => Ok(0),
    }
}

pub fn to_uint32(v: &Value) -> Result<u32, ValueError> {
    match v.prim_to_number()? {
        Value::Int(i) => Ok(i as u32),
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => Ok(go_float_to_int(f) as u32),
        _ => Ok(0),
    }
}

pub fn func_name(rt: &Runtime, prefix: &str, n: &Value) -> JsString {
    let mut b = crate::string::StringBuilder::new();
    b.write_string(&ascii_string(prefix));
    if let Value::Symbol(sym) = n {
        if let Some(desc) = rt.symbol_desc(*sym) {
            b.write_rune('[' as i32);
            b.write_string(&desc);
            b.write_rune(']' as i32);
        }
    } else {
        b.write_string(&n.prim_to_string());
    }
    b.string()
}
