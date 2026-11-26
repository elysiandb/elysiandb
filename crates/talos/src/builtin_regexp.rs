use crate::array::go_append;
use crate::builtin_string::{
    StrUnits, get_replace_value, lenient_decode_at, lenient_decode_units, string_replace,
    write_substitution,
};
use crate::builtin_symbol::{
    SYM_MATCH, SYM_MATCH_ALL, SYM_REPLACE, SYM_SEARCH, SYM_SPLIT, SYM_TO_STRING_TAG,
};
use crate::deps::regexp;
use crate::deps::regexp::syntax::ErrorCode;
use crate::func::{NativeFn, NativeFnPtr};
use crate::heap::{ObjRef, SymRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_OBJECT, CLASS_REG_EXP_STRING_ITERATOR, CallFn, FunctionCall,
    GuardedObject, ObjClass, ObjData, ObjExt, UStr, to_method,
};
use crate::parser::{TransformError, transform_reg_exp};
use crate::regexp::{LAST_INDEX, RegexpObject, RegexpPattern, RegexpWrapper, compile_regexp2};
use crate::runtime::{JsResult, Runtime, Throw, to_length, to_uint32};
use crate::string::{JsString, STRING_EMPTY, StringBuilder, ascii_string};
use crate::string_ascii::AsciiString;
use crate::value::{MAX_INT, Value, ValueProperty, int_to_value, nil_safe, value_prop};
use gocompat::utf8;
use gocompat::utf16;
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, LazyLock};

static EXEC: LazyLock<UStr> = LazyLock::new(|| UStr::new("exec"));
static SOURCE: LazyLock<UStr> = LazyLock::new(|| UStr::new("source"));
static FLAGS: LazyLock<UStr> = LazyLock::new(|| UStr::new("flags"));
static INDEX: LazyLock<UStr> = LazyLock::new(|| UStr::new("index"));

const HEX: &[u8] = b"0123456789abcdef";

fn is_utf16_first_surrogate(c: u16) -> bool {
    (0xD800..=0xDBFF).contains(&c)
}

fn is_utf16_second_surrogate(c: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&c)
}

fn decode_hex(s: &[u8]) -> Option<i64> {
    let mut hex: i64 = 0;
    for &chr in s {
        let n = match chr {
            b'0'..=b'9' => chr - b'0',
            b'a'..=b'f' => chr - b'a' + 10,
            b'A'..=b'F' => chr - b'A' + 10,
            _ => return None,
        };
        hex = hex * 16 + i64::from(n);
    }
    Some(hex)
}

fn write_hex4(b: &mut Vec<u8>, i: i64) {
    b.push(HEX[(i >> 12) as usize]);
    b.push(HEX[((i >> 8) & 0xF) as usize]);
    b.push(HEX[((i >> 4) & 0xF) as usize]);
    b.push(HEX[(i & 0xF) as usize]);
}

pub fn convert_regexp_to_unicode(pattern_str: &[u8]) -> Vec<u8> {
    let mut sb = Vec::new();
    let mut pos = 0usize;
    let mut i = 0usize;
    while (i as isize) < pattern_str.len() as isize - 11 {
        let (r, size) = utf8::decode_rune(&pattern_str[i..]);
        if r == '\\' as i32 {
            i += 1;
            if pattern_str[i] == b'u'
                && pattern_str[i + 5] == b'\\'
                && pattern_str[i + 6] == b'u'
                && let Some(first) = decode_hex(&pattern_str[i + 1..i + 5])
                && is_utf16_first_surrogate(first as u16)
                && let Some(second) = decode_hex(&pattern_str[i + 7..i + 11])
                && is_utf16_second_surrogate(second as u16)
            {
                let r = utf16::decode_rune(first as i32, second as i32);
                sb.extend_from_slice(&pattern_str[pos..i - 1]);
                utf8::append_rune(&mut sb, r);
                i += 11;
                pos = i;
                continue;
            }
            i += 1;
        } else {
            i += size;
        }
    }
    if pos > 0 {
        sb.extend_from_slice(&pattern_str[pos..]);
        return sb;
    }
    pattern_str.to_vec()
}

pub fn convert_regexp_to_utf16(pattern_str: &[u8]) -> Vec<u8> {
    let mut sb = Vec::new();
    let mut pos = 0usize;
    let mut prev_rune = 0i32;
    let mut i = 0usize;
    while i < pattern_str.len() {
        let (r, size) = utf8::decode_rune(&pattern_str[i..]);
        if r > 0xFFFF {
            sb.extend_from_slice(&pattern_str[pos..i]);
            if prev_rune == '\\' as i32 {
                sb.push(b'\\');
            }
            let (first, second) = utf16::encode_rune(r);
            sb.extend_from_slice(b"\\u");
            write_hex4(&mut sb, i64::from(first));
            sb.extend_from_slice(b"\\u");
            write_hex4(&mut sb, i64::from(second));
            pos = i + size;
        }
        i += size;
        prev_rune = r;
    }
    if pos > 0 {
        sb.extend_from_slice(&pattern_str[pos..]);
        return sb;
    }
    pattern_str.to_vec()
}

fn invalid_pattern(pattern: &[u8], reason: &str) -> String {
    format!(
        "Invalid regular expression: /{}/: {}",
        String::from_utf8_lossy(pattern),
        reason
    )
}

