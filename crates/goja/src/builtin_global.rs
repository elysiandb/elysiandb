use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{CLASS_GLOBAL, FunctionCall};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::{JsString, ascii_string, devirtualize_string};
use crate::string_ascii::AsciiString;
use crate::string_unicode::{UnicodeString, unicode_string_from_runes};
use crate::unistring;
use crate::value::{Value, float_to_value, int_to_value, value_prop};
use gocompat::utf8;
use std::sync::OnceLock;

const HEX_UPPER: &[u8] = b"0123456789ABCDEF";

fn parse_float_regexp() -> &'static crate::deps::regexp::Regexp {
    static R: OnceLock<crate::deps::regexp::Regexp> = OnceLock::new();
    R.get_or_init(|| {
        crate::deps::regexp::must_compile(
            br"^([+-]?(?:Infinity|[0-9]*\.?[0-9]*(?:[eE][+-]?[0-9]+)?))",
        )
    })
}

struct UriTables {
    uri_unescaped: [bool; 256],
    uri_reserved_hash: [bool; 256],
    uri_reserved_unescaped_hash: [bool; 256],
    empty_escape_set: [bool; 256],
}

fn uri_tables() -> &'static UriTables {
    static T: OnceLock<UriTables> = OnceLock::new();
    T.get_or_init(|| {
        let mut uri_unescaped = [false; 256];
        let mut uri_reserved = [false; 256];
        let mut uri_reserved_hash = [false; 256];
        let mut uri_reserved_unescaped_hash = [false; 256];
        for c in "abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.!~*'()".bytes() {
            uri_unescaped[c as usize] = true;
        }
        for c in ";/?:@&=+$,".bytes() {
            uri_reserved[c as usize] = true;
        }
        for i in 0..256 {
            if uri_unescaped[i] || uri_reserved[i] {
                uri_reserved_unescaped_hash[i] = true;
            }
            uri_reserved_hash[i] = uri_reserved[i];
        }
        uri_reserved_unescaped_hash[b'#' as usize] = true;
        uri_reserved_hash[b'#' as usize] = true;
        UriTables {
            uri_unescaped,
            uri_reserved_hash,
            uri_reserved_unescaped_hash,
            empty_escape_set: [false; 256],
        }
    })
}

fn ishex(c: u8) -> bool {
    c.is_ascii_hexdigit()
}

fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn digit_val(d: u8) -> i64 {
    match d {
        b'0'..=b'9' => i64::from(d - b'0'),
        b'a'..=b'z' => i64::from(d - b'a' + 10),
        b'A'..=b'Z' => i64::from(d - b'A' + 10),
        _ => 36,
    }
}

#[cfg(target_arch = "aarch64")]
fn mul_add(n: f64, b: f64, v: f64) -> f64 {
    n.mul_add(b, v)
}

#[cfg(not(target_arch = "aarch64"))]
fn mul_add(n: f64, b: f64, v: f64) -> f64 {
    n * b + v
}

fn parse_large_int(mut n: f64, s: &[u8], base: i64, sign: bool) -> Value {
    let b = base as f64;
    for &c in s {
        let v = digit_val(c);
        if v >= base {
            break;
        }
        n = mul_add(n, b, v as f64);
    }
    if sign {
        n = -n;
    }
    Value::Float(n)
}

pub fn parse_int(s: &[u8], mut base: i64) -> Value {
    let mut s = s;
    let mut sign = false;
    if s.is_empty() {
        return Value::Float(f64::NAN);
    }
    match s[0] {
        b'-' => {
            sign = true;
            s = &s[1..];
        }
        b'+' => s = &s[1..],
        _ => {}
    }
    if s.is_empty() {
        return Value::Float(f64::NAN);
    }
    if s[0] == b'0' && s.len() > 1 && (s[1] == b'x' || s[1] == b'X') && (base == 0 || base == 16) {
        base = 16;
        s = &s[2..];
    }
    if s.is_empty() {
        return Value::Float(f64::NAN);
    }
    if base == 0 {
        if s[0] == b'0' && s.len() > 1 && (s[1] == b'x' || s[1] == b'X') {
            if s.len() < 3 {
                return Value::Float(f64::NAN);
            }
            base = 16;
            s = &s[2..];
        } else {
            base = 10;
        }
    } else if !(2..=36).contains(&base) {
        return Value::Float(f64::NAN);
    }
    let cutoff: i64 = match base {
        10 => i64::MAX / 10 + 1,
        16 => i64::MAX / 16 + 1,
        _ => i64::MAX / base + 1,
    };
    let mut n: i64 = 0;
    let mut i = 0;
    while i < s.len() {
        if n >= cutoff {
            return parse_large_int(n as f64, &s[i..], base, sign);
        }
        let v = digit_val(s[i]);
        if v >= base {
            break;
        }
        n = n.wrapping_mul(base);
        let n1 = n.wrapping_add(v);
        if n1 < n {
            return parse_large_int(n as f64 + v as f64, &s[i + 1..], base, sign);
        }
        n = n1;
        i += 1;
    }
    if i == 0 {
        return Value::Float(f64::NAN);
    }
    if sign {
        n = n.wrapping_neg();
    }
    int_to_value(n)
}

