use crate::deps::xtext::cases;
use crate::deps::xtext::language::UND;
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
        0
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
        Box::new(UnicodeRuneReader {
            s: self.clone(),
            pos: 1,
            invalid: false,
        })
    }

    pub fn utf16_reader(&self) -> Box<dyn Utf16Reader> {
        Box::new(Utf16RuneReaderImpl {
            s: self.clone(),
            pos: 1,
        })
    }

    pub fn utf16_rune_reader(&self) -> Box<dyn RuneReader> {
        Box::new(Utf16RuneReaderImpl {
            s: self.clone(),
            pos: 1,
        })
    }

    pub fn utf16_runes(&self) -> Vec<i32> {
        self.0[1..].iter().map(|&c| i32::from(c)).collect()
    }

    fn units_of(substr: &JsString) -> Vec<u16> {
        let (a, u) = devirtualize_string(substr);
        match u {
            Some(u) => u.0[1..].to_vec(),
            None => a
                .map(|a| a.as_bytes().iter().map(|&c| u16::from(c)).collect())
                .unwrap_or_default(),
        }
    }

    pub fn index(&self, substr: &JsString, start: isize) -> isize {
        let ss = UnicodeString::units_of(substr);
        let s1 = &self.0[1..];
        let end = s1.len() as isize - ss.len() as isize;
        let mut start = start;
        while start <= end {
            let st = start as usize;
            if s1[st..st + ss.len()] == ss[..] {
                return start;
            }
            start += 1;
        }
        -1
    }

    pub fn last_index(&self, substr: &JsString, start: isize) -> isize {
        let ss = UnicodeString::units_of(substr);
        let s1 = &self.0[1..];
        let max_start = s1.len() as isize - ss.len() as isize;
        let mut start = start.min(max_start);
        while start >= 0 {
            let st = start as usize;
            if s1[st..st + ss.len()] == ss[..] {
                return start;
            }
            start -= 1;
        }
        -1
    }

    pub fn to_lower(&self) -> JsString {
        to_lower(&self.go_string())
    }

    pub fn to_upper(&self) -> JsString {
        let caser = cases::upper(UND, &[]);
        crate::string::new_string_value(&caser.string(&self.go_string()))
    }
}

pub fn unicode_string_from_runes(r: &[i32]) -> UnicodeString {
    match unistring::new_from_runes(r).as_utf16() {
        Some(u) => UnicodeString::from_raw(u.to_vec()),
        None => {
            let mut buf = Vec::with_capacity(r.len() + 1);
            buf.push(unistring::BOM);
            buf.extend(r.iter().map(|&c| c as u16));
            UnicodeString::from_raw(buf)
        }
    }
}

pub fn to_lower(s: &[u8]) -> JsString {
    let caser = cases::lower(UND, &[]);
    let lowered = caser.string(s);
    let mut r = Vec::with_capacity(lowered.len());
    let mut i = 0;
    while i < lowered.len() {
        let (c, size) = utf8::decode_rune(&lowered[i..]);
        r.push(c);
        i += size;
    }
    let mut ascii = true;
    let mut i = 0;
    while i + 1 < r.len() {
        if (i == 0 || r[i - 1] != 0x3b1) && r[i] == 0x345 && r[i + 1] == 0x3c2 {
            i += 1;
            r[i] = 0x3c3;
        }
        if r[i] >= 0x80 {
            ascii = false;
        }
        i += 1;
    }
    if ascii {
        ascii = r[r.len() - 1] < 0x80;
    }
    if ascii {
        let b: Vec<u8> = r.iter().map(|&c| c as u8).collect();
        return JsString::Ascii(AsciiString::new(&b));
    }
    JsString::Unicode(unicode_string_from_runes(&r))
}

struct UnicodeRuneReader {
    s: UnicodeString,
    pos: usize,
    invalid: bool,
}

impl RuneReader for UnicodeRuneReader {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        let s = &self.s.0;
        self.invalid = false;
        if self.pos < s.len() {
            let c = s[self.pos];
            let mut size = 1;
            self.pos += 1;
            if crate::string::is_utf16_first_surrogate(c) {
                if self.pos < s.len() {
                    let second = s[self.pos];
                    if crate::string::is_utf16_second_surrogate(second) {
                        let r = utf16::decode_rune(i32::from(c), i32::from(second));
                        size += 1;
                        self.pos += 1;
                        return Some((r, size));
                    }
                }
                self.invalid = true;
            } else if crate::string::is_utf16_second_surrogate(c) {
                self.invalid = true;
            }
            return Some((i32::from(c), size));
        }
        None
    }

    fn last_rune_invalid(&self) -> bool {
        self.invalid
    }
}

struct Utf16RuneReaderImpl {
    s: UnicodeString,
    pos: usize,
}

impl Utf16Reader for Utf16RuneReaderImpl {
    fn read_char(&mut self) -> Option<u16> {
        if self.pos < self.s.0.len() {
            let c = self.s.0[self.pos];
            self.pos += 1;
            return Some(c);
        }
        None
    }
}

impl RuneReader for Utf16RuneReaderImpl {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos < self.s.0.len() {
            let r = i32::from(self.s.0[self.pos]);
            self.pos += 1;
            return Some((r, 1));
        }
        None
    }
}

struct LenientUtf16Decoder {
    utf16_reader: Box<dyn Utf16Reader>,
    prev: u16,
    prev_set: bool,
}

impl RuneReader for LenientUtf16Decoder {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        let c = if self.prev_set {
            self.prev_set = false;
            self.prev
        } else {
            self.utf16_reader.read_char()?
        };
        let mut size = 1;
        if crate::string::is_utf16_first_surrogate(c) {
            let Some(second) = self.utf16_reader.read_char() else {
                return Some((i32::from(c), size));
            };
            if crate::string::is_utf16_second_surrogate(second) {
                size += 1;
                return Some((utf16::decode_rune(i32::from(c), i32::from(second)), size));
            }
            self.prev = second;
            self.prev_set = true;
        }
        Some((i32::from(c), size))
    }
}

pub fn lenient_utf16_decoder(r: Box<dyn Utf16Reader>) -> Box<dyn RuneReader> {
    Box::new(LenientUtf16Decoder {
        utf16_reader: r,
        prev: 0,
        prev_set: false,
    })
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
