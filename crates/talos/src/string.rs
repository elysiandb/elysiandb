use crate::array::go_append;
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, EnumerableFlag, IterNextFunc, ObjClass, ObjExt, PropIter, PropIterItem,
    PropertyDescriptor, UStr, idx_string,
};
use crate::runtime::{JsResult, Runtime, Throw, str_to_go_idx};
use crate::string_ascii::AsciiString;
use crate::string_imported::ImportedString;
use crate::string_unicode::UnicodeString;
pub use crate::string_unicode::{StringBuilder, UnicodeStringBuilder};
use crate::unistring;
use crate::value::{Value, ValueProperty, int_to_value};
use std::sync::LazyLock;

pub const PROTO: &str = "__proto__";

#[derive(Clone, Debug)]
pub enum JsString {
    Ascii(AsciiString),
    Unicode(UnicodeString),
    Imported(ImportedString),
}

macro_rules! ascii_const {
    ($name:ident, $s:expr) => {
        pub static $name: LazyLock<JsString> = LazyLock::new(|| ascii_string($s));
    };
}

ascii_const!(STRING_TRUE, "true");
ascii_const!(STRING_FALSE, "false");
ascii_const!(STRING_NULL, "null");
ascii_const!(STRING_UNDEFINED, "undefined");
ascii_const!(STRING_OBJECT_C, "object");
ascii_const!(STRING_FUNCTION, "function");
ascii_const!(STRING_BOOLEAN, "boolean");
ascii_const!(STRING_STRING, "string");
ascii_const!(STRING_SYMBOL, "symbol");
ascii_const!(STRING_NUMBER, "number");
ascii_const!(STRING_BIG_INT, "bigint");
ascii_const!(STRING_NAN, "NaN");
ascii_const!(STRING_INFINITY, "Infinity");
ascii_const!(STRING_NEG_INFINITY, "-Infinity");
ascii_const!(STRING_BOUND_, "bound ");
ascii_const!(STRING_EMPTY, "");
ascii_const!(STRING_ERROR, "Error");
ascii_const!(STRING_AGGREGATE_ERROR, "AggregateError");
ascii_const!(STRING_TYPE_ERROR, "TypeError");
ascii_const!(STRING_REFERENCE_ERROR, "ReferenceError");
ascii_const!(STRING_SYNTAX_ERROR, "SyntaxError");
ascii_const!(STRING_RANGE_ERROR, "RangeError");
ascii_const!(STRING_EVAL_ERROR, "EvalError");
ascii_const!(STRING_URI_ERROR, "URIError");
ascii_const!(STRING_OBJECT_NULL, "[object Null]");
ascii_const!(STRING_OBJECT_UNDEFINED, "[object Undefined]");
ascii_const!(STRING_INVALID_DATE, "Invalid Date");

pub fn ascii_string(s: &str) -> JsString {
    JsString::Ascii(AsciiString::new(s.as_bytes()))
}

pub fn new_string_value(s: &[u8]) -> JsString {
    match unistring::scan(s) {
        Some(u) => JsString::Unicode(UnicodeString::from_raw(u)),
        None => JsString::Ascii(AsciiString::new(s)),
    }
}

pub fn string_value_from_raw(raw: &unistring::String) -> JsString {
    match raw.as_utf16() {
        Some(b) => JsString::Unicode(UnicodeString::from_raw(b.to_vec())),
        None => match raw.shared_ascii_bytes() {
            Some(b) => JsString::Ascii(AsciiString::from_shared(b.clone())),
            None => JsString::Ascii(AsciiString::new(&[])),
        },
    }
}

pub fn string_from_utf16(chars: &[u16]) -> JsString {
    if chars.iter().all(|&c| c < 0x80) {
        let b: Vec<u8> = chars.iter().map(|&c| c as u8).collect();
        return JsString::Ascii(AsciiString::new(&b));
    }
    let mut buf = Vec::with_capacity(chars.len() + 1);
    buf.push(unistring::BOM);
    buf.extend_from_slice(chars);
    JsString::Unicode(UnicodeString::from_raw(buf))
}

