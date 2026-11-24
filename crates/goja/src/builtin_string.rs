use crate::builtin_regexp::is_regexp;
use crate::builtin_symbol::{
    SYM_ITERATOR, SYM_MATCH, SYM_MATCH_ALL, SYM_REPLACE, SYM_SEARCH, SYM_SPLIT, SYM_TO_STRING_TAG,
};
use crate::deps::xtext::norm::Form;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{
    BaseObject, CLASS_OBJECT, CLASS_STRING, CLASS_STRING_ITERATOR, CallFn, FunctionCall, ObjExt,
    to_method,
};
use crate::object_template::ObjectTemplate;
use crate::regexp::RegexpObject;
use crate::runtime::{JsResult, Runtime, Throw, to_length, to_uint16, to_uint32};
use crate::string::{
    JsString, RuneReader, STRING_EMPTY, STRING_ITER_OBJECT_CLASS, STRING_OBJECT_CLASS,
    StringBuilder, StringIterObject, StringObject, UnicodeStringBuilder, ascii_string,
    is_utf16_first_surrogate, is_utf16_second_surrogate, new_string_value,
};
use crate::string_ascii::AsciiString;
use crate::string_imported::ImportedString;
use crate::string_unicode::UnicodeString;
use crate::unistring;
use crate::value::{Value, int_to_value, nil_safe, value_prop};
use gocompat::strings;
use gocompat::utf8;
use gocompat::utf16;
use std::sync::OnceLock;

#[derive(Clone, Debug)]
pub(crate) enum StrUnits {
    Ascii(AsciiString),
    Imported(ImportedString),
    Unicode(UnicodeString),
}

impl StrUnits {
    pub(crate) fn of(s: &JsString) -> StrUnits {
        match s {
            JsString::Ascii(a) => StrUnits::Ascii(a.clone()),
            JsString::Unicode(u) => StrUnits::Unicode(u.clone()),
            JsString::Imported(i) => match i.unicode() {
                Some(u) => StrUnits::Unicode(u.clone()),
                None => StrUnits::Imported(i.clone()),
            },
        }
    }

    #[inline]
    pub(crate) fn ascii(&self) -> Option<&[u8]> {
        match self {
            StrUnits::Ascii(a) => Some(a.as_bytes()),
            StrUnits::Imported(i) => Some(i.utf8()),
            StrUnits::Unicode(_) => None,
        }
    }

    #[inline]
    pub(crate) fn len(&self) -> usize {
        match self {
            StrUnits::Ascii(a) => a.len(),
            StrUnits::Imported(i) => i.utf8().len(),
            StrUnits::Unicode(u) => u.length(),
        }
    }

    pub(crate) fn substring(&self, start: usize, end: usize) -> JsString {
        match self {
            StrUnits::Unicode(u) => u.substring(start, end),
            _ => match self.ascii() {
                Some(b) => JsString::Ascii(AsciiString::new(&b[start..end])),
                None => unreachable!("ascii units"),
            },
        }
    }

    pub(crate) fn write_substring(&self, buf: &mut StringBuilder, start: usize, end: usize) {
        match self {
            StrUnits::Unicode(u) => {
                buf.write_substring(&JsString::Unicode(u.clone()), start, end);
            }
            _ => {
                if let Some(b) = self.ascii() {
                    buf.write_ascii(&b[start..end]);
                }
            }
        }
    }
}

pub(crate) fn lenient_decode_units(units: &[u16], pos: usize) -> (i32, usize) {
    let c = units[pos];
    if is_utf16_first_surrogate(c)
        && pos + 1 < units.len()
        && is_utf16_second_surrogate(units[pos + 1])
    {
        return (
            utf16::decode_rune(i32::from(c), i32::from(units[pos + 1])),
            2,
        );
    }
    (i32::from(c), 1)
}

pub(crate) fn lenient_decode_at(s: &StrUnits, pos: usize) -> (i32, usize) {
    match s {
        StrUnits::Unicode(u) => lenient_decode_units(&u.raw()[1..], pos),
        _ => match s.ascii() {
            Some(b) => (i32::from(b[pos]), 1),
            None => unreachable!("ascii units"),
        },
    }
}

struct LenientUtf16Decoder {
    s: StrUnits,
    pos: usize,
}

impl RuneReader for LenientUtf16Decoder {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos >= self.s.len() {
            return None;
        }
        let (r, size) = lenient_decode_at(&self.s, self.pos);
        self.pos += size;
        Some((r, size))
    }
}

fn bytes_index(hay: &[u8], needle: &[u8]) -> Option<usize> {
    let n = needle.len();
    if n == 0 {
        return Some(0);
    }
    if n > hay.len() {
        return None;
    }
    let first = needle[0];
    let last_start = hay.len() - n;
    let mut i = 0;
    while i <= last_start {
        let p = hay[i..=last_start].iter().position(|&b| b == first)?;
        i += p;
        if hay[i + 1..i + n] == needle[1..] {
            return Some(i);
        }
        i += 1;
    }
    None
}

pub(crate) fn string_index(s: &JsString, substr: &JsString, start: usize) -> isize {
    s.index(substr, start as isize)
}

pub(crate) fn string_last_index(s: &JsString, substr: &JsString, pos: usize) -> isize {
    s.last_index(substr, pos as isize)
}

pub(crate) fn string_to_lower(s: &JsString) -> JsString {
    s.to_lower()
}

pub(crate) fn string_to_upper(s: &JsString) -> JsString {
    s.to_upper()
}

pub fn is_whitespace_char(r: i32) -> bool {
    matches!(
        r,
        0x20 | 0x0c | 0x0a | 0x0d | 0x09 | 0x0b | 0xa0 | 0x1680 | 0x2000
            ..=0x200a | 0x2028 | 0x2029 | 0x202f | 0x205f | 0x3000 | 0xfeff
    )
}

fn trim_result(s: &JsString, trimmed: &[u8], whole: usize) -> JsString {
    if trimmed.len() == whole
        && let JsString::Ascii(_) = s
    {
        return s.clone();
    }
    JsString::Ascii(AsciiString::new(trimmed))
}

