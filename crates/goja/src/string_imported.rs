use crate::string::JsString;
use crate::string_ascii::AsciiString;
use crate::string_unicode::UnicodeString;
use crate::unistring;
use crate::value::Value;
use std::sync::{Arc, OnceLock};

#[derive(Debug)]
struct ImportedInner {
    s: Vec<u8>,
    u: OnceLock<Option<UnicodeString>>,
}

#[derive(Clone, Debug)]
pub struct ImportedString(Arc<ImportedInner>);

impl ImportedString {
    pub fn new(s: &[u8]) -> ImportedString {
        ImportedString(Arc::new(ImportedInner {
            s: s.to_vec(),
            u: OnceLock::new(),
        }))
    }

    pub fn utf8(&self) -> &[u8] {
        &self.0.s
    }

    pub fn unicode(&self) -> Option<&UnicodeString> {
        self.0
            .u
            .get_or_init(|| unistring::scan(&self.0.s).map(UnicodeString::from_raw))
            .as_ref()
    }

    pub fn string(&self) -> unistring::String {
        match self.unicode() {
            Some(u) => u.string(),
            None => unistring::String::from_bytes(&self.0.s),
        }
    }

    pub fn go_string(&self) -> Vec<u8> {
        self.0.s.clone()
    }

    pub fn to_float(&self) -> f64 {
        match self.unicode() {
            Some(_) => f64::NAN,
            None => AsciiString::new(&self.0.s).to_float(),
        }
    }

    pub fn to_number(&self) -> Value {
        match self.unicode() {
            Some(u) => u.to_number(),
            None => AsciiString::new(&self.0.s).to_number(),
        }
    }

    pub fn to_trimmed_utf8(&self) -> Vec<u8> {
        match self.unicode() {
            Some(u) => u.to_trimmed_utf8(),
            None => AsciiString::new(&self.0.s).to_trimmed_utf8(),
        }
    }

    pub fn strict_equals(&self, other: &Value) -> bool {
        match self.unicode() {
            Some(u) => u.strict_equals(other),
            None => AsciiString::new(&self.0.s).strict_equals(other),
        }
    }

    pub fn to_integer(&self) -> i64 {
        todo!("importedString.ToInteger")
    }

    pub fn equals(&self, other: &Value) -> bool {
        match self.unicode() {
            Some(u) => u.equals(other),
            None => AsciiString::new(&self.0.s).equals(other),
        }
    }

    pub fn concat(&self, other: &JsString) -> JsString {
        match self.unicode() {
            Some(u) => u.concat(other),
            None => AsciiString::new(&self.0.s).concat(other),
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
            None => AsciiString::new(&self.0.s).substring(start, end),
        }
    }

    pub fn reader(&self) -> Box<dyn crate::string::RuneReader> {
        todo!("importedString.Reader")
    }

    pub fn utf16_reader(&self) -> Box<dyn crate::string::Utf16Reader> {
        todo!("importedString.utf16Reader")
    }

    pub fn utf16_rune_reader(&self) -> Box<dyn crate::string::RuneReader> {
        todo!("importedString.utf16RuneReader")
    }

    pub fn utf16_runes(&self) -> Vec<i32> {
        todo!("importedString.utf16Runes")
    }

    pub fn index(&self, _substr: &JsString, _start: isize) -> isize {
        todo!("importedString.index")
    }

    pub fn last_index(&self, _substr: &JsString, _pos: isize) -> isize {
        todo!("importedString.lastIndex")
    }

    pub fn to_lower(&self) -> JsString {
        todo!("importedString.toLower")
    }

    pub fn to_upper(&self) -> JsString {
        todo!("importedString.toUpper")
    }
}