impl Runtime {
    pub fn builtin_is_nan(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(Value::Bool(call.argument(0).to_float(self)?.is_nan()))
    }

    pub fn builtin_parse_int(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let str_ = call.argument(0).to_string(self)?.to_trimmed_utf8();
        let radix = crate::runtime::to_int32(self, &call.argument(1))?;
        Ok(parse_int(&str_, i64::from(radix)))
    }

    pub fn builtin_parse_float(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let t = call.argument(0).to_string(self)?.to_trimmed_utf8();
        if let Some(m) = parse_float_regexp().find_string_submatch(&t)
            && m.len() == 2
        {
            let s = m[1];
            if !s.is_empty() && s != b"+" && s != b"-" {
                match s {
                    b"Infinity" | b"+Infinity" => return Ok(Value::Float(f64::INFINITY)),
                    b"-Infinity" => return Ok(Value::Float(f64::NEG_INFINITY)),
                    _ => match gocompat::strconv::parse_float(s, 64) {
                        Ok(f) => return Ok(float_to_value(f)),
                        Err(e) if e.kind == gocompat::strconv::ErrorKind::Range => {
                            return Ok(float_to_value(e.value));
                        }
                        Err(_) => {}
                    },
                }
            }
        }
        Ok(Value::Float(f64::NAN))
    }

    pub fn builtin_is_finite(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let f = call.argument(0).to_float(self)?;
        Ok(Value::Bool(!(f.is_nan() || f.is_infinite())))
    }

    fn malformed_uri(&mut self) -> Throw {
        let e = self.get_uri_error();
        Throw::Value(self.new_error(e, "Malformed URI"))
    }

    pub fn _encode(
        &mut self,
        uri_string: &JsString,
        unescaped: &[bool; 256],
    ) -> JsResult<JsString> {
        let mut reader = uri_string.reader();
        let mut needed = false;
        let mut l = 0usize;
        while let Some((rn, _)) = reader.read_rune() {
            if reader.last_rune_invalid() {
                return Err(self.malformed_uri());
            }
            if rn >= 0x80 {
                needed = true;
                let mut b = Vec::new();
                utf8::append_rune(&mut b, rn);
                l += b.len() * 3;
            } else if !unescaped[rn as usize] {
                needed = true;
                l += 3;
            } else {
                l += 1;
            }
        }
        if !needed {
            return Ok(uri_string.clone());
        }
        let mut buf = Vec::with_capacity(l);
        let mut reader = uri_string.reader();
        while let Some((rn, _)) = reader.read_rune() {
            if rn >= 0x80 {
                let mut b = Vec::new();
                utf8::append_rune(&mut b, rn);
                for c in b {
                    buf.push(b'%');
                    buf.push(HEX_UPPER[(c >> 4) as usize]);
                    buf.push(HEX_UPPER[(c & 15) as usize]);
                }
            } else if !unescaped[rn as usize] {
                buf.push(b'%');
                buf.push(HEX_UPPER[(rn >> 4) as usize]);
                buf.push(HEX_UPPER[(rn & 15) as usize]);
            } else {
                buf.push(rn as u8);
            }
        }
        Ok(JsString::Ascii(AsciiString::new(&buf)))
    }

    pub fn _decode(&mut self, sv: &JsString, reserved_set: &[bool; 256]) -> JsResult<JsString> {
        let s = sv.go_string();
        let mut hex_count = 0;
        let mut i = 0;
        while i < s.len() {
            if s[i] == b'%' {
                if i + 2 >= s.len() || !ishex(s[i + 1]) || !ishex(s[i + 2]) {
                    return Err(self.malformed_uri());
                }
                let c = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                if !reserved_set[c as usize] {
                    hex_count += 1;
                }
                i += 3;
            } else {
                i += 1;
            }
        }
        if hex_count == 0 {
            return Ok(sv.clone());
        }
        let mut t = Vec::with_capacity(s.len() - hex_count * 2);
        let mut is_unicode = false;
        let mut i = 0;
        while i < s.len() {
            let ch = s[i];
            if ch == b'%' {
                let c = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                if reserved_set[c as usize] {
                    t.extend_from_slice(&s[i..i + 3]);
                } else {
                    t.push(c);
                    if c >= 0x80 {
                        is_unicode = true;
                    }
                }
                i += 3;
            } else {
                if ch >= 0x80 {
                    is_unicode = true;
                }
                t.push(ch);
                i += 1;
            }
        }
        if !is_unicode {
            return Ok(JsString::Ascii(AsciiString::new(&t)));
        }
        let mut us = Vec::with_capacity(s.len());
        let mut t = &t[..];
        while !t.is_empty() {
            let (rn, size) = utf8::decode_rune(t);
            if rn == 0xFFFD && (size != 3 || t[0] != 0xef || t[1] != 0xbf || t[2] != 0xbd) {
                return Err(self.malformed_uri());
            }
            us.push(rn);
            t = &t[size..];
        }
        Ok(JsString::Unicode(unicode_string_from_runes(&us)))
    }