#[derive(Clone, Copy)]
enum TrimKind {
    Both,
    Start,
    End,
}

fn trim_bytes(b: &[u8], kind: TrimKind) -> &[u8] {
    match kind {
        TrimKind::Both => strings::trim_func(b, is_whitespace_char),
        TrimKind::Start => strings::trim_left_func(b, is_whitespace_char),
        TrimKind::End => strings::trim_right_func(b, is_whitespace_char),
    }
}

fn string_trim(s: &JsString, kind: TrimKind) -> JsString {
    let units = StrUnits::of(s);
    if let Some(b) = units.ascii() {
        return trim_result(s, trim_bytes(b, kind), b.len());
    }
    let b = s.go_string();
    new_string_value(trim_bytes(&b, kind))
}

fn explode(s: &[u8], n: isize) -> Vec<&[u8]> {
    let l = utf8::rune_count(s) as isize;
    let n = if n < 0 || n > l { l } else { n };
    let mut a = Vec::with_capacity(n as usize);
    let mut s = s;
    let mut i = 0;
    while i < n - 1 {
        let (_, size) = utf8::decode_rune(s);
        a.push(&s[..size]);
        s = &s[size..];
        i += 1;
    }
    if n > 0 {
        a.push(s);
    }
    a
}

fn split_n<'a>(s: &'a [u8], sep: &[u8], n: isize) -> Vec<&'a [u8]> {
    if n == 0 {
        return Vec::new();
    }
    if sep.is_empty() {
        return explode(s, n);
    }
    let mut n = n;
    if n < 0 {
        let mut count = 0;
        let mut rest = s;
        while let Some(m) = bytes_index(rest, sep) {
            count += 1;
            rest = &rest[m + sep.len()..];
        }
        n = count + 1;
    }
    if n > s.len() as isize + 1 {
        n = s.len() as isize + 1;
    }
    let mut a = Vec::with_capacity(n as usize);
    let mut s = s;
    let n = n - 1;
    let mut i = 0;
    while i < n {
        let Some(m) = bytes_index(s, sep) else {
            break;
        };
        a.push(&s[..m]);
        s = &s[m + sep.len()..];
        i += 1;
    }
    a.push(s);
    a
}

fn to_string(rt: &mut Runtime, arg: &Value) -> JsResult<JsString> {
    if let Value::String(s) = arg {
        return Ok(s.clone());
    }
    if let Value::Symbol(s) = arg {
        return Ok(rt.symbol_descriptive_string(*s));
    }
    arg.to_string(rt)
}

pub(crate) fn get_replace_value(
    rt: &mut Runtime,
    replace_value: &Value,
) -> JsResult<(Option<JsString>, Option<CallFn>)> {
    if let Value::Object(o) = replace_value
        && let Some(c) = o.assert_callable(rt)
    {
        return Ok((None, Some(c)));
    }
    Ok((Some(replace_value.to_string(rt)?), None))
}

pub(crate) fn write_substitution<F: FnMut(usize) -> JsResult<JsString>>(
    s: &JsString,
    position: usize,
    num_captures: usize,
    mut get_capture: F,
    replace_str: &JsString,
    buf: &mut StringBuilder,
) -> JsResult<()> {
    let units = StrUnits::of(s);
    let l = units.len();
    let rl = replace_str.length();
    let matched = get_capture(0)?;
    let tail_pos = position + matched.length();
    let mut i = 0;
    while i < rl {
        let c = replace_str.char_at(i);
        if c == u16::from(b'$') && i + 1 < rl {
            let ch = replace_str.char_at(i + 1);
            match ch {
                0x24 => buf.write_rune('$' as i32),
                0x60 => units.write_substring(buf, 0, position),
                0x27 => {
                    if tail_pos < l {
                        units.write_substring(buf, tail_pos, l);
                    }
                }
                0x26 => buf.write_string(&matched),
                _ => {
                    let mut match_number = 0;
                    let mut j = i + 1;
                    while j < rl {
                        let ch = replace_str.char_at(j);
                        if (u16::from(b'0')..=u16::from(b'9')).contains(&ch) {
                            let m = match_number * 10 + usize::from(ch - u16::from(b'0'));
                            if m >= num_captures {
                                break;
                            }
                            match_number = m;
                            j += 1;
                        } else {
                            break;
                        }
                    }
                    if match_number > 0 {
                        buf.write_string(&get_capture(match_number)?);
                        i = j;
                        continue;
                    }
                    buf.write_rune('$' as i32);
                    buf.write_rune(i32::from(ch));
                }
            }
            i += 2;
        } else {
            buf.write_rune(i32::from(c));
            i += 1;
        }
    }
    Ok(())
}

pub(crate) fn string_replace(
    rt: &mut Runtime,
    s: &JsString,
    found: &[Vec<isize>],
    newstring: Option<&JsString>,
    rcall: Option<CallFn>,
) -> JsResult<Value> {
    if found.is_empty() {
        return Ok(Value::String(s.clone()));
    }
    let units = StrUnits::of(s);
    let mut buf = StringBuilder::new();
    let mut last_index = 0usize;
    let length_s = s.length();
    if let Some(rcall) = rcall {
        for item in found {
            if item[0] as usize != last_index {
                units.write_substring(&mut buf, last_index, item[0] as usize);
            }
            let match_count = item.len() / 2;
            let mut argument_list = Vec::with_capacity(match_count + 2);
            for index in 0..match_count {
                let offset = 2 * index;
                if item[offset] != -1 {
                    argument_list.push(Value::String(
                        units.substring(item[offset] as usize, item[offset + 1] as usize),
                    ));
                } else {
                    argument_list.push(Value::Undefined);
                }
            }
            argument_list.push(Value::Int(item[0] as i64));
            argument_list.push(Value::String(s.clone()));
            let replacement = rcall
                .call(rt, Value::Undefined, &argument_list)?
                .to_string(rt)?;
            buf.write_string(&replacement);
            last_index = item[1] as usize;
        }
    } else {
        let newstring = match newstring {
            Some(n) => n.clone(),
            None => STRING_EMPTY.clone(),
        };
        for item in found {
            if item[0] as usize != last_index {
                units.write_substring(&mut buf, last_index, item[0] as usize);
            }
            let match_count = item.len() / 2;
            write_substitution(
                s,
                item[0] as usize,
                match_count,
                |idx| {
                    if item[idx * 2] != -1 {
                        return Ok(
                            units.substring(item[idx * 2] as usize, item[idx * 2 + 1] as usize)
                        );
                    }
                    Ok(STRING_EMPTY.clone())
                },
                &newstring,
                &mut buf,
            )?;
            last_index = item[1] as usize;
        }
    }
    if last_index != length_s {
        units.write_substring(&mut buf, last_index, length_s);
    }
    Ok(Value::String(buf.string()))
}

