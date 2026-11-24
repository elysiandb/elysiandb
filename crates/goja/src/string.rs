use crate::string_ascii::AsciiString;
use crate::string_imported::ImportedString;
use crate::string_unicode::UnicodeString;
use crate::unistring;
use crate::value::Value;

pub const PROTO: &str = "__proto__";

#[derive(Clone, Debug)]
pub enum JsString {
    Ascii(AsciiString),
    Unicode(UnicodeString),
    Imported(ImportedString),
}

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
        None => JsString::Ascii(AsciiString::new(raw.ascii_bytes().unwrap_or_default())),
    }
}

pub(crate) fn devirtualize_string(s: &JsString) -> (Option<AsciiString>, Option<UnicodeString>) {
    match s {
        JsString::Ascii(a) => (Some(a.clone()), None),
        JsString::Unicode(u) => (None, Some(u.clone())),
        JsString::Imported(i) => match i.unicode() {
            Some(u) => (None, Some(u.clone())),
            None => (Some(AsciiString::new(i.utf8())), None),
        },
    }
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

    pub fn to_trimmed_utf8(&self) -> Vec<u8> {
        match self {
            JsString::Ascii(s) => s.to_trimmed_utf8(),
            JsString::Unicode(s) => s.to_trimmed_utf8(),
            JsString::Imported(s) => s.to_trimmed_utf8(),
        }
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
            JsString::Imported(s) => match s.unicode() {
                Some(u) => u.equals(other),
                None => AsciiString::new(s.utf8()).equals(other),
            },
        }
    }

    pub fn concat(&self, other: &JsString) -> JsString {
        match self {
            JsString::Ascii(s) => s.concat(other),
            JsString::Unicode(s) => s.concat(other),
            JsString::Imported(s) => match s.unicode() {
                Some(u) => u.concat(other),
                None => AsciiString::new(s.utf8()).concat(other),
            },
        }
    }

    pub fn compare_to(&self, other: &JsString) -> i32 {
        match self {
            JsString::Ascii(s) => s.compare_to(other),
            JsString::Unicode(s) => s.compare_to(other),
            JsString::Imported(s) => match s.utf8().cmp(&other.go_string()[..]) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            },
        }
    }
}
