use crate::string::{JsString, RuneReader, Utf16Reader};
use crate::string_ascii::AsciiString;
use crate::string_unicode::{UnicodeString, trim_whitespace_chars};
use crate::unistring;
use crate::value::Value;
use gocompat::utf8;
use gocompat::utf16;
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
struct ImportedInner {
    s: Arc<[u8]>,
    u: OnceLock<Option<UnicodeString>>,
}

#[derive(Clone, Debug)]
pub struct ImportedString(Arc<ImportedInner>);

impl ImportedString {
    pub fn new(s: &[u8]) -> ImportedString {
        ImportedString(Arc::new(ImportedInner {
            s: Arc::from(s),
            u: OnceLock::new(),
        }))
    }

    pub fn utf8(&self) -> &[u8] {
        &self.0.s
    }

    pub fn is_scanned(&self) -> bool {
        self.0.u.get().is_some()
    }

    pub fn unicode(&self) -> Option<&UnicodeString> {
        self.0
            .u
            .get_or_init(|| unistring::scan(&self.0.s).map(UnicodeString::from_raw))
            .as_ref()
    }

    pub fn ascii(&self) -> AsciiString {
        AsciiString::from_arc(self.0.s.clone())
    }

    pub fn string(&self) -> unistring::String {
        match self.unicode() {
            Some(u) => unistring::from_utf16(u.raw()),
            None => unistring::String::from_bytes(&self.0.s),
        }
    }

    pub fn go_string(&self) -> Vec<u8> {
        self.0.s.to_vec()
    }

    pub fn to_float(&self) -> f64 {
        match self.unicode() {
            Some(_) => f64::NAN,
            None => self.ascii().to_float(),
        }
    }

    pub fn to_number(&self) -> Value {
        match self.unicode() {
            Some(u) => u.to_number(),
            None => self.ascii().to_number(),
        }
    }

    pub fn to_trimmed_utf8(&self) -> Vec<u8> {
        trim_whitespace_chars(&self.0.s).to_vec()
    }

    pub fn strict_equals(&self, other: &Value) -> bool {
        match other {
            Value::String(JsString::Ascii(o)) => {
                if self.0.u.get().is_some_and(|u| u.is_some()) {
                    return false;
                }
                &self.0.s[..] == o.as_bytes()
            }
            Value::String(JsString::Unicode(o)) => match self.unicode() {
                Some(u) => u.raw() == o.raw(),
                None => false,
            },
            Value::String(JsString::Imported(o)) => self.0.s == o.0.s,
            _ => false,
        }
    }

    pub fn to_integer(&self) -> i64 {
        match self.unicode() {
            Some(_) => 0,
            None => self.ascii().to_integer(),
        }
    }

    pub fn equals(&self, other: &Value) -> bool {
        match self.unicode() {
            Some(u) => u.equals(other),
            None => self.ascii().equals(other),
        }
    }

    pub fn concat(&self, other: &JsString) -> JsString {
        if !self.is_scanned() {
            if let JsString::Imported(v) = other
                && !v.is_scanned()
            {
                let mut b = Vec::with_capacity(self.0.s.len() + v.0.s.len());
                b.extend_from_slice(&self.0.s);
                b.extend_from_slice(&v.0.s);
                return JsString::Imported(ImportedString::new(&b));
            }
            self.unicode();
        }
        match self.unicode() {
            Some(u) => u.concat(other),
            None => self.ascii().concat(other),
        }
    }

    pub fn compare_to(&self, other: &JsString) -> i32 {
        match self.utf8().cmp(&other.go_string()[..]) {
            std::cmp::Ordering::Less => -1,
            std::cmp::Ordering::Equal => 0,
            std::cmp::Ordering::Greater => 1,
        }
    }

    pub fn char_at(&self, idx: usize) -> u16 {
        match self.unicode() {
            Some(u) => u.char_at(idx),
            None => u16::from(self.0.s[idx]),
        }
    }

    pub fn length(&self) -> usize {
        match self.unicode() {
            Some(u) => u.length(),
            None => self.0.s.len(),
        }
    }