pub fn compile_regexp(pattern: &[u8], flags: &[u8]) -> Result<RegexpPattern, String> {
    let (mut global, mut ignore_case, mut multiline, mut dot_all, mut sticky, mut unicode) =
        (false, false, false, false, false, false);
    if !flags.is_empty() {
        let invalid_flags = || {
            format!(
                "Invalid flags supplied to RegExp constructor '{}'",
                String::from_utf8_lossy(flags)
            )
        };
        let mut i = 0;
        while i < flags.len() {
            let (chr, size) = utf8::decode_rune(&flags[i..]);
            i += size;
            let flag = match u8::try_from(chr) {
                Ok(c) => c,
                Err(_) => return Err(invalid_flags()),
            };
            let seen = match flag {
                b'g' => &mut global,
                b'm' => &mut multiline,
                b's' => &mut dot_all,
                b'i' => &mut ignore_case,
                b'y' => &mut sticky,
                b'u' => {
                    unicode = true;
                    continue;
                }
                _ => return Err(invalid_flags()),
            };
            if *seen {
                return Err(invalid_flags());
            }
            *seen = true;
        }
    }

    let pattern_str = if unicode {
        convert_regexp_to_unicode(pattern)
    } else {
        convert_regexp_to_utf16(pattern)
    };

    let mut wrapper = None;
    match transform_reg_exp(&pattern_str, dot_all, unicode) {
        Ok(re2_str) => {
            let mut re2_flags = String::new();
            if multiline {
                re2_flags.push('m');
            }
            if dot_all {
                re2_flags.push('s');
            }
            if ignore_case {
                re2_flags.push('i');
            }
            let re2_str = if !re2_flags.is_empty() {
                let mut s = format!("(?{re2_flags}:").into_bytes();
                s.extend_from_slice(&re2_str);
                s.push(b')');
                s
            } else {
                re2_str
            };
            match regexp::compile(&re2_str) {
                Ok(p) => wrapper = Some(Arc::new(RegexpWrapper(p))),
                Err(e) => {
                    if e.code != ErrorCode::InvalidRepeatSize {
                        return Err(invalid_pattern(pattern, e.code.reason()));
                    }
                }
            }
        }
        Err(e) => {
            if let TransformError::Syntax(_) = e {
                return Err(e.message().to_string());
            }
        }
    }

    let mut wrapper2 = None;
    if wrapper.is_none() {
        match compile_regexp2(&pattern_str, multiline, dot_all, ignore_case, unicode) {
            Ok(w) => wrapper2 = Some(w),
            Err(e) => return Err(invalid_pattern(pattern, &e.reason())),
        }
    }

    Ok(RegexpPattern {
        src: pattern_str,
        global,
        ignore_case,
        multiline,
        dot_all,
        sticky,
        unicode,
        regexp_wrapper: wrapper,
        regexp2_wrapper: wrapper2,
    })
}

pub fn escape_invalid_utf16(s: &JsString) -> Vec<u8> {
    let u = match s {
        JsString::Imported(i) => return i.utf8().to_vec(),
        JsString::Ascii(a) => return a.as_bytes().to_vec(),
        JsString::Unicode(u) => u,
    };
    let units = &u.raw()[1..];
    let mut sb: Vec<u8> = Vec::new();
    let mut pos = 0usize;
    let mut i = 0usize;
    while i < units.len() {
        let (c, size) = lenient_decode_units(units, i);
        i += size;
        if utf16::is_surrogate(c) {
            if sb.is_empty() {
                let mut last = 0i32;
                let mut p = 0usize;
                while p < pos {
                    let (c, size) = lenient_decode_units(units, p);
                    utf8::append_rune(&mut sb, c);
                    last = c;
                    p += size;
                }
                if last == '\\' as i32 {
                    utf8::append_rune(&mut sb, last);
                }
            }
            sb.extend_from_slice(b"\\u");
            write_hex4(&mut sb, i64::from(c));
        } else if !sb.is_empty() {
            utf8::append_rune(&mut sb, c);
        } else {
            pos += size;
        }
    }
    if !sb.is_empty() {
        return sb;
    }
    s.go_string()
}

pub fn compile_regexp_from_value_string(
    pattern_str: &JsString,
    flags: &[u8],
) -> Result<RegexpPattern, String> {
    compile_regexp(&escape_invalid_utf16(pattern_str), flags)
}

pub fn is_regexp(rt: &mut Runtime, v: &Value) -> JsResult<bool> {
    if let Value::Object(o) = v {
        let matcher = o.get_sym(rt, SYM_MATCH, None)?;
        if let Some(m) = matcher
            && !m.is_undefined()
        {
            return Ok(m.to_boolean());
        }
        return Ok(RegexpObject::data(rt, *o).is_some());
    }
    Ok(false)
}

pub fn advance_string_index(s: &JsString, pos: usize, unicode: bool) -> usize {
    let next = pos + 1;
    if !unicode {
        return next;
    }
    let l = s.length();
    if next >= l {
        return next;
    }
    if !is_utf16_first_surrogate(s.char_at(pos)) {
        return next;
    }
    if !is_utf16_second_surrogate(s.char_at(next)) {
        return next;
    }
    next + 1
}

pub fn advance_string_index64(s: &JsString, pos: i64, unicode: bool) -> i64 {
    let next = pos + 1;
    if !unicode {
        return next;
    }
    let l = s.length() as i64;
    if next >= l {
        return next;
    }
    if !is_utf16_first_surrogate(s.char_at(pos as usize)) {
        return next;
    }
    if !is_utf16_second_surrogate(s.char_at(next as usize)) {
        return next;
    }
    next + 1
}

pub struct RegExpStringIterObject {
    pub matcher: ObjRef,
    pub s: JsString,
    pub global: bool,
    pub full_unicode: bool,
    pub done: bool,
}