pub fn string_from_rune(r: i32) -> JsString {
    if r < 0x80 {
        return JsString::Ascii(AsciiString::new(&[r as u8]));
    }
    let mut sb = UnicodeStringBuilder::default();
    sb.write_rune(r);
    sb.string()
}

pub fn is_utf16_first_surrogate(c: u16) -> bool {
    (0xD800..=0xDBFF).contains(&c)
}

pub fn is_utf16_second_surrogate(c: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&c)
}

pub(crate) fn devirtualize_string(s: &JsString) -> (Option<AsciiString>, Option<UnicodeString>) {
    match s {
        JsString::Ascii(a) => (Some(a.clone()), None),
        JsString::Unicode(u) => (None, Some(u.clone())),
        JsString::Imported(i) => match i.unicode() {
            Some(u) => (None, Some(u.clone())),
            None => (Some(i.ascii()), None),
        },
    }
}

pub trait RuneReader {
    fn read_rune(&mut self) -> Option<(i32, usize)>;

    fn last_rune_invalid(&self) -> bool {
        false
    }
}

pub trait Utf16Reader {
    fn read_char(&mut self) -> Option<u16>;
}

impl JsString {
    pub fn string(&self) -> unistring::String {
        match self {
            JsString::Ascii(s) => s.string(),
            JsString::Unicode(s) => s.string(),
            JsString::Imported(s) => s.string(),
        }
    }

    pub fn go_string(&self) -> Vec<u8> {
        match self {
            JsString::Ascii(s) => s.go_string(),
            JsString::Unicode(s) => s.go_string(),
            JsString::Imported(s) => s.go_string(),
        }
    }

    pub fn to_rust_string(&self) -> String {
        match String::from_utf8(self.go_string()) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    }

    pub fn to_boolean(&self) -> bool {
        match self {
            JsString::Ascii(s) => s.to_boolean(),
            JsString::Unicode(s) => s.to_boolean(),
            JsString::Imported(s) => !s.utf8().is_empty(),
        }
    }

    pub fn to_float(&self) -> f64 {
        match self {
            JsString::Ascii(s) => s.to_float(),
            JsString::Unicode(s) => s.to_float(),
            JsString::Imported(s) => s.to_float(),
        }
    }

    pub fn to_number(&self) -> Value {
        match self {
            JsString::Ascii(s) => s.to_number(),
            JsString::Unicode(s) => s.to_number(),
            JsString::Imported(s) => s.to_number(),
        }
    }

    pub fn to_integer(&self) -> i64 {
        match self {
            JsString::Ascii(s) => s.to_integer(),
            JsString::Unicode(s) => s.to_integer(),
            JsString::Imported(s) => s.to_integer(),
        }
    }

    pub fn to_trimmed_utf8(&self) -> Vec<u8> {
        match self {
            JsString::Ascii(s) => s.to_trimmed_utf8(),
            JsString::Unicode(s) => s.to_trimmed_utf8(),
            JsString::Imported(s) => s.to_trimmed_utf8(),
        }
    }

    pub fn same_as(&self, other: &Value) -> bool {
        self.strict_equals(other)
    }

    pub fn strict_equals(&self, other: &Value) -> bool {
        match self {
            JsString::Ascii(s) => s.strict_equals(other),
            JsString::Unicode(s) => s.strict_equals(other),
            JsString::Imported(s) => s.strict_equals(other),
        }
    }

    pub fn equals(&self, other: &Value) -> bool {
        match self {
            JsString::Ascii(s) => s.equals(other),
            JsString::Unicode(s) => s.equals(other),
            JsString::Imported(s) => s.equals(other),
        }
    }

    pub fn char_at(&self, idx: usize) -> u16 {
        match self {
            JsString::Ascii(s) => s.char_at(idx),
            JsString::Unicode(s) => s.char_at(idx),
            JsString::Imported(s) => s.char_at(idx),
        }
    }

    pub fn length(&self) -> usize {
        match self {
            JsString::Ascii(s) => s.length(),
            JsString::Unicode(s) => s.length(),
            JsString::Imported(s) => s.length(),
        }
    }

