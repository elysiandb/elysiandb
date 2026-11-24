use crate::parser::WHITESPACE_CHARS;
use crate::string::{JsString, RuneReader, Utf16Reader, devirtualize_string};
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

    pub fn to_integer(&self) -> i64 {
        todo!("unicodeString.ToInteger")
    }

    pub fn substring(&self, start: usize, end: usize) -> JsString {
        let ss = &self.0[start + 1..end + 1];
        if ss.iter().all(|&c| c < 0x80) {
            let b: Vec<u8> = ss.iter().map(|&c| c as u8).collect();
            return JsString::Ascii(AsciiString::new(&b));
        }
        let mut buf = Vec::with_capacity(end - start + 1);
        buf.push(unistring::BOM);
        buf.extend_from_slice(ss);
        JsString::Unicode(UnicodeString::from_raw(buf))
    }

    pub fn reader(&self) -> Box<dyn RuneReader> {
        todo!("unicodeString.Reader")
    }

    pub fn utf16_reader(&self) -> Box<dyn Utf16Reader> {
        todo!("unicodeString.utf16Reader")
    }

    pub fn utf16_rune_reader(&self) -> Box<dyn RuneReader> {
        todo!("unicodeString.utf16RuneReader")
    }

    pub fn utf16_runes(&self) -> Vec<i32> {
        self.0[1..].iter().map(|&c| i32::from(c)).collect()
    }

    pub fn index(&self, _substr: &JsString, _start: isize) -> isize {
        todo!("unicodeString.index")
    }

    pub fn last_index(&self, _substr: &JsString, _start: isize) -> isize {
        todo!("unicodeString.lastIndex")
    }

    pub fn to_lower(&self) -> JsString {
        todo!("unicodeString.toLower")
    }

    pub fn to_upper(&self) -> JsString {
        todo!("unicodeString.toUpper")
    }
}

pub fn lenient_utf16_decoder(_r: Box<dyn Utf16Reader>) -> Box<dyn RuneReader> {
    todo!("lenientUtf16Decoder")
}

#[derive(Default)]
pub struct UnicodeStringBuilder {
    pub buf: Vec<u16>,
    pub unicode: bool,
}

impl UnicodeStringBuilder {
    pub fn grow(&mut self, mut n: usize) {
        if self.buf.is_empty() {
            n += 1;
        }
        if self.buf.capacity() - self.buf.len() < n {
            let new_cap = 2 * self.buf.capacity() + n;
            self.buf.reserve_exact(new_cap - self.buf.len());
        }
    }

    pub fn ensure_started(&mut self, initial_size: usize) {
        self.grow(initial_size);
        if self.buf.is_empty() {
            self.buf.push(unistring::BOM);
        }
    }

    pub fn write_string(&mut self, s: &JsString) {
        let (a, u) = devirtualize_string(s);
        if let Some(u) = u {
            self.buf.extend_from_slice(&u.raw()[1..]);
            self.unicode = true;
        } else if let Some(a) = a {
            self.buf.extend(a.as_bytes().iter().map(|&c| u16::from(c)));
        }
    }

    pub fn string(&self) -> JsString {
        if self.unicode {
            return JsString::Unicode(UnicodeString::from_raw(self.buf.clone()));
        }
        if self.buf.len() < 2 {
            return crate::string::STRING_EMPTY.clone();
        }
        let b: Vec<u8> = self.buf[1..].iter().map(|&c| c as u8).collect();
        JsString::Ascii(AsciiString::new(&b))
    }

    pub fn write_rune(&mut self, r: i32) {
        self.ensure_started(2);
        self.write_rune_fast(r);
    }

    pub fn write_rune_fast(&mut self, r: i32) {
        if r <= 0xFFFF {
            self.buf.push(r as u16);
            if !self.unicode && r >= 0x80 {
                self.unicode = true;
            }
        } else {
            let (first, second) = utf16::encode_rune(r);
            self.buf.push(first as u16);
            self.buf.push(second as u16);
            self.unicode = true;
        }
    }

    pub fn write_ascii_string(&mut self, bytes: &[u8]) {
        self.buf.extend(bytes.iter().map(|&c| u16::from(c)));
    }