    pub fn substring(&self, start: usize, end: usize) -> JsString {
        match self.unicode() {
            Some(u) => u.substring(start, end),
            None => {
                if start == 0 && end == self.0.s.len() {
                    return JsString::Ascii(self.ascii());
                }
                JsString::Ascii(AsciiString::new(&self.0.s[start..end]))
            }
        }
    }

    pub fn reader(&self) -> Box<dyn RuneReader> {
        if let Some(u) = self.0.u.get() {
            return match u {
                Some(u) => u.reader(),
                None => self.ascii().reader(),
            };
        }
        Box::new(Utf8RuneReader {
            s: self.0.s.clone(),
            pos: 0,
        })
    }

    pub fn utf16_reader(&self) -> Box<dyn Utf16Reader> {
        if let Some(u) = self.0.u.get() {
            return match u {
                Some(u) => u.utf16_reader(),
                None => self.ascii().utf16_reader(),
            };
        }
        Box::new(StringUtf16Reader {
            s: self.0.s.clone(),
            pos: 0,
            second: 0,
        })
    }

    pub fn utf16_rune_reader(&self) -> Box<dyn RuneReader> {
        if let Some(u) = self.0.u.get() {
            return match u {
                Some(u) => u.utf16_rune_reader(),
                None => self.ascii().utf16_rune_reader(),
            };
        }
        Box::new(StringUtf16Reader {
            s: self.0.s.clone(),
            pos: 0,
            second: 0,
        })
    }

    pub fn utf16_runes(&self) -> Vec<i32> {
        match self.unicode() {
            Some(u) => u.utf16_runes(),
            None => self.ascii().utf16_runes(),
        }
    }

    pub fn index(&self, v: &JsString, start: isize) -> isize {
        match self.unicode() {
            Some(u) => u.index(v, start),
            None => self.ascii().index(v, start),
        }
    }

    pub fn last_index(&self, v: &JsString, pos: isize) -> isize {
        match self.unicode() {
            Some(u) => u.last_index(v, pos),
            None => self.ascii().last_index(v, pos),
        }
    }

    pub fn to_lower(&self) -> JsString {
        match self.unicode() {
            Some(_) => crate::string_unicode::to_lower(&self.0.s),
            None => self.ascii().to_lower(),
        }
    }

    pub fn to_upper(&self) -> JsString {
        match self.unicode() {
            Some(_) => {
                let caser =
                    crate::deps::xtext::cases::upper(crate::deps::xtext::language::UND, &[]);
                crate::string::new_string_value(&caser.string(&self.0.s))
            }
            None => self.ascii().to_upper(),
        }
    }
}

struct Utf8RuneReader {
    s: Arc<[u8]>,
    pos: usize,
}

impl RuneReader for Utf8RuneReader {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        if self.pos >= self.s.len() {
            return None;
        }
        let c = self.s[self.pos];
        if c < 0x80 {
            self.pos += 1;
            return Some((i32::from(c), 1));
        }
        let (r, size) = utf8::decode_rune(&self.s[self.pos..]);
        self.pos += size;
        Some((r, size))
    }
}

struct StringUtf16Reader {
    s: Arc<[u8]>,
    pos: usize,
    second: u16,
}

impl Utf16Reader for StringUtf16Reader {
    fn read_char(&mut self) -> Option<u16> {
        if self.second != 0 {
            let c = self.second;
            self.second = 0;
            return Some(c);
        }
        if self.pos < self.s.len() {
            let (r1, size1) = utf8::decode_rune(&self.s[self.pos..]);
            self.pos += size1;
            if r1 <= 0xFFFF {
                return Some(r1 as u16);
            }
            let (first, second) = utf16::encode_rune(r1);
            self.second = second as u16;
            return Some(first as u16);
        }
        None
    }
}

impl RuneReader for StringUtf16Reader {
    fn read_rune(&mut self) -> Option<(i32, usize)> {
        let c = self.read_char()?;
        Some((i32::from(c), 1))
    }
}