    pub fn builtin_decode_uri(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let uri_string = call.argument(0).to_string(self)?;
        Ok(Value::String(
            self._decode(&uri_string, &uri_tables().uri_reserved_hash)?,
        ))
    }

    pub fn builtin_decode_uri_component(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let uri_string = call.argument(0).to_string(self)?;
        Ok(Value::String(
            self._decode(&uri_string, &uri_tables().empty_escape_set)?,
        ))
    }

    pub fn builtin_encode_uri(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let uri_string = call.argument(0).to_string(self)?;
        Ok(Value::String(self._encode(
            &uri_string,
            &uri_tables().uri_reserved_unescaped_hash,
        )?))
    }

    pub fn builtin_encode_uri_component(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let uri_string = call.argument(0).to_string(self)?;
        Ok(Value::String(
            self._encode(&uri_string, &uri_tables().uri_unescaped)?,
        ))
    }

    pub fn builtin_escape(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let s = call.argument(0).to_string(self)?;
        let mut sb = Vec::new();
        let l = s.length();
        for i in 0..l {
            let r = s.char_at(i);
            if (r < 0x80 && (r as u8).is_ascii_alphanumeric())
                || r == u16::from(b'@')
                || r == u16::from(b'*')
                || r == u16::from(b'_')
                || r == u16::from(b'+')
                || r == u16::from(b'-')
                || r == u16::from(b'.')
                || r == u16::from(b'/')
            {
                sb.push(r as u8);
            } else if r <= 0xff {
                sb.push(b'%');
                sb.push(HEX_UPPER[(r >> 4) as usize]);
                sb.push(HEX_UPPER[(r & 0xf) as usize]);
            } else {
                sb.extend_from_slice(b"%u");
                sb.push(HEX_UPPER[(r >> 12) as usize]);
                sb.push(HEX_UPPER[((r >> 8) & 0xf) as usize]);
                sb.push(HEX_UPPER[((r >> 4) & 0xf) as usize]);
                sb.push(HEX_UPPER[(r & 0xf) as usize]);
            }
        }
        Ok(Value::String(JsString::Ascii(AsciiString::new(&sb))))
    }

    pub fn builtin_unescape(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let s = call.argument(0).to_string(self)?;
        let l = s.length();
        let mut ascii_buf: Vec<u8> = Vec::new();
        let mut unicode_buf: Vec<u16> = Vec::new();
        let (_, u) = devirtualize_string(&s);
        let mut unicode = u.is_some();
        if unicode {
            unicode_buf.reserve(l + 1);
            unicode_buf.push(unistring::BOM);
        } else {
            ascii_buf.reserve(l);
        }
        let hexc = |c: u16| c <= 0xff && ishex(c as u8);
        let mut i = 0;
        while i < l {
            let mut r = s.char_at(i);
            'out: {
                if r == u16::from(b'%') {
                    if i + 6 <= l && s.char_at(i + 1) == u16::from(b'u') {
                        let c0 = s.char_at(i + 2);
                        let c1 = s.char_at(i + 3);
                        let c2 = s.char_at(i + 4);
                        let c3 = s.char_at(i + 5);
                        if hexc(c0) && hexc(c1) && hexc(c2) && hexc(c3) {
                            r = (u16::from(unhex(c0 as u8)) << 12)
                                | (u16::from(unhex(c1 as u8)) << 8)
                                | (u16::from(unhex(c2 as u8)) << 4)
                                | u16::from(unhex(c3 as u8));
                            i += 5;
                            break 'out;
                        }
                    }
                    if i + 3 <= l {
                        let c0 = s.char_at(i + 1);
                        let c1 = s.char_at(i + 2);
                        if hexc(c0) && hexc(c1) {
                            r = u16::from((unhex(c0 as u8) << 4) | unhex(c1 as u8));
                            i += 2;
                        }
                    }
                }
            }
            if r >= 0x80 && !unicode {
                unicode_buf = Vec::with_capacity(l + 1);
                unicode_buf.push(unistring::BOM);
                for &b in &ascii_buf {
                    unicode_buf.push(u16::from(b));
                }
                ascii_buf = Vec::new();
                unicode = true;
            }
            if unicode {
                unicode_buf.push(r);
            } else {
                ascii_buf.push(r as u8);
            }
            i += 1;
        }
        if unicode {
            return Ok(Value::String(JsString::Unicode(UnicodeString::from_raw(
                unicode_buf,
            ))));
        }
        Ok(Value::String(JsString::Ascii(AsciiString::new(&ascii_buf))))
    }
}