fn range_error(rt: &mut Runtime, msg: &str) -> Throw {
    let e = rt.get_range_error();
    Throw::Value(rt.new_error(e, msg))
}

fn type_error(rt: &mut Runtime, msg: &str) -> Throw {
    Throw::Value(rt.new_type_error(msg))
}

fn create_string_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("length", |_| {
        value_prop(int_to_value(0), false, false, false)
    });

    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_string()), true, false, true)
    });

    t.put_str("at", |r| r.method_prop(|r, c| r.stringproto_at(c), "at", 1));
    t.put_str("charAt", |r| {
        r.method_prop(|r, c| r.stringproto_char_at(c), "charAt", 1)
    });
    t.put_str("charCodeAt", |r| {
        r.method_prop(|r, c| r.stringproto_char_code_at(c), "charCodeAt", 1)
    });
    t.put_str("codePointAt", |r| {
        r.method_prop(|r, c| r.stringproto_code_point_at(c), "codePointAt", 1)
    });
    t.put_str("concat", |r| {
        r.method_prop(|r, c| r.stringproto_concat(c), "concat", 1)
    });
    t.put_str("endsWith", |r| {
        r.method_prop(|r, c| r.stringproto_ends_with(c), "endsWith", 1)
    });
    t.put_str("includes", |r| {
        r.method_prop(|r, c| r.stringproto_includes(c), "includes", 1)
    });
    t.put_str("indexOf", |r| {
        r.method_prop(|r, c| r.stringproto_index_of(c), "indexOf", 1)
    });
    t.put_str("lastIndexOf", |r| {
        r.method_prop(|r, c| r.stringproto_last_index_of(c), "lastIndexOf", 1)
    });
    t.put_str("localeCompare", |r| {
        r.method_prop(|r, c| r.stringproto_locale_compare(c), "localeCompare", 1)
    });
    t.put_str("match", |r| {
        r.method_prop(|r, c| r.stringproto_match(c), "match", 1)
    });
    t.put_str("matchAll", |r| {
        r.method_prop(|r, c| r.stringproto_match_all(c), "matchAll", 1)
    });
    t.put_str("normalize", |r| {
        r.method_prop(|r, c| r.stringproto_normalize(c), "normalize", 0)
    });
    t.put_str("padEnd", |r| {
        r.method_prop(|r, c| r.stringproto_pad_end(c), "padEnd", 1)
    });
    t.put_str("padStart", |r| {
        r.method_prop(|r, c| r.stringproto_pad_start(c), "padStart", 1)
    });
    t.put_str("repeat", |r| {
        r.method_prop(|r, c| r.stringproto_repeat(c), "repeat", 1)
    });
    t.put_str("replace", |r| {
        r.method_prop(|r, c| r.stringproto_replace(c), "replace", 2)
    });
    t.put_str("replaceAll", |r| {
        r.method_prop(|r, c| r.stringproto_replace_all(c), "replaceAll", 2)
    });
    t.put_str("search", |r| {
        r.method_prop(|r, c| r.stringproto_search(c), "search", 1)
    });
    t.put_str("slice", |r| {
        r.method_prop(|r, c| r.stringproto_slice(c), "slice", 2)
    });
    t.put_str("split", |r| {
        r.method_prop(|r, c| r.stringproto_split(c), "split", 2)
    });
    t.put_str("startsWith", |r| {
        r.method_prop(|r, c| r.stringproto_starts_with(c), "startsWith", 1)
    });
    t.put_str("substring", |r| {
        r.method_prop(|r, c| r.stringproto_substring(c), "substring", 2)
    });
    t.put_str("toLocaleLowerCase", |r| {
        r.method_prop(
            |r, c| r.stringproto_to_lower_case(c),
            "toLocaleLowerCase",
            0,
        )
    });
    t.put_str("toLocaleUpperCase", |r| {
        r.method_prop(
            |r, c| r.stringproto_to_upper_case(c),
            "toLocaleUpperCase",
            0,
        )
    });
    t.put_str("toLowerCase", |r| {
        r.method_prop(|r, c| r.stringproto_to_lower_case(c), "toLowerCase", 0)
    });
    t.put_str("toString", |r| {
        r.method_prop(|r, c| r.stringproto_to_string(c), "toString", 0)
    });
    t.put_str("toUpperCase", |r| {
        r.method_prop(|r, c| r.stringproto_to_upper_case(c), "toUpperCase", 0)
    });
    t.put_str("trim", |r| {
        r.method_prop(|r, c| r.stringproto_trim(c), "trim", 0)
    });
    t.put_str("trimEnd", |r| {
        value_prop(
            Value::Object(r.get_stringproto_trim_end()),
            true,
            false,
            true,
        )
    });
    t.put_str("trimStart", |r| {
        value_prop(
            Value::Object(r.get_stringproto_trim_start()),
            true,
            false,
            true,
        )
    });
    t.put_str("trimRight", |r| {
        value_prop(
            Value::Object(r.get_stringproto_trim_end()),
            true,
            false,
            true,
        )
    });
    t.put_str("trimLeft", |r| {
        value_prop(
            Value::Object(r.get_stringproto_trim_start()),
            true,
            false,
            true,
        )
    });
    t.put_str("valueOf", |r| {
        r.method_prop(|r, c| r.stringproto_value_of(c), "valueOf", 0)
    });

    t.put_str("substr", |r| {
        r.method_prop(|r, c| r.stringproto_substr(c), "substr", 2)
    });

    t.put_sym(SYM_ITERATOR, |r| {
        let f = r.new_native_func(
            NativeFn::Fn(|r, c| r.stringproto_iterator(c)),
            "[Symbol.iterator]",
            0,
        );
        value_prop(Value::Object(f), true, false, true)
    });

    t
}

