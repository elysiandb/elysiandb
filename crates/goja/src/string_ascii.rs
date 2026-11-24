use crate::builtin_bigint::string_to_big_int;
use crate::string::{JsString, devirtualize_string};
use crate::string_unicode::UnicodeString;
use crate::unistring::{self, BOM};
use crate::value::{Value, float_to_value, int_to_value};
use gocompat::strconv::{self, ErrorKind};
use gocompat::strings::trim_space;
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AsciiString(Arc<[u8]>);

pub(crate) enum ParseErr {
    Syntax,
    Range,
}

pub(crate) fn string_to_int(ss: &[u8]) -> Result<i64, ParseErr> {
    if ss.is_empty() {
        return Ok(0);
    }
    if ss == b"-0" {
        return Err(ParseErr::Syntax);
    }
    let parse = |s: &[u8], base: u32| match strconv::parse_int(s, base, 64) {
        Ok(v) => Ok(v),
        Err(e) => match e.kind {
            ErrorKind::Range => Err(ParseErr::Range),
            _ => Err(ParseErr::Syntax),
        },
    };
    if ss.len() > 2 {
        match &ss[..2] {
            b"0x" | b"0X" => return parse(&ss[2..], 16),
            b"0b" | b"0B" => return parse(&ss[2..], 2),
            b"0o" | b"0O" => return parse(&ss[2..], 8),
            _ => {}
        }
    }
    parse(ss, 10)
}

fn to_float_trimmed(trimmed: &[u8]) -> Result<f64, ParseErr> {
    if trimmed.is_empty() {
        return Ok(0.0);
    }
    if trimmed == b"-0" {
        return Ok(-0.0);
    }
    if trimmed.contains(&b'_') {
        return Err(ParseErr::Syntax);
    }
    if trimmed.len() >= 2 {
        let prefix = if trimmed[0] == b'-' || trimmed[0] == b'+' {
            &trimmed[1..]
        } else {
            trimmed
        };
        if prefix.len() >= 2 && prefix[0] == b'0' && (prefix[1] == b'x' || prefix[1] == b'X') {
            return Err(ParseErr::Syntax);
        }
    }
    match strconv::parse_float(trimmed, 64) {
        Ok(f) => {
            if f.is_infinite() {
                let ss = trimmed.to_ascii_lowercase();
                if ss.starts_with(b"inf") || ss.starts_with(b"-inf") || ss.starts_with(b"+inf") {
                    return Err(ParseErr::Syntax);
                }
            }
            Ok(f)
        }
        Err(e) => match e.kind {
            ErrorKind::Range => Ok(e.value),
            _ => Err(ParseErr::Syntax),
        },
    }
}

impl AsciiString {
    pub fn new(s: &[u8]) -> AsciiString {
        AsciiString(Arc::from(s))
    }

    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn string(&self) -> unistring::String {
        unistring::String::from_bytes(&self.0)
    }

