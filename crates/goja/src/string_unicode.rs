use crate::parser::WHITESPACE_CHARS;
use crate::string::{JsString, devirtualize_string};
use crate::string_ascii::AsciiString;
use crate::unistring;
use crate::value::Value;
use gocompat::utf8;
use gocompat::utf16;
use std::cmp::Ordering;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct UnicodeString(Arc<[u16]>);

fn is_whitespace_char(r: u32) -> bool {
    std::str::from_utf8(WHITESPACE_CHARS.as_bytes())
        .map(|s| s.chars().any(|c| c as u32 == r))
        .unwrap_or(false)
}

pub(crate) fn trim_whitespace_chars(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() {
        let (r, size) = utf8::decode_rune(&s[start..]);
        if !is_whitespace_char(r as u32) {
            break;
        }
        start += size;
    }
    let mut end = s.len();
    while end > start {
        let (r, size) = utf8::decode_last_rune(&s[start..end]);
        if !is_whitespace_char(r as u32) {
            break;
        }
        end -= size;
    }
    &s[start..end]
}

impl UnicodeString {
    pub fn from_raw(b: Vec<u16>) -> UnicodeString {
        UnicodeString(Arc::from(b))
    }

    pub fn raw(&self) -> &[u16] {
        &self.0
    }

    pub fn length(&self) -> usize {
        self.0.len() - 1
    }

    pub fn char_at(&self, idx: usize) -> u16 {
        self.0[idx + 1]
    }

    pub fn string(&self) -> unistring::String {
        unistring::from_utf16(&self.0)
    }

    pub fn go_string(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.0.len() * 3);
        for r in utf16::decode(&self.0[1..]) {
            utf8::append_rune(&mut out, r);
        }
        out
    }

    pub fn to_boolean(&self) -> bool {
        !self.0.is_empty()
    }

    pub fn to_float(&self) -> f64 {
        f64::NAN
    }

    pub fn to_trimmed_utf8(&self) -> Vec<u8> {
        if self.0.is_empty() {
            return Vec::new();
        }
        trim_whitespace_chars(&self.go_string()).to_vec()
    }

    pub fn to_number(&self) -> Value {
        AsciiString::new(&self.to_trimmed_utf8()).to_number()
    }

    fn equals_unicode(&self, other: &UnicodeString) -> bool {
        self.0 == other.0
    }

    pub fn strict_equals(&self, other: &Value) -> bool {
        match other {
            Value::String(JsString::Unicode(o)) => self.equals_unicode(o),
            Value::String(JsString::Imported(o)) => match o.unicode() {
                Some(u) => self.equals_unicode(u),
                None => false,
            },
            _ => false,
        }
    }

    pub fn equals(&self, other: &Value) -> bool {
        self.strict_equals(other)
    }

    pub fn concat(&self, other: &JsString) -> JsString {
        let (a, u) = devirtualize_string(other);
        let mut b = Vec::with_capacity(self.0.len() + 8);
        b.extend_from_slice(&self.0);
        if let Some(u) = u {
            b.extend_from_slice(&u.0[1..]);
        } else if let Some(a) = a {
            b.extend(a.as_bytes().iter().map(|&c| c as u16));
        }
        JsString::Unicode(UnicodeString::from_raw(b))
    }

    pub fn compare_to(&self, other: &JsString) -> i32 {
        match self.go_string()[..].cmp(&other.go_string()[..]) {
            Ordering::Less => -1,
            Ordering::Equal => 0,
            Ordering::Greater => 1,
        }
    }
}