pub fn get_string_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_string_proto_template)
}

impl Runtime {
    pub fn builtin_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Some(a) = call.arguments.first() {
            return Ok(Value::String(to_string(self, a)?));
        }
        Ok(Value::String(STRING_EMPTY.clone()))
    }

    pub fn _new_string(&mut self, s: JsString, proto: Option<ObjRef>) -> ObjRef {
        let o = self.alloc_object(
            &STRING_OBJECT_CLASS,
            BaseObject::new(CLASS_STRING, proto),
            ObjExt::StringObj(Box::new(StringObject {
                value: Some(s),
                length: 0,
            })),
        );
        StringObject::init(self, o);
        o
    }

    pub fn builtin_new_string(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let s = match args.first() {
            Some(a) => a.to_string(self)?,
            None => STRING_EMPTY.clone(),
        };
        Ok(self._new_string(s, proto))
    }

    pub fn stringproto_to_string_value_of(
        &mut self,
        this: &Value,
        func_name: &str,
    ) -> JsResult<Value> {
        if let Value::String(_) = this {
            return Ok(this.clone());
        }
        if let Value::Object(obj) = this {
            if let ObjExt::StringObj(s) = &self.obj(*obj).ext {
                return Ok(match &s.value {
                    Some(v) => Value::String(v.clone()),
                    None => Value::Undefined,
                });
            }
            if Some(*obj) == self.global.string_prototype {
                return Ok(Value::String(STRING_EMPTY.clone()));
            }
        }
        Err(type_error(
            self,
            &format!("String.prototype.{func_name} is called on incompatible receiver"),
        ))
    }

    pub fn stringproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.stringproto_to_string_value_of(call.this, "toString")
    }

    pub fn stringproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.stringproto_to_string_value_of(call.this, "valueOf")
    }

    pub fn stringproto_iterator(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        Ok(self.create_string_iterator_lenient(&s))
    }

    fn create_string_iterator_lenient(&mut self, s: &JsString) -> Value {
        let proto = self.get_string_iterator_prototype();
        let o = self.alloc_object(
            &STRING_ITER_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, Some(proto)),
            ObjExt::StringIter(Box::new(StringIterObject {
                reader: Some(Box::new(LenientUtf16Decoder {
                    s: StrUnits::of(s),
                    pos: 0,
                })),
            })),
        );
        Value::Object(o)
    }

    pub fn string_fromcharcode(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let args = call.arguments;
        let mut b = Vec::with_capacity(args.len());
        for (i, arg) in args.iter().enumerate() {
            let chr = to_uint16(self, arg)?;
            if chr >= 0x80 {
                let mut bb = vec![0u16; args.len() + 1];
                bb[0] = unistring::BOM;
                for (j, &c) in b.iter().enumerate() {
                    bb[1 + j] = u16::from(c);
                }
                bb[1 + i] = chr;
                let i = i + 1;
                for (j, arg) in args[i..].iter().enumerate() {
                    bb[1 + i + j] = to_uint16(self, arg)?;
                }
                return Ok(Value::String(JsString::Unicode(UnicodeString::from_raw(
                    bb,
                ))));
            }
            b.push(chr as u8);
        }
        Ok(Value::String(JsString::Ascii(AsciiString::new(&b))))
    }

    pub fn string_fromcodepoint(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut sb = StringBuilder::new();
        for arg in call.arguments {
            let num = arg.to_number(self)?;
            let c = match num {
                Value::Int(n) => {
                    if !(0..=0x10FFFF).contains(&n) {
                        return Err(range_error(self, &format!("Invalid code point {n}")));
                    }
                    n as i32
                }
                _ => {
                    let s = num.go_string(self)?;
                    return Err(range_error(self, &format!("Invalid code point {s}")));
                }
            };
            sb.write_rune(c);
        }
        Ok(Value::String(sb.string()))
    }

    pub fn string_raw(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let cooked = call.arg(0).to_object(self)?;
        let raw = nil_safe(cooked.get_str(self, &"raw".into(), None)?).to_object(self)?;
        let l = nil_safe(raw.get_str(self, &"length".into(), None)?);
        let literal_segments = to_length(self, &l)?;
        if literal_segments <= 0 {
            return Ok(Value::String(STRING_EMPTY.clone()));
        }
        let mut string_elements = StringBuilder::new();
        let mut next_index = 0i64;
        let number_of_substitutions = call.arguments.len() as i64 - 1;
        loop {
            let next_seg = nil_safe(raw.get_idx(self, next_index, None)?).to_string(self)?;
            string_elements.write_string(&next_seg);
            if next_index + 1 == literal_segments {
                return Ok(Value::String(string_elements.string()));
            }
            if next_index < number_of_substitutions {
                let sub = call.arguments[(next_index + 1) as usize].to_string(self)?;
                string_elements.write_string(&sub);
            }
            next_index += 1;
        }
    }

    pub fn stringproto_at(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let mut pos = call.arg(0).to_integer(self)?;
        let length = s.length() as i64;
        if pos < 0 {
            pos += length;
        }
        if pos >= length || pos < 0 {
            return Ok(Value::Undefined);
        }
        Ok(Value::String(s.substring(pos as usize, pos as usize + 1)))
    }

    pub fn stringproto_char_at(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let pos = call.arg(0).to_integer(self)?;
        if pos < 0 || pos >= s.length() as i64 {
            return Ok(Value::String(STRING_EMPTY.clone()));
        }
        Ok(Value::String(s.substring(pos as usize, pos as usize + 1)))
    }

    pub fn stringproto_char_code_at(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let pos = call.arg(0).to_integer(self)?;
        if pos < 0 || pos >= s.length() as i64 {
            return Ok(Value::Float(f64::NAN));
        }
        Ok(int_to_value(i64::from(s.char_at(pos as usize))))
    }

    pub fn stringproto_code_point_at(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let p = call.arg(0).to_integer(self)?;
        let size = s.length();
        if p < 0 || p >= size as i64 {
            return Ok(Value::Undefined);
        }
        let mut pos = p as usize;
        let first = s.char_at(pos);
        if is_utf16_first_surrogate(first) {
            pos += 1;
            if pos < size {
                let second = s.char_at(pos);
                if is_utf16_second_surrogate(second) {
                    return Ok(int_to_value(i64::from(utf16::decode_rune(
                        i32::from(first),
                        i32::from(second),
                    ))));
                }
            }
        }
        Ok(int_to_value(i64::from(first)))
    }

    pub fn stringproto_concat(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let mut strs = Vec::with_capacity(call.arguments.len() + 1);
        let this = call.this.to_string(self)?;
        let mut all_ascii = true;
        let mut total_len = 0;
        let first = StrUnits::of(&this);
        if first.ascii().is_none() {
            all_ascii = false;
        }
        total_len += first.len();
        strs.push(first);
        for arg in call.arguments {
            let s = StrUnits::of(&arg.to_string(self)?);
            if s.ascii().is_none() {
                all_ascii = false;
            }
            total_len += s.len();
            strs.push(s);
        }
        if all_ascii {
            let mut buf = Vec::with_capacity(total_len);
            for s in &strs {
                if let Some(a) = s.ascii() {
                    buf.extend_from_slice(a);
                }
            }
            return Ok(Value::String(JsString::Ascii(AsciiString::new(&buf))));
        }
        let mut buf = Vec::with_capacity(total_len + 1);
        buf.push(unistring::BOM);
        for s in &strs {
            match (s, s.ascii()) {
                (StrUnits::Unicode(u), _) => buf.extend_from_slice(&u.raw()[1..]),
                (_, Some(a)) => buf.extend(a.iter().map(|&c| u16::from(c))),
                _ => {}
            }
        }
        Ok(Value::String(JsString::Unicode(UnicodeString::from_raw(
            buf,
        ))))
    }

    pub fn stringproto_ends_with(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let search_string = call.arg(0);
        if is_regexp(self, search_string)? {
            return Err(type_error(
                self,
                "First argument to String.prototype.endsWith must not be a regular expression",
            ));
        }
        let search_str = search_string.to_string(self)?;
        let l = s.length() as i64;
        let pos = match call.arg(1) {
            Value::Undefined => l,
            p => p.to_integer(self)?,
        };
        let end = pos.clamp(0, l) as usize;
        let search_length = search_str.length();
        if search_length > end {
            return Ok(Value::Bool(false));
        }
        let start = end - search_length;
        for i in 0..search_length {
            if s.char_at(start + i) != search_str.char_at(i) {
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(true))
    }

    pub fn stringproto_includes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let search_string = call.arg(0);
        if is_regexp(self, search_string)? {
            return Err(type_error(
                self,
                "First argument to String.prototype.includes must not be a regular expression",
            ));
        }
        let search_str = search_string.to_string(self)?;
        let pos = match call.arg(1) {
            Value::Undefined => 0,
            p => p.to_integer(self)?,
        };
        let start = pos.clamp(0, s.length() as i64) as usize;
        Ok(Value::Bool(string_index(&s, &search_str, start) != -1))
    }

    pub fn stringproto_index_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let value = call.this.to_string(self)?;
        let target = call.arg(0).to_string(self)?;
        let mut pos = call.arg(1).to_number(self)?.to_integer(self)?;
        if pos < 0 {
            pos = 0;
        } else {
            let l = value.length() as i64;
            if pos > l {
                pos = l;
            }
        }
        Ok(int_to_value(
            string_index(&value, &target, pos as usize) as i64
        ))
    }

    pub fn stringproto_last_index_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let value = call.this.to_string(self)?;
        let target = call.arg(0).to_string(self)?;
        let num_pos = call.arg(1).to_number(self)?;
        let pos = match num_pos {
            Value::Float(f) if f.is_nan() => value.length() as i64,
            _ => {
                let mut pos = num_pos.to_integer(self)?;
                if pos < 0 {
                    pos = 0;
                } else {
                    let l = value.length() as i64;
                    if pos > l {
                        pos = l;
                    }
                }
                pos
            }
        };
        Ok(int_to_value(
            string_last_index(&value, &target, pos as usize) as i64,
        ))
    }

    pub fn stringproto_locale_compare(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let this = call.this.to_string(self)?.go_string();
        let this = Form::Nfd.string(&this);
        let that = call.arg(0).to_string(self)?.go_string();
        let that = Form::Nfd.string(&that);
        Ok(int_to_value(i64::from(
            self.collator().compare_string(&this, &that),
        )))
    }

    pub fn stringproto_match(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let regexp = call.argument(0);
        if !matches!(regexp, Value::Undefined | Value::Null) {
            let m = self.get_v(&regexp, &Value::Symbol(SYM_MATCH))?;
            if let Some(matcher) = to_method(self, m)? {
                return matcher.call(self, regexp, std::slice::from_ref(call.this));
            }
        }
        let mut rx = None;
        if let Value::Object(o) = &regexp
            && RegexpObject::data(self, *o).is_some()
        {
            rx = Some(*o);
        }
        let rx = match rx {
            Some(rx) => rx,
            None => {
                let proto = self.get_reg_exp_prototype();
                self.new_reg_exp(Some(&regexp), None, Some(proto))?
            }
        };
        let m = nil_safe(rx.get_sym(self, SYM_MATCH, None)?);
        let m = self.to_object(&m)?;
        if let Some(matcher) = m.assert_callable(self) {
            let s = call.this.to_string(self)?;
            return matcher.call(self, Value::Object(rx), &[Value::String(s)]);
        }
        Err(type_error(self, "RegExp matcher is not a function"))
    }

    pub fn stringproto_match_all(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let regexp = call.argument(0);
        if !matches!(regexp, Value::Undefined | Value::Null) {
            if is_regexp(self, &regexp)?
                && let Value::Object(o) = &regexp
            {
                let flags = nil_safe(o.get_str(self, &"flags".into(), None)?);
                self.check_object_coercible(&flags)?;
                let f = flags.to_string(self)?.go_string();
                if !f.contains(&b'g') {
                    return Err(type_error(self, "RegExp doesn't have global flag set"));
                }
            }
            let m = self.get_v(&regexp, &Value::Symbol(SYM_MATCH_ALL))?;
            if let Some(matcher) = to_method(self, m)? {
                return matcher.call(self, regexp, std::slice::from_ref(call.this));
            }
        }
        let proto = self.get_reg_exp_prototype();
        let g = Value::String(ascii_string("g"));
        let rx = self.new_reg_exp(Some(&regexp), Some(&g), Some(proto))?;
        let m = nil_safe(rx.get_sym(self, SYM_MATCH_ALL, None)?);
        let m = self.to_object(&m)?;
        if let Some(matcher) = m.assert_callable(self) {
            let s = call.this.to_string(self)?;
            return matcher.call(self, Value::Object(rx), &[Value::String(s)]);
        }
        Err(type_error(self, "RegExp matcher is not a function"))
    }

    pub fn stringproto_normalize(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let form = match call.arg(0) {
            Value::Undefined => b"NFC".to_vec(),
            f => f.to_string(self)?.go_string(),
        };
        let f = match &form[..] {
            b"NFC" => Form::Nfc,
            b"NFD" => Form::Nfd,
            b"NFKC" => Form::Nfkc,
            b"NFKD" => Form::Nfkd,
            _ => {
                return Err(range_error(
                    self,
                    "The normalization form should be one of NFC, NFD, NFKC, NFKD",
                ));
            }
        };
        match &s {
            JsString::Ascii(_) => Ok(Value::String(s)),
            JsString::Unicode(u) => {
                let ss = u.go_string();
                Ok(Value::String(new_string_value(&f.string(&ss))))
            }
            JsString::Imported(i) => Ok(Value::String(new_string_value(&f.string(i.utf8())))),
        }
    }

    pub fn _string_pad(&mut self, call: FunctionCall<'_>, start: bool) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let max_length = to_length(self, call.arg(0))?;
        let string_length = s.length() as i64;
        if max_length <= string_length {
            return Ok(Value::String(s));
        }
        let str_units = StrUnits::of(&s);
        let filler;
        let filler_units;
        match call.arg(1) {
            Value::Undefined => {
                filler = ascii_string(" ");
                filler_units = StrUnits::of(&filler);
            }
            fill_string => {
                filler = fill_string.to_string(self)?;
                if filler.length() == 0 {
                    return Ok(Value::String(s));
                }
                filler_units = StrUnits::of(&filler);
            }
        }
        let mut remaining = (max_length - string_length) as usize;
        if let (Some(filler_ascii), Some(str_ascii)) = (filler_units.ascii(), str_units.ascii()) {
            let fl = filler_ascii.len();
            let mut sb = Vec::new();
            reserve_or_panic(&mut sb, max_length as usize);
            if !start {
                sb.extend_from_slice(str_ascii);
            }
            while remaining >= fl {
                sb.extend_from_slice(filler_ascii);
                remaining -= fl;
            }
            if remaining > 0 {
                sb.extend_from_slice(&filler_ascii[..remaining]);
            }
            if start {
                sb.extend_from_slice(str_ascii);
            }
            return Ok(Value::String(JsString::Ascii(AsciiString::new(&sb))));
        }
        let mut sb = UnicodeStringBuilder::default();
        reserve_or_panic(&mut sb.buf, max_length as usize + 1);
        sb.ensure_started(max_length as usize);
        if !start {
            sb.write_string(&s);
        }
        let fl = filler.length();
        while remaining >= fl {
            sb.write_string(&filler);
            remaining -= fl;
        }
        if remaining > 0 {
            sb.write_string(&filler.substring(0, remaining));
        }
        if start {
            sb.write_string(&s);
        }
        Ok(Value::String(sb.string()))
    }

    pub fn stringproto_pad_end(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self._string_pad(call, false)
    }

    pub fn stringproto_pad_start(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self._string_pad(call, true)
    }

    pub fn stringproto_repeat(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let n = call.arg(0).to_number(self)?;
        if matches!(n, Value::Float(f) if f == f64::INFINITY) {
            return Err(range_error(self, "Invalid count value"));
        }
        let num_int = n.to_integer(self)?;
        if num_int < 0 {
            return Err(range_error(self, "Invalid count value"));
        }
        if num_int == 0 || s.length() == 0 {
            return Ok(Value::String(STRING_EMPTY.clone()));
        }
        let num = num_int as usize;
        let units = StrUnits::of(&s);
        match (&units, units.ascii()) {
            (_, Some(a)) => {
                let mut sb = Vec::new();
                reserve_or_panic(&mut sb, total_size(a.len(), num));
                for _ in 0..num {
                    sb.extend_from_slice(a);
                }
                Ok(Value::String(JsString::Ascii(AsciiString::new(&sb))))
            }
            (StrUnits::Unicode(u), _) => {
                let mut sb = UnicodeStringBuilder::default();
                reserve_or_panic(&mut sb.buf, total_size(u.length(), num).saturating_add(1));
                for _ in 0..num {
                    sb.write_unicode_string(u);
                }
                Ok(Value::String(sb.string()))
            }
            _ => unreachable!("string units"),
        }
    }

    pub fn stringproto_replace(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let search_value = call.argument(0);
        let replace_value = call.argument(1);
        if !matches!(search_value, Value::Undefined | Value::Null) {
            let m = self.get_v(&search_value, &Value::Symbol(SYM_REPLACE))?;
            if let Some(replacer) = to_method(self, m)? {
                return replacer.call(self, search_value, &[call.this.clone(), replace_value]);
            }
        }
        let s = call.this.to_string(self)?;
        let mut found = Vec::new();
        let search_str = search_value.to_string(self)?;
        let pos = string_index(&s, &search_str, 0);
        if pos != -1 {
            found.push(vec![pos, pos + search_str.length() as isize]);
        }
        let (str_, rcall) = get_replace_value(self, &replace_value)?;
        string_replace(self, &s, &found, str_.as_ref(), rcall)
    }

    pub fn stringproto_replace_all(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let search_value = call.argument(0);
        let replace_value = call.argument(1);
        if !matches!(search_value, Value::Undefined | Value::Null) {
            if is_regexp(self, &search_value)?
                && let Value::Object(o) = &search_value
            {
                let flags = nil_safe(o.get_str(self, &"flags".into(), None)?);
                self.check_object_coercible(&flags)?;
                let f = flags.to_string(self)?.go_string();
                if !f.contains(&b'g') {
                    return Err(type_error(
                        self,
                        "String.prototype.replaceAll called with a non-global RegExp argument",
                    ));
                }
            }
            let m = self.get_v(&search_value, &Value::Symbol(SYM_REPLACE))?;
            if let Some(replacer) = to_method(self, m)? {
                return replacer.call(self, search_value, &[call.this.clone(), replace_value]);
            }
        }
        let s = call.this.to_string(self)?;
        let mut found = Vec::new();
        let search_str = search_value.to_string(self)?;
        let search_length = search_str.length();
        let advance_by = search_length.max(1);
        let mut pos = string_index(&s, &search_str, 0);
        while pos != -1 {
            found.push(vec![pos, pos + search_length as isize]);
            pos = string_index(&s, &search_str, pos as usize + advance_by);
        }
        let (str_, rcall) = get_replace_value(self, &replace_value)?;
        string_replace(self, &s, &found, str_.as_ref(), rcall)
    }

    pub fn stringproto_search(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let regexp = call.argument(0);
        if !matches!(regexp, Value::Undefined | Value::Null) {
            let m = self.get_v(&regexp, &Value::Symbol(SYM_SEARCH))?;
            if let Some(searcher) = to_method(self, m)? {
                return searcher.call(self, regexp, std::slice::from_ref(call.this));
            }
        }
        let mut rx = None;
        if let Value::Object(o) = &regexp
            && RegexpObject::data(self, *o).is_some()
        {
            rx = Some(*o);
        }
        let rx = match rx {
            Some(rx) => rx,
            None => {
                let proto = self.get_reg_exp_prototype();
                self.new_reg_exp(Some(&regexp), None, Some(proto))?
            }
        };
        let m = nil_safe(rx.get_sym(self, SYM_SEARCH, None)?);
        let m = self.to_object(&m)?;
        if let Some(searcher) = m.assert_callable(self) {
            let s = call.this.to_string(self)?;
            return searcher.call(self, Value::Object(rx), &[Value::String(s)]);
        }
        Err(type_error(self, "RegExp searcher is not a function"))
    }

    pub fn stringproto_slice(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let l = s.length() as i64;
        let mut start = call.arg(0).to_integer(self)?;
        let mut end = match call.arg(1) {
            Value::Undefined => l,
            a => a.to_integer(self)?,
        };
        if start < 0 {
            start += l;
            if start < 0 {
                start = 0;
            }
        } else if start > l {
            start = l;
        }
        if end < 0 {
            end += l;
            if end < 0 {
                end = 0;
            }
        } else if end > l {
            end = l;
        }
        if end > start {
            return Ok(Value::String(s.substring(start as usize, end as usize)));
        }
        Ok(Value::String(STRING_EMPTY.clone()))
    }

    pub fn stringproto_split(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let separator_value = call.argument(0);
        let limit_value = call.argument(1);
        if !matches!(separator_value, Value::Undefined | Value::Null) {
            let m = self.get_v(&separator_value, &Value::Symbol(SYM_SPLIT))?;
            if let Some(splitter) = to_method(self, m)? {
                return splitter.call(self, separator_value, &[call.this.clone(), limit_value]);
            }
        }
        let s = call.this.to_string(self)?;
        let mut limit: isize = -1;
        if !limit_value.is_undefined() {
            limit = to_uint32(self, &limit_value)? as isize;
        }
        let separator_value = separator_value.to_string_value(self)?;
        if limit == 0 {
            return Ok(Value::Object(self.new_array_values(Vec::new())));
        }
        if separator_value.is_undefined() {
            return Ok(Value::Object(self.new_array_values(vec![Value::String(s)])));
        }
        let split_limit = if limit > 0 { limit + 1 } else { limit };
        let s_units = StrUnits::of(&s);
        let sep_units = match &separator_value {
            Value::String(sep) => Some(StrUnits::of(sep)),
            _ => None,
        };
        if let (Some(a), Some(sep)) = (s_units.ascii(), sep_units.as_ref().and_then(|u| u.ascii()))
        {
            let mut split = split_n(a, sep, split_limit);
            if limit > 0 && split.len() > limit as usize {
                split.truncate(limit as usize);
            }
            let value_array = split
                .into_iter()
                .map(|v| Value::String(JsString::Ascii(AsciiString::new(v))))
                .collect();
            return Ok(Value::Object(self.new_array_values(value_array)));
        }
        let separator = match &separator_value {
            Value::String(sep) => sep.go_string(),
            other => other.go_string(self)?.into_bytes(),
        };
        let str_ = s.go_string();
        let mut split = split_n(&str_, &separator, split_limit);
        if limit > 0 && split.len() > limit as usize {
            split.truncate(limit as usize);
        }
        let value_array = split
            .into_iter()
            .map(|v| Value::String(new_string_value(v)))
            .collect();
        Ok(Value::Object(self.new_array_values(value_array)))
    }

    pub fn stringproto_starts_with(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let search_string = call.arg(0);
        if is_regexp(self, search_string)? {
            return Err(type_error(
                self,
                "First argument to String.prototype.startsWith must not be a regular expression",
            ));
        }
        let search_str = search_string.to_string(self)?;
        let l = s.length() as i64;
        let pos = match call.arg(1) {
            Value::Undefined => 0,
            p => p.to_integer(self)?,
        };
        let start = pos.clamp(0, l) as usize;
        let search_length = search_str.length();
        if (search_length + start) as i64 > l {
            return Ok(Value::Bool(false));
        }
        for i in 0..search_length {
            if s.char_at(start + i) != search_str.char_at(i) {
                return Ok(Value::Bool(false));
            }
        }
        Ok(Value::Bool(true))
    }

    pub fn stringproto_substring(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let l = s.length() as i64;
        let mut int_start = call.arg(0).to_integer(self)?;
        let mut int_end = match call.arg(1) {
            Value::Undefined => l,
            e => e.to_integer(self)?,
        };
        int_start = int_start.clamp(0, l);
        int_end = int_end.clamp(0, l);
        if int_start > int_end {
            std::mem::swap(&mut int_start, &mut int_end);
        }
        Ok(Value::String(
            s.substring(int_start as usize, int_end as usize),
        ))
    }

    pub fn stringproto_to_lower_case(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        Ok(Value::String(string_to_lower(&s)))
    }

    pub fn stringproto_to_upper_case(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        Ok(Value::String(string_to_upper(&s)))
    }

    pub fn stringproto_trim(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        Ok(Value::String(string_trim(&s, TrimKind::Both)))
    }

    pub fn stringproto_trim_end(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        Ok(Value::String(string_trim(&s, TrimKind::End)))
    }

    pub fn stringproto_trim_start(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        Ok(Value::String(string_trim(&s, TrimKind::Start)))
    }

    pub fn stringproto_substr(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.check_object_coercible(call.this)?;
        let s = call.this.to_string(self)?;
        let mut start = call.arg(0).to_integer(self)?;
        let sl = s.length() as i64;
        let mut length = match call.arg(1) {
            Value::Undefined => sl,
            a => a.to_integer(self)?,
        };
        if start < 0 {
            start = (sl + start).max(0);
        }
        length = length.max(0).min(sl - start);
        if length <= 0 {
            return Ok(Value::String(STRING_EMPTY.clone()));
        }
        Ok(Value::String(
            s.substring(start as usize, (start + length) as usize),
        ))
    }

    pub fn string_iter_proto_next(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        if let ObjExt::StringIter(_) = &self.obj(this_obj).ext {
            return StringIterObject::next(self, this_obj);
        }
        let s = self
            .objectproto_to_string(FunctionCall::new(&Value::Object(this_obj), &[]))?
            .go_string(self)?;
        Err(type_error(
            self,
            &format!("Method String Iterator.prototype.next called on incompatible receiver {s}"),
        ))
    }

    pub fn create_string_iter_proto(&mut self, val: ObjRef) {
        let proto = self.get_iterator_prototype();
        self.new_base_object_obj(val, Some(proto), CLASS_OBJECT);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.string_iter_proto_next(c)), "next", 0);
        val._put_prop(self, &"next".into(), Value::Object(f), true, false, true);
        val._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_STRING_ITERATOR)),
                false,
                false,
                true,
            ),
        );
    }

    pub fn get_string_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.string_iterator_prototype {
            return r;
        }
        let o = self.reserve_object();
        self.global.string_iterator_prototype = Some(o);
        self.create_string_iter_proto(o);
        o
    }

    pub fn get_stringproto_trim_end(&mut self) -> ObjRef {
        if let Some(r) = self.global.stringproto_trim_end {
            return r;
        }
        let ret =
            self.new_native_func(NativeFn::Fn(|r, c| r.stringproto_trim_end(c)), "trimEnd", 0);
        self.global.stringproto_trim_end = Some(ret);
        ret
    }

    pub fn get_stringproto_trim_start(&mut self) -> ObjRef {
        if let Some(r) = self.global.stringproto_trim_start {
            return r;
        }
        let ret = self.new_native_func(
            NativeFn::Fn(|r, c| r.stringproto_trim_start(c)),
            "trimStart",
            0,
        );
        self.global.stringproto_trim_start = Some(ret);
        ret
    }

    pub fn get_string_singleton(&mut self) -> JsResult<ObjRef> {
        if let Some(r) = self.string_singleton {
            return Ok(r);
        }
        let c = self.get_string();
        let ret = self.builtin_new(c, &[])?;
        self.string_singleton = Some(ret);
        Ok(ret)
    }

    pub fn get_string(&mut self) -> ObjRef {
        if let Some(r) = self.global.string {
            return r;
        }
        let ret = self.reserve_object();
        self.global.string = Some(ret);
        let proto = self.get_string_prototype();
        let ctor = self.wrap_native_construct(
            Some(|r: &mut Runtime, args: &[Value], proto: Option<ObjRef>| {
                r.builtin_new_string(args, proto)
            }),
            ret,
            Some(proto),
        );
        self.new_native_func_and_construct(
            Some(ret),
            Some(NativeFn::Fn(|r, c| r.builtin_string(c))),
            ctor,
            Some(proto),
            "String",
            int_to_value(1),
        );
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.string_fromcharcode(c)),
            "fromCharCode",
            1,
        );
        ret._put_prop(
            self,
            &"fromCharCode".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.string_fromcodepoint(c)),
            "fromCodePoint",
            1,
        );
        ret._put_prop(
            self,
            &"fromCodePoint".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.string_raw(c)), "raw", 1);
        ret._put_prop(self, &"raw".into(), Value::Object(f), true, false, true);
        ret
    }

    pub fn get_string_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.string_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.string_prototype = Some(ret);
        self.new_templated_object(get_string_proto_template(), Some(ret));
        self.obj_mut(ret).base.class = CLASS_STRING;
        ret
    }
}

fn total_size(unit: usize, num: usize) -> usize {
    match unit.checked_mul(num) {
        Some(n) => n,
        None => panic!("strings: repeat count causes overflow"),
    }
}

fn reserve_or_panic<T>(v: &mut Vec<T>, n: usize) {
    if v.try_reserve_exact(n).is_err() {
        panic!("out of memory");
    }
}