    pub fn concat(&self, other: &JsString) -> JsString {
        match self {
            JsString::Ascii(s) => s.concat(other),
            JsString::Unicode(s) => s.concat(other),
            JsString::Imported(s) => s.concat(other),
        }
    }

    pub fn substring(&self, start: usize, end: usize) -> JsString {
        match self {
            JsString::Ascii(s) => s.substring(start, end),
            JsString::Unicode(s) => s.substring(start, end),
            JsString::Imported(s) => s.substring(start, end),
        }
    }

    pub fn compare_to(&self, other: &JsString) -> i32 {
        match self {
            JsString::Ascii(s) => s.compare_to(other),
            JsString::Unicode(s) => s.compare_to(other),
            JsString::Imported(s) => s.compare_to(other),
        }
    }

    pub fn reader(&self) -> Box<dyn RuneReader> {
        match self {
            JsString::Ascii(s) => s.reader(),
            JsString::Unicode(s) => s.reader(),
            JsString::Imported(s) => s.reader(),
        }
    }

    pub fn utf16_reader(&self) -> Box<dyn Utf16Reader> {
        match self {
            JsString::Ascii(s) => s.utf16_reader(),
            JsString::Unicode(s) => s.utf16_reader(),
            JsString::Imported(s) => s.utf16_reader(),
        }
    }

    pub fn utf16_rune_reader(&self) -> Box<dyn RuneReader> {
        match self {
            JsString::Ascii(s) => s.utf16_rune_reader(),
            JsString::Unicode(s) => s.utf16_rune_reader(),
            JsString::Imported(s) => s.utf16_rune_reader(),
        }
    }

    pub fn utf16_runes(&self) -> Vec<i32> {
        match self {
            JsString::Ascii(s) => s.utf16_runes(),
            JsString::Unicode(s) => s.utf16_runes(),
            JsString::Imported(s) => s.utf16_runes(),
        }
    }

    pub fn index(&self, substr: &JsString, start: isize) -> isize {
        match self {
            JsString::Ascii(s) => s.index(substr, start),
            JsString::Unicode(s) => s.index(substr, start),
            JsString::Imported(s) => s.index(substr, start),
        }
    }

    pub fn last_index(&self, substr: &JsString, start: isize) -> isize {
        match self {
            JsString::Ascii(s) => s.last_index(substr, start),
            JsString::Unicode(s) => s.last_index(substr, start),
            JsString::Imported(s) => s.last_index(substr, start),
        }
    }

    pub fn to_lower(&self) -> JsString {
        match self {
            JsString::Ascii(s) => s.to_lower(),
            JsString::Unicode(s) => s.to_lower(),
            JsString::Imported(s) => s.to_lower(),
        }
    }

    pub fn to_upper(&self) -> JsString {
        match self {
            JsString::Ascii(s) => s.to_upper(),
            JsString::Unicode(s) => s.to_upper(),
            JsString::Imported(s) => s.to_upper(),
        }
    }

    pub fn is_ascii(&self) -> bool {
        match self {
            JsString::Ascii(_) => true,
            JsString::Unicode(_) => false,
            JsString::Imported(s) => s.unicode().is_none(),
        }
    }
}

pub struct StringObject {
    pub value: Option<JsString>,
    pub length: usize,
}

impl StringObject {
    pub fn trace(&self, _t: &mut Tracer) {}

    fn data(rt: &Runtime, o: ObjRef) -> &StringObject {
        match &rt.obj(o).ext {
            ObjExt::StringObj(s) => s,
            _ => unreachable!("not a string object"),
        }
    }

    pub fn init(rt: &mut Runtime, o: ObjRef) {
        StringObject::set_length(rt, o);
    }

    pub fn set_length(rt: &mut Runtime, o: ObjRef) {
        let obj = rt.obj_mut(o);
        let len = match &mut obj.ext {
            ObjExt::StringObj(s) => {
                if let Some(v) = &s.value {
                    s.length = v.length();
                }
                s.length
            }
            _ => 0,
        };
        let lp = ValueProperty {
            value: int_to_value(len as i64),
            ..ValueProperty::default()
        };
        if let Some(Value::Property(p)) = obj.base.values.get_mut(&*crate::vm::STR_LENGTH) {
            **p = lp;
            return;
        }
        obj.base
            ._put(&crate::vm::STR_LENGTH, Value::Property(Box::new(lp)));
    }