impl ObjData for RegExpStringIterObject {
    fn trace(&self, t: &mut Tracer) {
        t.obj(self.matcher);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub static REG_EXP_STRING_ITER_OBJECT_CLASS: ObjClass = BASE_CLASS;

pub fn reg_exp_exec(rt: &mut Runtime, r: ObjRef, s: &JsString) -> JsResult<Value> {
    let exec = r.get_str(rt, &EXEC, None)?;
    if let Some(Value::Object(exec_object)) = exec
        && let Some(exec_fn) = exec_object.assert_callable(rt)
    {
        return rt.reg_exp_exec(exec_fn, r, Value::String(s.clone()));
    }
    if RegexpObject::data(rt, r).is_some() {
        return RegexpObject::exec(rt, r, s);
    }
    Err(type_error(rt, "no RegExpMatcher internal slot"))
}

impl RegExpStringIterObject {
    fn state(rt: &Runtime, o: ObjRef) -> (bool, ObjRef, JsString, bool, bool) {
        match rt.ext_dyn::<RegExpStringIterObject>(o) {
            Some(ri) => (
                ri.done,
                ri.matcher,
                ri.s.clone(),
                ri.global,
                ri.full_unicode,
            ),
            None => unreachable!("not a regexp string iterator"),
        }
    }

    fn set_done(rt: &mut Runtime, o: ObjRef) {
        if let Some(ri) = rt.ext_dyn_mut::<RegExpStringIterObject>(o) {
            ri.done = true;
        }
    }

    pub fn next(rt: &mut Runtime, o: ObjRef) -> JsResult<Value> {
        let (done, matcher, s, global, full_unicode) = RegExpStringIterObject::state(rt, o);
        if done {
            return Ok(rt.create_iter_result_object(Value::Undefined, true));
        }
        let m = reg_exp_exec(rt, matcher, &s)?;
        if m.is_null() {
            RegExpStringIterObject::set_done(rt, o);
            return Ok(rt.create_iter_result_object(Value::Undefined, true));
        }
        if !global {
            RegExpStringIterObject::set_done(rt, o);
            return Ok(rt.create_iter_result_object(m, false));
        }
        let mo = rt.to_object(&m)?;
        let match_str = nil_safe(mo.get_idx(rt, 0, None)?).to_string(rt)?;
        if match_str.length() == 0 {
            let li = nil_safe(matcher.get_str(rt, &LAST_INDEX, None)?);
            let this_index = to_length(rt, &li)?;
            matcher.set_own_str(
                rt,
                &LAST_INDEX,
                Value::Int(advance_string_index64(&s, this_index, full_unicode)),
                true,
            )?;
        }
        Ok(rt.create_iter_result_object(m, false))
    }
}

fn type_error(rt: &mut Runtime, msg: &str) -> Throw {
    Throw::Value(rt.new_type_error(msg))
}

impl RegexpObject {
    pub fn write_escaped_source(source: &JsString, sb: &mut StringBuilder) -> bool {
        if source.length() == 0 {
            sb.write_string(&ascii_string("(?:)"));
            return true;
        }
        let units = StrUnits::of(source);
        let n = units.len();
        let mut pos = 0usize;
        let mut last_pos = 0usize;
        let mut i = 0usize;
        while i < n {
            let (c, mut size) = lenient_decode_at(&units, i);
            i += size;
            match c {
                0x5c => {
                    pos += 1;
                    if i >= n {
                        break;
                    }
                    let (_, s2) = lenient_decode_at(&units, i);
                    i += s2;
                    size = s2;
                }
                0x2f | 0x0a | 0x0d | 0x2028 | 0x2029 => {
                    sb.write_substring(source, last_pos, pos);
                    sb.write_rune('\\' as i32);
                    match c {
                        0x0a => sb.write_rune('n' as i32),
                        0x0d => sb.write_rune('r' as i32),
                        _ => {
                            sb.write_rune('u' as i32);
                            sb.write_rune(i32::from(HEX[(c >> 12) as usize]));
                            sb.write_rune(i32::from(HEX[((c >> 8) & 0xF) as usize]));
                            sb.write_rune(i32::from(HEX[((c >> 4) & 0xF) as usize]));
                            sb.write_rune(i32::from(HEX[(c & 0xF) as usize]));
                        }
                    }
                    last_pos = pos + size;
                }
                _ => {}
            }
            pos += size;
        }
        if last_pos > 0 {
            sb.write_substring(source, last_pos, source.length());
            return true;
        }
        false
    }
}

impl Runtime {
    pub fn new_reg_expp(
        &mut self,
        pattern: RegexpPattern,
        pattern_str: JsString,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        self.new_regexp_object(proto, Rc::new(RefCell::new(pattern)), pattern_str)
    }

    pub fn _new_reg_exp(
        &mut self,
        pattern_str: JsString,
        flags: &[u8],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        match compile_regexp_from_value_string(&pattern_str, flags) {
            Ok(pattern) => Ok(self.new_reg_expp(pattern, pattern_str, proto)),
            Err(e) => Err(Throw::Value(self.new_syntax_error(&e, -1))),
        }
    }

    pub fn builtin_new_reg_exp(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        self.new_reg_exp(args.first(), args.get(1), proto)
    }

    pub fn new_reg_exp(
        &mut self,
        pattern_val: Option<&Value>,
        flags_val: Option<&Value>,
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let mut pattern = None;
        let mut flags = Vec::new();
        'exit: {
            if let Some(pv) = pattern_val
                && is_regexp(self, pv)?
                && let Value::Object(obj) = pv
            {
                let obj = *obj;
                if RegexpObject::data(self, obj).is_some() {
                    return match flags_val {
                        None | Some(Value::Undefined) => Ok(RegexpObject::clone(self, obj)),
                        Some(f) => {
                            let source = RegexpObject::source_of(self, obj);
                            let flags = f.to_string(self)?.go_string();
                            self._new_reg_exp(source, &flags, proto)
                        }
                    };
                }
                let source = nil_safe(obj.get_str(self, &SOURCE, None)?);
                pattern = Some(source.to_string(self)?);
                flags = match flags_val {
                    None | Some(Value::Undefined) => {
                        let f = nil_safe(obj.get_str(self, &FLAGS, None)?);
                        f.to_string(self)?.go_string()
                    }
                    Some(f) => f.to_string(self)?.go_string(),
                };
                break 'exit;
            }
            if let Some(pv) = pattern_val
                && !pv.is_undefined()
            {
                pattern = Some(pv.to_string(self)?);
            }
            if let Some(fv) = flags_val
                && !fv.is_undefined()
            {
                flags = fv.to_string(self)?.go_string();
            }
        }
        let pattern = pattern.unwrap_or_else(|| STRING_EMPTY.clone());
        self._new_reg_exp(pattern, &flags, proto)
    }

    pub fn builtin_reg_exp(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let pattern = call.argument(0);
        let pattern_is_reg_exp = is_regexp(self, &pattern)?;
        let flags = call.argument(1);
        if pattern_is_reg_exp
            && flags.is_undefined()
            && let Value::Object(obj) = &pattern
        {
            let pattern_constructor = obj.get_str(self, &"constructor".into(), None)?;
            if let Some(Value::Object(c)) = pattern_constructor
                && Some(c) == self.global.reg_exp
            {
                return Ok(pattern);
            }
        }
        let proto = self.get_reg_exp_prototype();
        Ok(Value::Object(self.new_reg_exp(
            Some(&pattern),
            Some(&flags),
            Some(proto),
        )?))
    }

    fn incompatible_receiver(&mut self, msg: &str, this: &Value) -> Throw {
        let s = match self.objectproto_to_string(FunctionCall::new(this, &[])) {
            Ok(v) => match v.go_string(self) {
                Ok(s) => s,
                Err(e) => return e,
            },
            Err(e) => return e,
        };
        type_error(self, &format!("{msg} {s}"))
    }

    pub fn regexpproto_compile(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this = self.to_object(call.this)?;
        if RegexpObject::data(self, this).is_some() {
            let pattern_val = call.arg(0);
            let flags_val = call.arg(1);
            'exit: {
                if let Value::Object(o) = pattern_val
                    && RegexpObject::data(self, *o).is_some()
                {
                    if !flags_val.is_undefined() {
                        return Err(type_error(
                            self,
                            "Cannot supply flags when constructing one RegExp from another",
                        ));
                    }
                    let pattern = RegexpObject::pattern_of(self, *o);
                    let source = RegexpObject::source_of(self, *o);
                    if let Some(r) = self.ext_dyn_mut::<RegexpObject>(this) {
                        r.pattern = pattern;
                        r.source = source;
                    }
                    break 'exit;
                }
                let source = if pattern_val.is_undefined() {
                    STRING_EMPTY.clone()
                } else {
                    pattern_val.to_string(self)?
                };
                let flags = if flags_val.is_undefined() {
                    Vec::new()
                } else {
                    flags_val.to_string(self)?.go_string()
                };
                let pattern = match compile_regexp_from_value_string(&source, &flags) {
                    Ok(p) => p,
                    Err(e) => return Err(Throw::Value(self.new_syntax_error(&e, -1))),
                };
                if let Some(r) = self.ext_dyn_mut::<RegexpObject>(this) {
                    r.pattern = Rc::new(RefCell::new(pattern));
                    r.source = source;
                }
            }
            this.set_own_str(self, &LAST_INDEX, int_to_value(0), true)?;
            return Ok(call.this.clone());
        }
        Err(self.incompatible_receiver(
            "Method RegExp.prototype.compile called on incompatible receiver",
            call.this,
        ))
    }

