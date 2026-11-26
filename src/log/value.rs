use crate::gocompat::errors::Error as MessageError;
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError, MapKey, Value as JsonValue};
use crate::gocompat::time::{Duration, Time};
use std::borrow::Cow;
use std::collections::BTreeMap;
use std::fmt;

pub type Attr<'a> = (&'a str, Value<'a>);

#[derive(Clone)]
pub enum Value<'a> {
    Nil,
    Str(&'a [u8]),
    Int(i64),
    Uint(u64),
    Float(f64),
    Bool(bool),
    Duration(Duration),
    Time(Time),
    Display(&'a dyn fmt::Display),
    Strings(Vec<&'a [u8]>),
    Json(&'a dyn JsonText),
    Group(&'a [Attr<'a>]),
}

#[derive(Clone, Debug, PartialEq)]
pub enum AttrValue {
    Nil,
    Str(Vec<u8>),
    Int(i64),
    Uint(u64),
    Float(f64),
    Bool(bool),
    Strings(Vec<Vec<u8>>),
}

pub trait JsonText {
    fn json_text(&self) -> Vec<u8>;
}

impl<T: Encode + ?Sized> JsonText for T {
    fn json_text(&self) -> Vec<u8> {
        let mut e = EncodeState::new(false);
        match self.encode(&mut e) {
            Ok(()) => e.into_bytes(),
            Err(err) => err.to_string().into_bytes(),
        }
    }
}

pub trait ToValue {
    fn to_value(&self) -> Value<'_>;
}

pub fn display(v: &dyn fmt::Display) -> Value<'_> {
    Value::Display(v)
}

pub fn json<T: Encode>(v: &T) -> Value<'_> {
    Value::Json(v)
}

pub fn group<'a>(attrs: &'a [Attr<'a>]) -> Value<'a> {
    Value::Group(attrs)
}

#[macro_export]
macro_rules! kv {
    () => {
        &[]
    };
    ($($key:expr => $value:expr),+ $(,)?) => {
        &[$(($key, $crate::log::ToValue::to_value(&$value))),+]
    };
}

impl<'v> ToValue for Value<'v> {
    fn to_value(&self) -> Value<'_> {
        self.clone()
    }
}

impl<T: ToValue + ?Sized> ToValue for &T {
    fn to_value(&self) -> Value<'_> {
        (**self).to_value()
    }
}

impl<T: ToValue + ?Sized> ToValue for Box<T> {
    fn to_value(&self) -> Value<'_> {
        (**self).to_value()
    }
}

impl<T: ToValue> ToValue for Option<T> {
    fn to_value(&self) -> Value<'_> {
        match self {
            Some(v) => v.to_value(),
            None => Value::Nil,
        }
    }
}

impl ToValue for str {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self.as_bytes())
    }
}

impl ToValue for String {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self.as_bytes())
    }
}

impl ToValue for Cow<'_, str> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self.as_bytes())
    }
}

impl ToValue for [u8] {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for Vec<u8> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

impl ToValue for Cow<'_, [u8]> {
    fn to_value(&self) -> Value<'_> {
        Value::Str(self)
    }
}

macro_rules! signed_value {
    ($($t:ty),*) => {$(
        impl ToValue for $t {
            fn to_value(&self) -> Value<'_> {
                Value::Int(*self as i64)
            }
        }
    )*};
}

macro_rules! unsigned_value {
    ($($t:ty),*) => {$(
        impl ToValue for $t {
            fn to_value(&self) -> Value<'_> {
                Value::Uint(*self as u64)
            }
        }
    )*};
}

signed_value!(i8, i16, i32, i64, isize);
unsigned_value!(u8, u16, u32, u64, usize);

impl ToValue for f64 {
    fn to_value(&self) -> Value<'_> {
        Value::Float(*self)
    }
}

impl ToValue for f32 {
    fn to_value(&self) -> Value<'_> {
        Value::Float(f64::from(*self))
    }
}

impl ToValue for bool {
    fn to_value(&self) -> Value<'_> {
        Value::Bool(*self)
    }
}

impl ToValue for Duration {
    fn to_value(&self) -> Value<'_> {
        Value::Duration(*self)
    }
}

impl ToValue for std::time::Duration {
    fn to_value(&self) -> Value<'_> {
        Value::Duration(Duration(i64::try_from(self.as_nanos()).unwrap_or(i64::MAX)))
    }
}

impl ToValue for Time {
    fn to_value(&self) -> Value<'_> {
        Value::Time(self.clone())
    }
}

impl ToValue for [String] {
    fn to_value(&self) -> Value<'_> {
        Value::Strings(self.iter().map(|s| s.as_bytes()).collect())
    }
}

impl ToValue for Vec<String> {
    fn to_value(&self) -> Value<'_> {
        self.as_slice().to_value()
    }
}

impl ToValue for [&str] {
    fn to_value(&self) -> Value<'_> {
        Value::Strings(self.iter().map(|s| s.as_bytes()).collect())
    }
}

impl ToValue for Vec<&str> {
    fn to_value(&self) -> Value<'_> {
        self.as_slice().to_value()
    }
}

impl ToValue for [Vec<u8>] {
    fn to_value(&self) -> Value<'_> {
        Value::Strings(self.iter().map(Vec::as_slice).collect())
    }
}

impl ToValue for Vec<Vec<u8>> {
    fn to_value(&self) -> Value<'_> {
        self.as_slice().to_value()
    }
}

impl<'e> ToValue for dyn std::error::Error + 'e {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

impl<'e> ToValue for dyn std::error::Error + Send + Sync + 'e {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

impl ToValue for std::io::Error {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

impl ToValue for MessageError {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

impl ToValue for JsonError {
    fn to_value(&self) -> Value<'_> {
        Value::Display(self)
    }
}

impl ToValue for JsonValue {
    fn to_value(&self) -> Value<'_> {
        match self {
            JsonValue::Null => Value::Nil,
            JsonValue::Bool(b) => Value::Bool(*b),
            JsonValue::Number(f) => Value::Float(*f),
            JsonValue::Int(n) => Value::Int(*n),
            JsonValue::String(s) => Value::Str(s.as_bytes()),
            JsonValue::Array(_) | JsonValue::Object(_) | JsonValue::DocList(_) => Value::Json(self),
        }
    }
}

impl<K: MapKey, V: Encode> ToValue for BTreeMap<K, V> {
    fn to_value(&self) -> Value<'_> {
        Value::Json(self)
    }
}

impl Encode for AttrValue {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        match self {
            AttrValue::Nil => e.write_null(),
            AttrValue::Str(s) => e.write_string(s),
            AttrValue::Int(n) => e.write_i64(*n),
            AttrValue::Uint(n) => e.write_u64(*n),
            AttrValue::Float(f) => e.write_f64(*f)?,
            AttrValue::Bool(b) => e.write_bool(*b),
            AttrValue::Strings(list) => {
                e.write_byte(b'[');
                for (i, s) in list.iter().enumerate() {
                    if i > 0 {
                        e.write_byte(b',');
                    }
                    e.write_string(s);
                }
                e.write_byte(b']');
            }
        }
        Ok(())
    }
}