    pub fn go_string(&self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn to_boolean(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn to_float(&self) -> f64 {
        let ss = trim_space(&self.0);
        if ss.is_empty() {
            return 0.0;
        }
        if ss == b"Infinity" || ss == b"+Infinity" {
            return f64::INFINITY;
        }
        if ss == b"-Infinity" {
            return f64::NEG_INFINITY;
        }
        match to_float_trimmed(ss) {
            Ok(f) => f,
            Err(_) => match string_to_int(ss) {
                Ok(i) => i as f64,
                Err(_) => f64::NAN,
            },
        }
    }

    pub fn to_number(&self) -> Value {
        let ss = trim_space(&self.0);
        if ss.is_empty() {
            return int_to_value(0);
        }
        if ss == b"Infinity" || ss == b"+Infinity" {
            return Value::Float(f64::INFINITY);
        }
        if ss == b"-Infinity" {
            return Value::Float(f64::NEG_INFINITY);
        }
        if let Ok(i) = string_to_int(ss) {
            return int_to_value(i);
        }
        if let Ok(f) = to_float_trimmed(ss) {
            return float_to_value(f);
        }
        Value::Float(f64::NAN)
    }

    pub fn to_trimmed_utf8(&self) -> Vec<u8> {
        trim_space(&self.0).to_vec()
    }

    pub fn strict_equals(&self, other: &Value) -> bool {
        match other {
            Value::String(JsString::Ascii(o)) => self.0 == o.0,
            Value::String(JsString::Imported(o)) => {
                o.unicode().is_none() && &self.0[..] == o.utf8()
            }
            _ => false,
        }
    }

    pub fn equals(&self, other: &Value) -> bool {
        if self.strict_equals(other) {
            return true;
        }
        match other {
            Value::Int(o) => match string_to_int(trim_space(&self.0)) {
                Ok(o1) => o1 == *o,
                Err(_) => false,
            },
            Value::Float(o) => self.to_float() == *o,
            Value::Bool(_) => match to_float_trimmed(trim_space(&self.0)) {
                Ok(o1) => o1 == other.prim_to_float(),
                Err(_) => false,
            },
            Value::BigInt(o) => match string_to_big_int(&self.to_trimmed_utf8()) {
                Some(b) => b.cmp(o) == 0,
                None => false,
            },
            _ => false,
        }
    }

    pub fn concat(&self, other: &JsString) -> JsString {
        let (a, u) = devirtualize_string(other);
        if let Some(u) = u {
            let mut b = Vec::with_capacity(self.0.len() + u.raw().len());
            b.push(BOM);
            b.extend(self.0.iter().map(|&c| c as u16));
            b.extend_from_slice(&u.raw()[1..]);
            return JsString::Unicode(UnicodeString::from_raw(b));
        }
        let a = a.unwrap_or_else(|| AsciiString::new(b""));
        let joined: Arc<[u8]> = self.0.iter().chain(a.0.iter()).copied().collect();
        JsString::Ascii(AsciiString(joined))
    }

    pub fn compare_to(&self, other: &JsString) -> i32 {
        let o = match other {
            JsString::Ascii(o) => o.0.to_vec(),
            JsString::Unicode(o) => o.go_string(),
            JsString::Imported(o) => o.utf8().to_vec(),
        };
        match self.0[..].cmp(&o[..]) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }

    pub fn to_integer(&self) -> i64 {
        let ss = trim_space(&self.0);
        if ss.is_empty() {
            return 0;
        }
        if ss == b"Infinity" || ss == b"+Infinity" {
            return i64::MAX;
        }
        if ss == b"-Infinity" {
            return i64::MIN;
        }
        match string_to_int(ss) {
            Ok(i) => i,
            Err(e) => match to_float_trimmed(ss) {
                Ok(f) => crate::value::go_float_to_int(f),
                Err(_) => match e {
                    ParseErr::Range => {
                        if ss[0] == b'-' {
                            i64::MIN
                        } else {
                            i64::MAX
                        }
                    }
                    ParseErr::Syntax => 0,
                },
            },
        }
    }

    pub fn char_at(&self, idx: usize) -> u16 {
        u16::from(self.0[idx])
    }

    pub fn length(&self) -> usize {
        self.0.len()
    }

    pub fn substring(&self, start: usize, end: usize) -> JsString {
        JsString::Ascii(AsciiString::new(&self.0[start..end]))
    }

    pub fn reader(&self) -> Box<dyn crate::string::RuneReader> {
        Box::new(AsciiRuneReader {
            s: self.clone(),
            pos: 0,
        })
    }

    pub fn utf16_reader(&self) -> Box<dyn crate::string::Utf16Reader> {
        Box::new(AsciiUtf16Reader {
            s: self.clone(),
            pos: 0,
        })
    }

    pub fn utf16_rune_reader(&self) -> Box<dyn crate::string::RuneReader> {
        Box::new(AsciiUtf16Reader {
            s: self.clone(),
            pos: 0,
        })
    }

    pub fn utf16_runes(&self) -> Vec<i32> {
        self.0.iter().map(|&c| i32::from(c)).collect()
    }

    pub fn index(&self, substr: &JsString, start: isize) -> isize {
        let (a, u) = devirtualize_string(substr);
        if u.is_none() {
            let a = a.unwrap_or_else(|| AsciiString::new(b""));
            if start > self.0.len() as isize {
                return -1;
            }
            if let Some(p) = bytes_index(&self.0[start as usize..], &a.0) {
                return p as isize + start;
            }
        }
        -1
    }

    pub fn last_index(&self, substr: &JsString, pos: isize) -> isize {
        let (a, u) = devirtualize_string(substr);
        if u.is_none() {
            let a = a.unwrap_or_else(|| AsciiString::new(b""));
            let end = pos + a.0.len() as isize;
            let ss = if end > self.0.len() as isize {
                &self.0[..]
            } else {
                &self.0[..end as usize]
            };
            return match bytes_last_index(ss, &a.0) {
                Some(p) => p as isize,
                None => -1,
            };
        }
        -1
    }

    pub fn from_arc(a: Arc<[u8]>) -> AsciiString {
        AsciiString(a)
    }

    pub fn to_lower(&self) -> JsString {
        JsString::Ascii(AsciiString::new(&self.0.to_ascii_lowercase()))
    }

    pub fn to_upper(&self) -> JsString {
        JsString::Ascii(AsciiString::new(&self.0.to_ascii_uppercase()))
    }
}

pub(crate) fn bytes_index(s: &[u8], sub: &[u8]) -> Option<usize> {
    if sub.is_empty() {
        return Some(0);
    }
    if sub.len() > s.len() {
        return None;
    }
    let first = sub[0];
    let last = s.len() - sub.len();
    let mut i = 0;
    while i <= last {
        match s[i..=last].iter().position(|&c| c == first) {
            None => return None,
            Some(p) => {
                i += p;
                if &s[i..i + sub.len()] == sub {
                    return Some(i);
                }
                i += 1;
            }
        }
    }
    None
}

pub(crate) fn bytes_last_index(s: &[u8], sub: &[u8]) -> Option<usize> {
    if sub.is_empty() {
        return Some(s.len());
    }
    if sub.len() > s.len() {
        return None;
    }
    let mut i = s.len() - sub.len();
    loop {
        if &s[i..i + sub.len()] == sub {
            return Some(i);
        }
        if i == 0 {
            return None;
        }
        i -= 1;
    }
}

struct AsciiRuneReader {
    s: AsciiString,
    pos: usize,
}

impl crate::string::RuneReader for AsciiRuneReader {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos < self.s.0.len() {
            let r = i32::from(self.s.0[self.pos]);
            self.pos += 1;
            return Some((r, 1));
        }
        None
    }
}

struct AsciiUtf16Reader {
    s: AsciiString,
    pos: usize,
}

impl crate::string::Utf16Reader for AsciiUtf16Reader {
    fn read_char(&mut self) -> Option<u16> {
        if self.pos < self.s.0.len() {
            let c = u16::from(self.s.0[self.pos]);
            self.pos += 1;
            return Some(c);
        }
        None
    }
}

impl crate::string::RuneReader for AsciiUtf16Reader {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos < self.s.0.len() {
            let r = i32::from(self.s.0[self.pos]);
            self.pos += 1;
            return Some((r, 1));
        }
        None
    }
}