impl Runtime {
    pub fn get_eval(&mut self) -> ObjRef {
        if let Some(r) = self.global.eval {
            return r;
        }
        let ret = self.new_native_func(NativeFn::Fn(|r, c| r.builtin_eval(c)), "eval", 1);
        self.global.eval = Some(ret);
        ret
    }
}

fn prop(o: ObjRef) -> Value {
    value_prop(Value::Object(o), true, false, true)
}

fn create_global_object_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t.put_str("Object", |r| prop(r.get_object()));
    t.put_str("Function", |r| prop(r.get_function()));
    t.put_str("Array", |r| prop(r.get_array()));
    t.put_str("String", |r| prop(r.get_string()));
    t.put_str("Number", |r| prop(r.get_number()));
    t.put_str("BigInt", |r| prop(r.get_big_int()));
    t.put_str("RegExp", |r| prop(r.get_reg_exp()));
    t.put_str("Date", |r| prop(r.get_date()));
    t.put_str("Boolean", |r| prop(r.get_boolean()));
    t.put_str("Proxy", |r| prop(r.get_proxy()));
    t.put_str("Reflect", |r| prop(r.get_reflect()));
    t.put_str("Error", |r| prop(r.get_error()));
    t.put_str("AggregateError", |r| prop(r.get_aggregate_error()));
    t.put_str("TypeError", |r| prop(r.get_type_error()));
    t.put_str("ReferenceError", |r| prop(r.get_reference_error()));
    t.put_str("SyntaxError", |r| prop(r.get_syntax_error()));
    t.put_str("RangeError", |r| prop(r.get_range_error()));
    t.put_str("EvalError", |r| prop(r.get_eval_error()));
    t.put_str("URIError", |r| prop(r.get_uri_error()));
    t.put_str("eval", |r| prop(r.get_eval()));
    t.put_str("Math", |r| prop(r.get_math()));
    t.put_str("JSON", |r| prop(r.get_json()));
    crate::builtin_typedarrays::add_typed_arrays(&mut t);
    t.put_str("Symbol", |r| prop(r.get_symbol()));
    t.put_str("WeakSet", |r| prop(r.get_weak_set()));
    t.put_str("WeakMap", |r| prop(r.get_weak_map()));
    t.put_str("Map", |r| prop(r.get_map()));
    t.put_str("Set", |r| prop(r.get_set()));
    t.put_str("Promise", |r| prop(r.get_promise()));
    t.put_str("globalThis", |r| prop(r.global_object));
    t.put_str("NaN", |_| {
        value_prop(Value::Float(f64::NAN), false, false, false)
    });
    t.put_str("undefined", |_| {
        value_prop(Value::Undefined, false, false, false)
    });
    t.put_str("Infinity", |_| {
        value_prop(Value::Float(f64::INFINITY), false, false, false)
    });
    t.put_str("isNaN", |r| {
        r.method_prop(|r, c| r.builtin_is_nan(c), "isNaN", 1)
    });
    t.put_str("parseInt", |r| prop(r.get_parse_int()));
    t.put_str("parseFloat", |r| prop(r.get_parse_float()));
    t.put_str("isFinite", |r| {
        r.method_prop(|r, c| r.builtin_is_finite(c), "isFinite", 1)
    });
    t.put_str("decodeURI", |r| {
        r.method_prop(|r, c| r.builtin_decode_uri(c), "decodeURI", 1)
    });
    t.put_str("decodeURIComponent", |r| {
        r.method_prop(
            |r, c| r.builtin_decode_uri_component(c),
            "decodeURIComponent",
            1,
        )
    });
    t.put_str("encodeURI", |r| {
        r.method_prop(|r, c| r.builtin_encode_uri(c), "encodeURI", 1)
    });
    t.put_str("encodeURIComponent", |r| {
        r.method_prop(
            |r, c| r.builtin_encode_uri_component(c),
            "encodeURIComponent",
            1,
        )
    });
    t.put_str("escape", |r| {
        r.method_prop(|r, c| r.builtin_escape(c), "escape", 1)
    });
    t.put_str("unescape", |r| {
        r.method_prop(|r, c| r.builtin_unescape(c), "unescape", 1)
    });
    t.put_sym(SYM_TO_STRING_TAG, |_| {
        value_prop(
            Value::String(ascii_string(CLASS_GLOBAL)),
            false,
            false,
            true,
        )
    });
    t
}

pub fn get_global_object_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_global_object_template)
}