    fn len(rt: &Runtime, o: ObjRef) -> i64 {
        StringObject::data(rt, o).length as i64
    }

    pub fn _get_idx(rt: &Runtime, o: ObjRef, idx: usize) -> Value {
        match &StringObject::data(rt, o).value {
            Some(v) => Value::String(v.substring(idx, idx + 1)),
            None => Value::Undefined,
        }
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let i = str_to_go_idx(name);
        if i >= 0 && i < StringObject::len(rt, o) {
            return Ok(Some(StringObject::_get_idx(rt, o, i as usize)));
        }
        BaseObject::get_str(rt, o, name, receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if idx >= 0 && idx < StringObject::len(rt, o) {
            return Ok(Some(StringObject::_get_idx(rt, o, idx as usize)));
        }
        BaseObject::get_str(rt, o, &idx_string(idx), receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let i = str_to_go_idx(name);
        if i >= 0 && i < StringObject::len(rt, o) {
            let val = StringObject::_get_idx(rt, o, i as usize);
            return Ok(Some(Value::Property(Box::new(ValueProperty {
                value: val,
                enumerable: true,
                ..ValueProperty::default()
            }))));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        if idx >= 0 {
            if idx < StringObject::len(rt, o) {
                let val = StringObject::_get_idx(rt, o, idx as usize);
                return Ok(Some(Value::Property(Box::new(ValueProperty {
                    value: val,
                    enumerable: true,
                    ..ValueProperty::default()
                }))));
            }
            return Ok(None);
        }
        BaseObject::get_own_prop_str(rt, o, &idx_string(idx))
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let i = str_to_go_idx(name);
        if i >= 0 && i < StringObject::len(rt, o) {
            rt.type_error_result(
                throw,
                &format!("Cannot assign to read only property '{i}' of a String"),
            )?;
            return Ok(false);
        }
        BaseObject::set_own_str(rt, o, name, val, throw)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 && idx < StringObject::len(rt, o) {
            rt.type_error_result(
                throw,
                &format!("Cannot assign to read only property '{idx}' of a String"),
            )?;
            return Ok(false);
        }
        BaseObject::set_own_str(rt, o, &idx_string(idx), val, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let p = StringObject::get_own_prop_str(rt, o, name)?;
        BaseObject::_set_foreign_str(rt, o, name, p.as_ref(), val, receiver, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let p = StringObject::get_own_prop_idx(rt, o, idx)?;
        BaseObject::_set_foreign_idx(rt, o, idx, p.as_ref(), val, receiver, throw)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let i = str_to_go_idx(name);
        if i >= 0 && i < StringObject::len(rt, o) {
            let existing = Value::Property(Box::new(ValueProperty {
                enumerable: true,
                ..ValueProperty::default()
            }));
            let r = BaseObject::_define_own_property(rt, o, name, Some(&existing), &descr, throw)?;
            return Ok(r.is_some());
        }
        BaseObject::define_own_property_str(rt, o, name, descr, throw)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 && idx < StringObject::len(rt, o) {
            rt.type_error_result(throw, &format!("Cannot redefine property: {idx}"))?;
            return Ok(false);
        }
        BaseObject::define_own_property_str(rt, o, &idx_string(idx), descr, throw)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let length = StringObject::data(rt, o).length;
        Ok(Box::new(StringPropIter {
            obj: o,
            idx: 0,
            length,
            base: None,
        }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        for i in 0..StringObject::data(rt, o).length {
            go_append(&mut accum, Value::String(ascii_string(&i.to_string())));
        }
        BaseObject::string_keys(rt, o, all, accum)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let i = str_to_go_idx(name);
        if i >= 0 && i < StringObject::len(rt, o) {
            rt.type_error_result(throw, &format!("Cannot delete property '{i}' of a String"))?;
            return Ok(false);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        if idx >= 0 && idx < StringObject::len(rt, o) {
            rt.type_error_result(
                throw,
                &format!("Cannot delete property '{idx}' of a String"),
            )?;
            return Ok(false);
        }
        BaseObject::delete_str(rt, o, &idx_string(idx), throw)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let i = str_to_go_idx(name);
        if i >= 0 && i < StringObject::len(rt, o) {
            return Ok(true);
        }
        BaseObject::has_own_property_str(rt, o, name)
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        if idx >= 0 && idx < StringObject::len(rt, o) {
            return Ok(true);
        }
        BaseObject::has_own_property_str(rt, o, &idx_string(idx))
    }
}

pub struct StringPropIter {
    obj: ObjRef,
    idx: usize,
    length: usize,
    base: Option<IterNextFunc>,
}

impl PropIter for StringPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.base.is_none() {
            if self.idx < self.length {
                let name = self.idx.to_string();
                self.idx += 1;
                return Ok(Some(PropIterItem {
                    name: Value::String(ascii_string(&name)),
                    value: None,
                    enumerable: EnumerableFlag::True,
                }));
            }
            self.base = Some(BaseObject::iterate_string_keys(rt, self.obj)?);
        }
        match &mut self.base {
            Some(b) => b.next(rt),
            None => Ok(None),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.obj);
        if let Some(b) = &self.base {
            b.trace(t);
        }
    }
}

pub static STRING_OBJECT_CLASS: ObjClass = ObjClass {
    get_str: StringObject::get_str,
    get_idx: StringObject::get_idx,
    get_own_prop_str: StringObject::get_own_prop_str,
    get_own_prop_idx: StringObject::get_own_prop_idx,
    set_own_str: StringObject::set_own_str,
    set_own_idx: StringObject::set_own_idx,
    set_foreign_str: StringObject::set_foreign_str,
    set_foreign_idx: StringObject::set_foreign_idx,
    define_own_property_str: StringObject::define_own_property_str,
    define_own_property_idx: StringObject::define_own_property_idx,
    iterate_string_keys: StringObject::iterate_string_keys,
    string_keys: StringObject::string_keys,
    delete_str: StringObject::delete_str,
    delete_idx: StringObject::delete_idx,
    has_own_property_str: StringObject::has_own_property_str,
    has_own_property_idx: StringObject::has_own_property_idx,
    ..BASE_CLASS
};

pub struct StringIterObject {
    pub reader: Option<Box<dyn RuneReader>>,
}

impl StringIterObject {
    pub fn trace(&self, _t: &mut Tracer) {}

    pub fn next(rt: &mut Runtime, o: ObjRef) -> JsResult<Value> {
        let r = match &mut rt.obj_mut(o).ext {
            ObjExt::StringIter(si) => match &mut si.reader {
                None => None,
                Some(reader) => match reader.read_rune() {
                    None => {
                        si.reader = None;
                        None
                    }
                    Some((r, _)) => Some(r),
                },
            },
            _ => return Err(Throw::TypeError("not a string iterator".to_string())),
        };
        Ok(match r {
            None => rt.create_iter_result_object(Value::Undefined, true),
            Some(r) => rt.create_iter_result_object(Value::String(string_from_rune(r)), false),
        })
    }
}

pub static STRING_ITER_OBJECT_CLASS: ObjClass = BASE_CLASS;

impl Runtime {
    pub fn new_string_object(&mut self, s: JsString) -> ObjRef {
        let proto = self.get_string_prototype();
        let o = self.alloc_object(
            &STRING_OBJECT_CLASS,
            BaseObject::new(crate::object::CLASS_STRING, Some(proto)),
            ObjExt::StringObj(Box::new(StringObject {
                value: Some(s),
                length: 0,
            })),
        );
        StringObject::init(self, o);
        o
    }

    pub fn create_string_iterator(&mut self, s: &JsString) -> Value {
        let proto = self.get_string_iterator_prototype();
        let o = self.alloc_object(
            &STRING_ITER_OBJECT_CLASS,
            BaseObject::new(crate::object::CLASS_OBJECT, Some(proto)),
            ObjExt::StringIter(Box::new(StringIterObject {
                reader: Some(crate::string_unicode::lenient_utf16_decoder(
                    s.utf16_reader(),
                )),
            })),
        );
        Value::Object(o)
    }
}