    pub fn write_unicode_string(&mut self, s: &UnicodeString) {
        self.buf.extend_from_slice(&s.raw()[1..]);
        self.unicode = true;
    }
}

#[derive(Default)]
pub struct StringBuilder {
    pub ascii_builder: Vec<u8>,
    pub unicode_builder: UnicodeStringBuilder,
}

impl StringBuilder {
    pub fn new() -> StringBuilder {
        StringBuilder::default()
    }

    fn ascii(&self) -> bool {
        self.unicode_builder.buf.is_empty()
    }

    pub fn write_string(&mut self, s: &JsString) {
        let (a, u) = devirtualize_string(s);
        if let Some(u) = u {
            self.switch_to_unicode(u.length());
            self.unicode_builder.write_unicode_string(&u);
        } else if let Some(a) = a {
            if self.ascii() {
                self.ascii_builder.extend_from_slice(a.as_bytes());
            } else {
                self.unicode_builder.write_ascii_string(a.as_bytes());
            }
        }
    }

    pub fn write_utf8_string(&mut self, s: &[u8]) {
        let mut first_unicode_idx = 0;
        if self.ascii() {
            match s.iter().position(|&c| c >= 0x80) {
                Some(i) => {
                    self.switch_to_unicode(s.len());
                    self.unicode_builder.write_ascii_string(&s[..i]);
                    first_unicode_idx = i;
                }
                None => {
                    self.ascii_builder.extend_from_slice(s);
                    return;
                }
            }
        }
        let mut i = first_unicode_idx;
        while i < s.len() {
            let (r, size) = utf8::decode_rune(&s[i..]);
            self.unicode_builder.write_rune_fast(r);
            i += size;
        }
    }

    pub fn write_ascii(&mut self, s: &[u8]) {
        if self.ascii() {
            self.ascii_builder.extend_from_slice(s);
        } else {
            self.unicode_builder.write_ascii_string(s);
        }
    }

    pub fn write_rune(&mut self, r: i32) {
        if r < 0x80 {
            if self.ascii() {
                self.ascii_builder.push(r as u8);
            } else {
                self.unicode_builder.write_rune_fast(r);
            }
        } else {
            let extra_len = if r <= 0xFFFF { 1 } else { 2 };
            self.switch_to_unicode(extra_len);
            self.unicode_builder.write_rune_fast(r);
        }
    }

    pub fn string(&self) -> JsString {
        if self.ascii() {
            return JsString::Ascii(AsciiString::new(&self.ascii_builder));
        }
        self.unicode_builder.string()
    }

    pub fn grow(&mut self, n: usize) {
        if self.ascii() {
            self.ascii_builder.reserve(n);
        } else {
            self.unicode_builder.grow(n);
        }
    }

    pub fn likely_unicode(&mut self, extra_len: usize) {
        self.switch_to_unicode(extra_len);
    }

    fn switch_to_unicode(&mut self, extra_len: usize) {
        if self.ascii() {
            let c = self.ascii_builder.capacity();
            let new_cap = (self.ascii_builder.len() + extra_len).max(c);
            self.unicode_builder.ensure_started(new_cap);
            let a = std::mem::take(&mut self.ascii_builder);
            self.unicode_builder.write_ascii_string(&a);
        }
    }

    pub fn write_substring(&mut self, source: &JsString, start: usize, end: usize) {
        let (a, us) = devirtualize_string(source);
        let Some(us) = us else {
            let a = a.unwrap_or_else(|| AsciiString::new(b""));
            if self.ascii() {
                self.ascii_builder
                    .extend_from_slice(&a.as_bytes()[start..end]);
            } else {
                self.unicode_builder
                    .write_ascii_string(&a.as_bytes()[start..end]);
            }
            return;
        };
        if self.ascii() {
            let uc = (start..end).any(|i| us.char_at(i) >= 0x80);
            if uc {
                self.switch_to_unicode(end - start + 1);
            } else {
                self.ascii_builder.reserve(end - start + 1);
                for i in start..end {
                    self.ascii_builder.push(us.char_at(i) as u8);
                }
                return;
            }
        }
        self.unicode_builder
            .buf
            .extend_from_slice(&us.raw()[start + 1..end + 1]);
        self.unicode_builder.unicode = true;
    }
}