    pub fn regexpproto_exec(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this = self.to_object(call.this)?;
        if RegexpObject::data(self, this).is_some() {
            let s = call.arg(0).to_string(self)?;
            return RegexpObject::exec(self, this, &s);
        }
        Err(self.incompatible_receiver(
            "Method RegExp.prototype.exec called on incompatible receiver",
            call.this,
        ))
    }

    pub fn regexpproto_test(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this = self.to_object(call.this)?;
        if RegexpObject::data(self, this).is_some() {
            let s = call.arg(0).to_string(self)?;
            return Ok(Value::Bool(RegexpObject::test(self, this, &s)?));
        }
        Err(self.incompatible_receiver(
            "Method RegExp.prototype.test called on incompatible receiver",
            call.this,
        ))
    }

    pub fn regexpproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = self.to_object(call.this)?;
        if let Some(this) = self.check_std_regexp(obj) {
            let source = RegexpObject::source_of(self, this);
            let pattern = RegexpObject::pattern_of(self, this);
            let p = pattern.borrow();
            let mut sb = StringBuilder::new();
            sb.write_rune('/' as i32);
            if !RegexpObject::write_escaped_source(&source, &mut sb) {
                sb.write_string(&source);
            }
            sb.write_rune('/' as i32);
            if p.global {
                sb.write_rune('g' as i32);
            }
            if p.ignore_case {
                sb.write_rune('i' as i32);
            }
            if p.multiline {
                sb.write_rune('m' as i32);
            }
            if p.dot_all {
                sb.write_rune('s' as i32);
            }
            if p.unicode {
                sb.write_rune('u' as i32);
            }
            if p.sticky {
                sb.write_rune('y' as i32);
            }
            return Ok(Value::String(sb.string()));
        }
        let pattern = nil_safe(obj.get_str(self, &SOURCE, None)?).to_string(self)?;
        let flags = nil_safe(obj.get_str(self, &FLAGS, None)?).to_string(self)?;
        let mut sb = StringBuilder::new();
        sb.write_rune('/' as i32);
        sb.write_string(&pattern);
        sb.write_rune('/' as i32);
        sb.write_string(&flags);
        Ok(Value::String(sb.string()))
    }

    pub fn regexpproto_get_source(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this = self.to_object(call.this)?;
        if RegexpObject::data(self, this).is_some() {
            let source = RegexpObject::source_of(self, this);
            let mut sb = StringBuilder::new();
            if RegexpObject::write_escaped_source(&source, &mut sb) {
                return Ok(Value::String(sb.string()));
            }
            return Ok(Value::String(source));
        }
        if matches!(call.this, Value::Object(o) if Some(*o) == self.global.reg_exp_prototype) {
            return Ok(Value::String(ascii_string("(?:)")));
        }
        Err(type_error(
            self,
            "Method RegExp.prototype.source getter called on incompatible receiver",
        ))
    }

    fn regexpproto_get_flag(
        &mut self,
        call: FunctionCall<'_>,
        name: &str,
        flag: fn(&RegexpPattern) -> bool,
    ) -> JsResult<Value> {
        let this = self.to_object(call.this)?;
        if RegexpObject::data(self, this).is_some() {
            let pattern = RegexpObject::pattern_of(self, this);
            return Ok(Value::Bool(flag(&pattern.borrow())));
        }
        if matches!(call.this, Value::Object(o) if Some(*o) == self.global.reg_exp_prototype) {
            return Ok(Value::Undefined);
        }
        Err(self.incompatible_receiver(
            &format!("Method RegExp.prototype.{name} getter called on incompatible receiver"),
            call.this,
        ))
    }

    pub fn regexpproto_get_global(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.regexpproto_get_flag(call, "global", |p| p.global)
    }

    pub fn regexpproto_get_multiline(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.regexpproto_get_flag(call, "multiline", |p| p.multiline)
    }

    pub fn regexpproto_get_dot_all(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.regexpproto_get_flag(call, "dotAll", |p| p.dot_all)
    }

    pub fn regexpproto_get_ignore_case(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.regexpproto_get_flag(call, "ignoreCase", |p| p.ignore_case)
    }

    pub fn regexpproto_get_unicode(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.regexpproto_get_flag(call, "unicode", |p| p.unicode)
    }

    pub fn regexpproto_get_sticky(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.regexpproto_get_flag(call, "sticky", |p| p.sticky)
    }

    pub fn regexpproto_get_flags(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        let get = |rt: &mut Runtime, name: &str| -> JsResult<bool> {
            Ok(match this_obj.get_str(rt, &name.into(), None)? {
                Some(v) => v.to_boolean(),
                None => false,
            })
        };
        let global = get(self, "global")?;
        let ignore_case = get(self, "ignoreCase")?;
        let multiline = get(self, "multiline")?;
        let dot_all = get(self, "dotAll")?;
        let sticky = get(self, "sticky")?;
        let unicode = get(self, "unicode")?;
        let mut sb = Vec::with_capacity(6);
        if global {
            sb.push(b'g');
        }
        if ignore_case {
            sb.push(b'i');
        }
        if multiline {
            sb.push(b'm');
        }
        if dot_all {
            sb.push(b's');
        }
        if unicode {
            sb.push(b'u');
        }
        if sticky {
            sb.push(b'y');
        }
        Ok(Value::String(JsString::Ascii(AsciiString::new(&sb))))
    }

    pub fn reg_exp_exec(&mut self, exec_fn: CallFn, rx_obj: ObjRef, arg: Value) -> JsResult<Value> {
        let res = exec_fn.call(self, Value::Object(rx_obj), &[arg])?;
        if !res.is_null() && !res.is_object() {
            return Err(type_error(
                self,
                "RegExp exec method returned something other than an Object or null",
            ));
        }
        Ok(res)
    }

    fn reg_exp_exec_method(
        &mut self,
        exec_fn: Option<CallFn>,
        rx_obj: ObjRef,
        arg: Value,
    ) -> JsResult<Value> {
        match exec_fn {
            Some(f) => self.reg_exp_exec(f, rx_obj, arg),
            None => panic!("RegExp exec method is missing"),
        }
    }

    fn exec_fn_of(&mut self, rx_obj: ObjRef) -> JsResult<CallFn> {
        let e = nil_safe(rx_obj.get_str(self, &EXEC, None)?);
        let e = self.to_object(&e)?;
        match e.assert_callable(self) {
            Some(f) => Ok(f),
            None => Err(type_error(self, "exec is not a function")),
        }
    }

    pub fn get_global_regexp_matches(
        &mut self,
        rx_obj: ObjRef,
        s: &JsString,
        full_unicode: bool,
    ) -> JsResult<Vec<Value>> {
        rx_obj.set_own_str(self, &LAST_INDEX, int_to_value(0), true)?;
        let exec_fn = self.exec_fn_of(rx_obj)?;
        let mut a = Vec::new();
        loop {
            let res = self.reg_exp_exec(exec_fn, rx_obj, Value::String(s.clone()))?;
            if res.is_null() {
                break;
            }
            a.push(res.clone());
            let ro = self.to_object(&res)?;
            let match_str = nil_safe(ro.get_idx(self, 0, None)?).to_string(self)?;
            if match_str.length() == 0 {
                let li = nil_safe(rx_obj.get_str(self, &LAST_INDEX, None)?);
                let this_index = to_length(self, &li)?;
                rx_obj.set_own_str(
                    self,
                    &LAST_INDEX,
                    Value::Int(advance_string_index64(s, this_index, full_unicode)),
                    true,
                )?;
            }
        }
        Ok(a)
    }

    pub fn regexpproto_std_matcher_generic(
        &mut self,
        rx_obj: ObjRef,
        s: &JsString,
    ) -> JsResult<Value> {
        let flags = nil_safe(rx_obj.get_str(self, &FLAGS, None)?).go_string(self)?;
        let global = flags.contains('g');
        if global {
            let a = self.get_global_regexp_matches(rx_obj, s, flags.contains('u'))?;
            if a.is_empty() {
                return Ok(Value::Null);
            }
            let mut ar = Vec::with_capacity(a.len());
            for result in a {
                let obj = self.to_object(&result)?;
                let match_str = nil_safe(obj.get_idx(self, 0, None)?).to_string_value(self)?;
                ar.push(match_str);
            }
            return Ok(Value::Object(self.new_array_values(ar)));
        }
        let exec_fn = self.exec_fn_of(rx_obj)?;
        self.reg_exp_exec(exec_fn, rx_obj, Value::String(s.clone()))
    }

    pub fn check_std_regexp(&self, rx_obj: ObjRef) -> Option<ObjRef> {
        let rx = RegexpObject::data(self, rx_obj)?;
        if !rx.standard {
            return None;
        }
        let proto = self.obj(rx_obj).base.prototype?;
        if Some(proto) != self.global.std_regexp_proto
            || !matches!(self.obj(proto).ext, ObjExt::Guarded(_))
        {
            return None;
        }
        Some(rx_obj)
    }

    pub fn regexpproto_std_matcher(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        let s = call.arg(0).to_string(self)?;
        let Some(rx) = self.check_std_regexp(this_obj) else {
            return self.regexpproto_std_matcher_generic(this_obj, &s);
        };
        let pattern = RegexpObject::pattern_of(self, rx);
        if pattern.borrow().global {
            rx.set_own_str(self, &LAST_INDEX, int_to_value(0), true)?;
            let pattern = RegexpObject::pattern_of(self, rx);
            let mut p = pattern.borrow_mut();
            let sticky = p.sticky;
            let res = p.find_all_submatch_index(&s, 0, -1, sticky);
            drop(p);
            let res = match res {
                Some(r) if !r.is_empty() => r,
                _ => return Ok(Value::Null),
            };
            let units = StrUnits::of(&s);
            let a = res
                .iter()
                .map(|result| {
                    Value::String(units.substring(result[0] as usize, result[1] as usize))
                })
                .collect();
            return Ok(Value::Object(self.new_array_values(a)));
        }
        RegexpObject::exec(self, rx, &s)
    }

    pub fn regexpproto_std_search_generic(
        &mut self,
        rx_obj: ObjRef,
        arg: &JsString,
    ) -> JsResult<Value> {
        let previous_last_index = nil_safe(rx_obj.get_str(self, &LAST_INDEX, None)?);
        let zero = int_to_value(0);
        if !previous_last_index.same_as(&zero) {
            rx_obj.set_own_str(self, &LAST_INDEX, zero, true)?;
        }
        let exec_fn = self.exec_fn_of(rx_obj)?;
        let result = self.reg_exp_exec(exec_fn, rx_obj, Value::String(arg.clone()))?;
        let current_last_index = nil_safe(rx_obj.get_str(self, &LAST_INDEX, None)?);
        if !current_last_index.same_as(&previous_last_index) {
            rx_obj.set_own_str(self, &LAST_INDEX, previous_last_index, true)?;
        }
        if result.is_null() {
            return Ok(int_to_value(-1));
        }
        let r = self.to_object(&result)?;
        Ok(nil_safe(r.get_str(self, &INDEX, None)?))
    }

    pub fn regexpproto_std_matcher_all(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        let s = call.arg(0).to_string(self)?;
        let flags = nil_safe(this_obj.get_str(self, &FLAGS, None)?).to_string(self)?;
        let re = self.get_reg_exp();
        let c = self.species_constructor_obj(this_obj, re)?;
        let ctor = self.to_constructor(&Value::Object(c))?;
        let matcher = ctor.construct(
            self,
            &[call.this.clone(), Value::String(flags.clone())],
            None,
        )?;
        let li = nil_safe(this_obj.get_str(self, &LAST_INDEX, None)?);
        let li = to_length(self, &li)?;
        matcher.set_own_str(self, &LAST_INDEX, Value::Int(li), true)?;
        let flags_str = flags.go_string();
        let global = flags_str.contains(&b'g');
        let full_unicode = flags_str.contains(&b'u');
        Ok(self.create_reg_exp_string_iterator(matcher, s, global, full_unicode))
    }

    pub fn create_reg_exp_string_iterator(
        &mut self,
        matcher: ObjRef,
        s: JsString,
        global: bool,
        full_unicode: bool,
    ) -> Value {
        let proto = self.get_reg_exp_string_iterator_prototype();
        let o = self.alloc_object(
            &REG_EXP_STRING_ITER_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, Some(proto)),
            ObjExt::Dyn(Box::new(RegExpStringIterObject {
                matcher,
                s,
                global,
                full_unicode,
                done: false,
            })),
        );
        Value::Object(o)
    }

    pub fn regexpproto_std_search(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        let s = call.arg(0).to_string(self)?;
        let Some(rx) = self.check_std_regexp(this_obj) else {
            return self.regexpproto_std_search_generic(this_obj, &s);
        };
        let previous_last_index = nil_safe(rx.get_str(self, &LAST_INDEX, None)?);
        rx.set_own_str(self, &LAST_INDEX, int_to_value(0), true)?;
        let (matched, result) = RegexpObject::exec_regexp(self, rx, &s)?;
        rx.set_own_str(self, &LAST_INDEX, previous_last_index, true)?;
        if !matched {
            return Ok(int_to_value(-1));
        }
        match result {
            Some(r) => Ok(int_to_value(r[0] as i64)),
            None => Ok(int_to_value(-1)),
        }
    }

    pub fn regexpproto_std_splitter_generic(
        &mut self,
        splitter: ObjRef,
        s: &JsString,
        limit: &Value,
        unicode_matching: bool,
    ) -> JsResult<Value> {
        let mut a = Vec::new();
        let lim = if limit.is_undefined() {
            MAX_INT - 1
        } else {
            to_length(self, limit)?
        };
        if lim == 0 {
            return Ok(Value::Object(self.new_array_values(a)));
        }
        let size = s.length();
        let mut p = 0usize;
        let e = splitter.get_str(self, &EXEC, None)?;
        let exec_fn = to_method(self, e)?;
        if size == 0 {
            if self
                .reg_exp_exec_method(exec_fn, splitter, Value::String(s.clone()))?
                .is_null()
            {
                go_append(&mut a, Value::String(s.clone()));
            }
            return Ok(Value::Object(self.new_array_values(a)));
        }
        let mut q = p;
        while q < size {
            splitter.set_own_str(self, &LAST_INDEX, int_to_value(q as i64), true)?;
            let z = self.reg_exp_exec_method(exec_fn, splitter, Value::String(s.clone()))?;
            if z.is_null() {
                q = advance_string_index(s, q, unicode_matching);
                continue;
            }
            let z = self.to_object(&z)?;
            let li = nil_safe(splitter.get_str(self, &LAST_INDEX, None)?);
            let e = to_length(self, &li)?;
            if e == p as i64 {
                q = advance_string_index(s, q, unicode_matching);
                continue;
            }
            go_append(&mut a, Value::String(s.substring(p, q)));
            if a.len() as i64 == lim {
                return Ok(Value::Object(self.new_array_values(a)));
            }
            p = if e > size as i64 { size } else { e as usize };
            let l = nil_safe(z.get_str(self, &"length".into(), None)?);
            let number_of_captures = (to_length(self, &l)? - 1).max(0);
            for i in 1..=number_of_captures {
                go_append(&mut a, nil_safe(z.get_idx(self, i, None)?));
                if a.len() as i64 == lim {
                    return Ok(Value::Object(self.new_array_values(a)));
                }
            }
            q = p;
        }
        go_append(&mut a, Value::String(s.substring(p, size)));
        Ok(Value::Object(self.new_array_values(a)))
    }

    pub fn regexpproto_std_splitter(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let rx_obj = self.to_object(call.this)?;
        let s = call.arg(0).to_string(self)?;
        let limit_value = call.argument(1);
        let mut search = self.check_std_regexp(rx_obj);
        let re = self.get_reg_exp();
        let c = self.species_constructor_obj(rx_obj, re)?;
        if search.is_none() || Some(c) != self.global.reg_exp {
            let mut flags = nil_safe(rx_obj.get_str(self, &FLAGS, None)?).to_string(self)?;
            let flags_str = flags.go_string();
            if !flags_str.contains(&b'y') {
                flags = flags.concat(&ascii_string("y"));
            }
            let ctor = self.to_constructor(&Value::Object(c))?;
            let splitter =
                ctor.construct(self, &[Value::Object(rx_obj), Value::String(flags)], None)?;
            search = self.check_std_regexp(splitter);
            if search.is_none() {
                return self.regexpproto_std_splitter_generic(
                    splitter,
                    &s,
                    &limit_value,
                    flags_str.contains(&b'u'),
                );
            }
        }
        let Some(search) = search else {
            unreachable!("a standard splitter");
        };
        let pattern = RegexpObject::pattern_of(self, search);
        let mut limit: isize = -1;
        if !limit_value.is_undefined() {
            limit = to_uint32(self, &limit_value)? as isize;
        }
        if limit == 0 {
            return Ok(Value::Object(self.new_array_values(Vec::new())));
        }
        let target_length = s.length();
        let mut value_array = Vec::new();
        let mut last_index = 0usize;
        let mut found = 0isize;
        let result = pattern
            .borrow_mut()
            .find_all_submatch_index(&s, 0, -1, false);
        'ret: {
            if target_length == 0 {
                if result.is_none() {
                    go_append(&mut value_array, Value::String(s.clone()));
                }
                break 'ret;
            }
            for m in result.as_deref().unwrap_or(&[]) {
                if m[0] == m[1] && (m[0] == 0 || m[0] == target_length as isize) {
                    continue;
                }
                if last_index as isize != m[0] {
                    go_append(
                        &mut value_array,
                        Value::String(s.substring(last_index, m[0] as usize)),
                    );
                    found += 1;
                } else {
                    go_append(&mut value_array, Value::String(STRING_EMPTY.clone()));
                    found += 1;
                }
                last_index = m[1] as usize;
                if found == limit {
                    break 'ret;
                }
                let capture_count = m.len() / 2;
                for index in 1..capture_count {
                    let offset = index * 2;
                    let value = if m[offset] != -1 {
                        Value::String(s.substring(m[offset] as usize, m[offset + 1] as usize))
                    } else {
                        Value::Undefined
                    };
                    go_append(&mut value_array, value);
                    found += 1;
                    if found == limit {
                        break 'ret;
                    }
                }
            }
            if found != limit {
                if last_index != target_length {
                    go_append(
                        &mut value_array,
                        Value::String(s.substring(last_index, target_length)),
                    );
                } else {
                    go_append(&mut value_array, Value::String(STRING_EMPTY.clone()));
                }
            }
        }
        Ok(Value::Object(self.new_array_values(value_array)))
    }

    pub fn regexpproto_std_replacer_generic(
        &mut self,
        rx_obj: ObjRef,
        s: &JsString,
        replace_str: Option<&JsString>,
        rcall: Option<CallFn>,
    ) -> JsResult<Value> {
        let flags = nil_safe(rx_obj.get_str(self, &FLAGS, None)?).go_string(self)?;
        let is_global = flags.contains('g');
        let is_unicode = flags.contains('u');
        let results = if is_global {
            self.get_global_regexp_matches(rx_obj, s, is_unicode)?
        } else {
            let e = rx_obj.get_str(self, &EXEC, None)?;
            let exec_fn = to_method(self, e)?;
            let result = self.reg_exp_exec_method(exec_fn, rx_obj, Value::String(s.clone()))?;
            if result.is_null() {
                Vec::new()
            } else {
                vec![result]
            }
        };
        let length_s = s.length();
        let mut next_source_position = 0usize;
        let mut result_buf = StringBuilder::new();
        let empty = STRING_EMPTY.clone();
        let replace_str = replace_str.unwrap_or(&empty);
        for result in results {
            let obj = self.to_object(&result)?;
            let l = nil_safe(obj.get_str(self, &"length".into(), None)?);
            let n_captures = (to_length(self, &l)? - 1).max(0);
            let matched = nil_safe(obj.get_idx(self, 0, None)?).to_string(self)?;
            let match_length = matched.length();
            let index = nil_safe(obj.get_str(self, &INDEX, None)?).to_integer(self)?;
            let position = index.min(length_s as i64).max(0) as usize;
            let mut captures = Vec::with_capacity(n_captures as usize + 3);
            captures.push(Value::String(matched));
            for n in 1..=n_captures {
                let mut cap_n = nil_safe(obj.get_idx(self, n, None)?);
                if !cap_n.is_undefined() {
                    cap_n = cap_n.to_string_value(self)?;
                }
                captures.push(cap_n);
            }
            if let Some(rcall) = rcall {
                captures.push(int_to_value(position as i64));
                captures.push(Value::String(s.clone()));
                let replacement = rcall
                    .call(self, Value::Undefined, &captures)?
                    .to_string(self)?;
                if position >= next_source_position {
                    result_buf.write_string(&s.substring(next_source_position, position));
                    result_buf.write_string(&replacement);
                    next_source_position = position + match_length;
                }
            } else if position >= next_source_position {
                result_buf.write_string(&s.substring(next_source_position, position));
                let num_captures = captures.len();
                write_substitution(
                    s,
                    position,
                    num_captures,
                    |idx| {
                        let capture = &captures[idx];
                        if !capture.is_undefined() {
                            return capture.to_string(self);
                        }
                        Ok(STRING_EMPTY.clone())
                    },
                    replace_str,
                    &mut result_buf,
                )?;
                next_source_position = position + match_length;
            }
        }
        if next_source_position < length_s {
            result_buf.write_string(&s.substring(next_source_position, length_s));
        }
        Ok(Value::String(result_buf.string()))
    }

    pub fn regexpproto_std_replacer(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let rx_obj = self.to_object(call.this)?;
        let s = call.arg(0).to_string(self)?;
        let (replace_str, rcall) = get_replace_value(self, call.arg(1))?;
        let Some(rx) = self.check_std_regexp(rx_obj) else {
            return self.regexpproto_std_replacer_generic(rx_obj, &s, replace_str.as_ref(), rcall);
        };
        let mut index = 0i64;
        let mut find = 1isize;
        if RegexpObject::pattern_of(self, rx).borrow().global {
            find = -1;
        } else {
            index = RegexpObject::get_last_index(self, rx)?;
        }
        let pattern = RegexpObject::pattern_of(self, rx);
        let sticky = pattern.borrow().sticky;
        let found = pattern
            .borrow_mut()
            .find_all_submatch_index(&s, index as isize, find, sticky);
        let (global, sticky) = {
            let p = pattern.borrow();
            (p.global, p.sticky)
        };
        if global || sticky {
            let mut new_last_index = 0i64;
            if !global
                && let Some(f) = &found
                && let Some(last) = f.last()
            {
                new_last_index = last[1] as i64;
            }
            rx.set_own_str(self, &LAST_INDEX, int_to_value(new_last_index), true)?;
        }
        string_replace(
            self,
            &s,
            found.as_deref().unwrap_or(&[]),
            replace_str.as_ref(),
            rcall,
        )
    }

    pub fn reg_exp_string_iterator_proto_next(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        if self.ext_dyn::<RegExpStringIterObject>(this_obj).is_some() {
            return RegExpStringIterObject::next(self, this_obj);
        }
        Err(self.incompatible_receiver(
            "Method RegExp String Iterator.prototype.next called on incompatible receiver",
            &Value::Object(this_obj),
        ))
    }

    pub fn create_reg_exp_string_iterator_prototype(&mut self, val: ObjRef) {
        let proto = self.get_iterator_prototype();
        self.new_base_object_obj(val, Some(proto), CLASS_OBJECT);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.reg_exp_string_iterator_proto_next(c)),
            "next",
            0,
        );
        val._put_prop(self, &"next".into(), Value::Object(f), true, false, true);
        val._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_REG_EXP_STRING_ITERATOR)),
                false,
                false,
                true,
            ),
        );
    }

    pub fn get_reg_exp_string_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.reg_exp_string_iterator_prototype {
            return r;
        }
        let o = self.reserve_object();
        self.global.reg_exp_string_iterator_prototype = Some(o);
        self.create_reg_exp_string_iterator_prototype(o);
        o
    }

    pub fn get_reg_exp(&mut self) -> ObjRef {
        if let Some(r) = self.global.reg_exp {
            return r;
        }
        let ret = self.reserve_object();
        self.global.reg_exp = Some(ret);
        let proto = self.get_reg_exp_prototype();
        let ctor = self.wrap_native_construct(
            Some(|r: &mut Runtime, args: &[Value], proto: Option<ObjRef>| {
                r.builtin_new_reg_exp(args, proto)
            }),
            ret,
            Some(proto),
        );
        self.new_native_func_and_construct(
            Some(ret),
            Some(NativeFn::Fn(|r, c| r.builtin_reg_exp(c))),
            ctor,
            Some(proto),
            "RegExp",
            int_to_value(2),
        );
        self.put_species_return_this(ret);
        ret
    }

    fn put_getter(&mut self, o: ObjRef, name: &str, f: NativeFnPtr, fname: &str) -> JsResult<()> {
        let g = self.new_native_func(NativeFn::Fn(f), fname, 0);
        o.set_own_str(
            self,
            &name.into(),
            Value::Property(Box::new(ValueProperty {
                configurable: true,
                getter_func: Some(g),
                accessor: true,
                ..ValueProperty::default()
            })),
            false,
        )?;
        Ok(())
    }

    fn put_method_sym(&mut self, o: ObjRef, s: SymRef, f: NativeFnPtr, name: &str, length: i64) {
        let m = self.new_native_func(NativeFn::Fn(f), name, length);
        o._put_sym(self, s, value_prop(Value::Object(m), true, false, true));
    }

    fn put_method(&mut self, o: ObjRef, name: &str, f: NativeFnPtr, length: i64) {
        let m = self.new_native_func(NativeFn::Fn(f), name, length);
        o._put_prop(self, &name.into(), Value::Object(m), true, false, true);
    }

    pub fn get_reg_exp_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.reg_exp_prototype {
            return r;
        }
        let op = self.global.object_prototype;
        let o = self.new_guarded_object(op, CLASS_OBJECT);
        self.global.reg_exp_prototype = Some(o);
        self.global.std_regexp_proto = Some(o);

        let c = self.get_reg_exp();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(c),
            true,
            false,
            true,
        );
        self.put_method(o, "compile", |r, c| r.regexpproto_compile(c), 2);
        self.put_method(o, "exec", |r, c| r.regexpproto_exec(c), 1);
        self.put_method(o, "test", |r, c| r.regexpproto_test(c), 1);
        self.put_method(o, "toString", |r, c| r.regexpproto_to_string(c), 0);
        let getters: [(&str, NativeFnPtr, &str); 8] = [
            ("source", |r, c| r.regexpproto_get_source(c), "get source"),
            ("global", |r, c| r.regexpproto_get_global(c), "get global"),
            (
                "multiline",
                |r, c| r.regexpproto_get_multiline(c),
                "get multiline",
            ),
            ("dotAll", |r, c| r.regexpproto_get_dot_all(c), "get dotAll"),
            (
                "ignoreCase",
                |r, c| r.regexpproto_get_ignore_case(c),
                "get ignoreCase",
            ),
            (
                "unicode",
                |r, c| r.regexpproto_get_unicode(c),
                "get unicode",
            ),
            ("sticky", |r, c| r.regexpproto_get_sticky(c), "get sticky"),
            ("flags", |r, c| r.regexpproto_get_flags(c), "get flags"),
        ];
        for (name, f, fname) in getters {
            if self.put_getter(o, name, f, fname).is_err() {
                unreachable!("RegExp.prototype accessors are plain properties");
            }
        }

        self.put_method_sym(
            o,
            SYM_MATCH,
            |r, c| r.regexpproto_std_matcher(c),
            "[Symbol.match]",
            1,
        );
        self.put_method_sym(
            o,
            SYM_MATCH_ALL,
            |r, c| r.regexpproto_std_matcher_all(c),
            "[Symbol.matchAll]",
            1,
        );
        self.put_method_sym(
            o,
            SYM_SEARCH,
            |r, c| r.regexpproto_std_search(c),
            "[Symbol.search]",
            1,
        );
        self.put_method_sym(
            o,
            SYM_SPLIT,
            |r, c| r.regexpproto_std_splitter(c),
            "[Symbol.split]",
            2,
        );
        self.put_method_sym(
            o,
            SYM_REPLACE,
            |r, c| r.regexpproto_std_replacer(c),
            "[Symbol.replace]",
            2,
        );
        GuardedObject::guard(
            self,
            o,
            &[
                "exec",
                "global",
                "multiline",
                "ignoreCase",
                "unicode",
                "sticky",
            ],
        );
        o
    }
}
