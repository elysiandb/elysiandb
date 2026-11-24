use crate::builtin_bigint::string_to_big_int;
use crate::deps::big;
use crate::ftoa::{self, FToStrMode};
use crate::string::{JsString, ascii_string};
use crate::string_ascii::AsciiString;
use crate::unistring;
use std::sync::Arc;

pub const MAX_INT: i64 = 1 << 53;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ObjRef(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct SymRef(pub u32);

#[derive(Clone, Debug)]
pub enum Value {
    Undefined,
    Null,
    Bool(bool),
    Int(i64),
    Float(f64),
    String(JsString),
    BigInt(Arc<big::Int>),
    Symbol(SymRef),
    Object(ObjRef),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValueError {
    TypeError(String),
    RangeError(String),
    ReferenceError(String),
    SyntaxError(String),
}

pub const ERR_MIX_BIG_INT_TYPE: &str =
    "Cannot mix BigInt and other types, use explicit conversions";

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

pub fn f_to_str(num: f64, mode: FToStrMode, prec: i64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(128);
    ftoa::f_to_str(num, mode, prec, &mut buf);
    buf
}

impl Value {
    pub fn is_undefined(&self) -> bool {
        matches!(self, Value::Undefined)
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn to_boolean(&self) -> bool {
        match self {
            Value::Undefined | Value::Null => false,
            Value::Bool(b) => *b,
            Value::Int(i) => *i != 0,
            Value::Float(f) => *f != 0.0 && !f.is_nan(),
            Value::String(s) => s.to_boolean(),
            Value::BigInt(b) => b.sign() != 0,
            Value::Symbol(_) | Value::Object(_) => true,
        }
    }

    pub fn to_number(&self) -> Result<Value, ValueError> {
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
            Value::Object(_) => {
                return Err(ValueError::TypeError(
                    "Cannot convert an object without a runtime".to_string(),
                ));
            }
        })
    }

    pub fn to_float(&self) -> f64 {
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
            Value::BigInt(_) | Value::Symbol(_) | Value::Object(_) => 0.0,
        }
    }

    pub fn try_to_float(&self) -> Result<f64, ValueError> {
        match self {
            Value::BigInt(_) | Value::Symbol(_) => self.to_number().map(|_| 0.0),
            _ => Ok(self.to_float()),
        }
    }

    pub fn to_integer(&self) -> i64 {
        match self {
            Value::Bool(b) => i64::from(*b),
            Value::Int(i) => *i,
            Value::Float(f) => float_to_int_clip(*f),
            _ => 0,
        }
    }

    pub fn to_string(&self) -> JsString {
        match self {
            Value::Undefined => ascii_string("undefined"),
            Value::Null => ascii_string("null"),
            Value::Bool(true) => ascii_string("true"),
            Value::Bool(false) => ascii_string("false"),
            Value::Int(i) => JsString::Ascii(AsciiString::new(i.to_string().as_bytes())),
            Value::Float(f) => {
                JsString::Ascii(AsciiString::new(&f_to_str(*f, ftoa::MODE_STANDARD, 0)))
            }
            Value::String(s) => s.clone(),
            Value::BigInt(b) => JsString::Ascii(AsciiString::new(b.string().as_bytes())),
            Value::Symbol(_) | Value::Object(_) => ascii_string(""),
        }
    }

    pub fn string(&self) -> unistring::String {
        self.to_string().string()
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
        }
    }

    pub fn equals(&self, other: &Value) -> bool {
        match self {
            Value::Undefined | Value::Null => matches!(other, Value::Undefined | Value::Null),
            Value::Bool(b) => {
                if let Value::Bool(o) = other {
                    return b == o;
                }
                other.equals(&int_to_value(i64::from(*b)))
            }
            Value::Int(i) => match other {
                Value::Int(o) => i == o,
                Value::BigInt(o) => o.cmp(&big::Int::new_int(*i)) == 0,
                Value::Float(o) => *i as f64 == *o,
                Value::String(o) => o.to_number().equals(self),
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
                Value::String(_) | Value::Bool(_) => *f == other.to_float(),
                _ => false,
            },
            Value::String(s) => s.equals(other),
            Value::BigInt(b) => match other {
                Value::BigInt(o) => b.cmp(o) == 0,
                Value::Int(o) => b.cmp(&big::Int::new_int(*o)) == 0,
                Value::Float(_) => other.equals(self),
                Value::String(o) => match string_to_big_int(&o.to_trimmed_utf8()) {
                    Some(n) => n.cmp(b) == 0,
                    None => false,
                },
                Value::Bool(o) => b.int64() == i64::from(*o),
                _ => false,
            },
            Value::Symbol(s) => matches!(other, Value::Symbol(o) if o == s),
            Value::Object(r) => matches!(other, Value::Object(o) if o == r),
        }
    }
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
        _ => v.to_number(),
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
    match v.to_number()? {
        Value::Int(i) => Ok(i as i32),
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => Ok(go_float_to_int(f) as i32),
        _ => Ok(0),
    }
}

pub fn to_uint32(v: &Value) -> Result<u32, ValueError> {
    match v.to_number()? {
        Value::Int(i) => Ok(i as u32),
        Value::Float(f) if !f.is_nan() && !f.is_infinite() => Ok(go_float_to_int(f) as u32),
        _ => Ok(0),
    }
}
