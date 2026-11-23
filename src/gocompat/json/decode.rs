use super::scanner::{SyntaxError, check_valid, is_space};
use crate::gocompat::{utf8, utf16};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Number(String);

impl Number {
    pub fn new(literal: &str) -> Number {
        Number(literal.to_string())
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Map {
    entries: Vec<(String, Value)>,
}

impl Map {
    pub fn new() -> Map {
        Map::default()
    }

    pub fn insert(&mut self, key: String, value: Value) {
        if let Some(slot) = self.entries.iter_mut().find(|(k, _)| *k == key) {
            slot.1 = value;
        } else {
            self.entries.push((key, value));
        }
    }

    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn iter(&self) -> impl Iterator<Item = (&String, &Value)> {
        self.entries.iter().map(|(k, v)| (k, v))
    }

    pub fn keys(&self) -> impl Iterator<Item = &String> {
        self.entries.iter().map(|(k, _)| k)
    }

    fn take_values(&mut self) -> impl Iterator<Item = Value> + '_ {
        self.entries.drain(..).map(|(_, v)| v)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Null,
    Bool(bool),
    Number(Number),
    String(String),
    Array(Vec<Value>),
    Object(Map),
}

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }

    pub fn index(&self, i: usize) -> Option<&Value> {
        match self {
            Value::Array(a) => a.get(i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<&Number> {
        match self {
            Value::Number(n) => Some(n),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        self.as_number().and_then(|n| n.as_str().parse().ok())
    }

    pub fn as_u64(&self) -> Option<u64> {
        self.as_number().and_then(|n| n.as_str().parse().ok())
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let mut stack = Vec::new();
        match self {
            Value::Array(a) if !a.is_empty() => stack.append(a),
            Value::Object(m) if !m.is_empty() => stack.extend(m.take_values()),
            _ => return,
        }
        while let Some(mut v) = stack.pop() {
            match &mut v {
                Value::Array(a) => stack.append(a),
                Value::Object(m) => stack.extend(m.take_values()),
                _ => {}
            }
        }
    }
}

fn getu4(s: &[u8]) -> i32 {
    if s.len() < 6 || s[0] != b'\\' || s[1] != b'u' {
        return -1;
    }
    let mut r: i32 = 0;
    for &c in &s[2..6] {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return -1,
        };
        r = r * 16 + i32::from(d);
    }
    r
}

pub fn unquote_bytes(s: &[u8]) -> Option<Vec<u8>> {
    if s.len() < 2 || s[0] != b'"' || s[s.len() - 1] != b'"' {
        return None;
    }
    let s = &s[1..s.len() - 1];
    let mut r = 0;
    while r < s.len() {
        let c = s[r];
        if c == b'\\' || c == b'"' || c < b' ' {
            break;
        }
        if c < utf8::RUNE_SELF {
            r += 1;
            continue;
        }
        let (rr, size) = utf8::decode_rune(&s[r..]);
        if rr == utf8::RUNE_ERROR && size == 1 {
            break;
        }
        r += size;
    }
    if r == s.len() {
        return Some(s.to_vec());
    }
    let mut b = Vec::with_capacity(s.len() + 2 * utf8::UTF_MAX);
    b.extend_from_slice(&s[..r]);
    while r < s.len() {
        let c = s[r];
        if c == b'\\' {
            r += 1;
            if r >= s.len() {
                return None;
            }
            match s[r] {
                b'"' | b'\\' | b'/' | b'\'' => {
                    b.push(s[r]);
                    r += 1;
                }
                b'b' => {
                    b.push(0x08);
                    r += 1;
                }
                b'f' => {
                    b.push(0x0c);
                    r += 1;
                }
                b'n' => {
                    b.push(b'\n');
                    r += 1;
                }
                b'r' => {
                    b.push(b'\r');
                    r += 1;
                }
                b't' => {
                    b.push(b'\t');
                    r += 1;
                }
                b'u' => {
                    r -= 1;
                    let mut rr = getu4(&s[r..]);
                    if rr < 0 {
                        return None;
                    }
                    r += 6;
                    if utf16::is_surrogate(rr) {
                        let rr1 = getu4(&s[r..]);
                        let dec = utf16::decode_rune(rr, rr1);
                        if dec != utf8::RUNE_ERROR {
                            r += 6;
                            utf8::append_rune(&mut b, dec);
                            continue;
                        }
                        rr = utf8::RUNE_ERROR;
                    }
                    utf8::append_rune(&mut b, rr);
                }
                _ => return None,
            }
        } else if c == b'"' || c < b' ' {
            return None;
        } else if c < utf8::RUNE_SELF {
            b.push(c);
            r += 1;
        } else {
            let (rr, size) = utf8::decode_rune(&s[r..]);
            r += size;
            utf8::append_rune(&mut b, rr);
        }
    }
    Some(b)
}

enum Frame {
    Array(Vec<Value>),
    Object(Map, Option<String>),
}

fn skip_space(data: &[u8], mut i: usize) -> usize {
    while i < data.len() && is_space(data[i]) {
        i += 1;
    }
    i
}

fn string_end(data: &[u8], start: usize) -> usize {
    let mut i = start + 1;
    while i < data.len() {
        match data[i] {
            b'\\' => i += 2,
            b'"' => return i + 1,
            _ => i += 1,
        }
    }
    data.len()
}

fn literal_end(data: &[u8], start: usize) -> usize {
    let mut i = start;
    while i < data.len()
        && matches!(data[i], b'0'..=b'9' | b'-' | b'+' | b'.' | b'e' | b'E' | b'a'..=b'z')
    {
        i += 1;
    }
    i
}

fn string_value(raw: &[u8]) -> String {
    let bytes = unquote_bytes(raw).expect("string checked by the scanner");
    String::from_utf8(bytes).expect("unquote yields valid UTF-8")
}

pub fn unmarshal_value(data: &[u8]) -> Result<Value, SyntaxError> {
    check_valid(data)?;
    let mut stack: Vec<Frame> = Vec::new();
    let mut i = skip_space(data, 0);
    loop {
        i = skip_space(data, i);
        let c = data[i];
        let value = match c {
            b'{' => {
                i += 1;
                stack.push(Frame::Object(Map::new(), None));
                None
            }
            b'[' => {
                i += 1;
                stack.push(Frame::Array(Vec::new()));
                None
            }
            b']' => {
                i += 1;
                match stack.pop() {
                    Some(Frame::Array(a)) => Some(Value::Array(a)),
                    _ => unreachable!("scanner checked the brackets"),
                }
            }
            b'}' => {
                i += 1;
                match stack.pop() {
                    Some(Frame::Object(m, _)) => Some(Value::Object(m)),
                    _ => unreachable!("scanner checked the braces"),
                }
            }
            b',' | b':' => {
                i += 1;
                None
            }
            b'"' => {
                let end = string_end(data, i);
                let s = string_value(&data[i..end]);
                i = end;
                if let Some(Frame::Object(_, key @ None)) = stack.last_mut() {
                    *key = Some(s);
                    None
                } else {
                    Some(Value::String(s))
                }
            }
            _ => {
                let end = literal_end(data, i);
                let lit = &data[i..end];
                i = end;
                Some(match lit {
                    b"null" => Value::Null,
                    b"true" => Value::Bool(true),
                    b"false" => Value::Bool(false),
                    _ => Value::Number(Number(String::from_utf8_lossy(lit).into_owned())),
                })
            }
        };
        let Some(v) = value else { continue };
        match stack.last_mut() {
            None => return Ok(v),
            Some(Frame::Array(a)) => a.push(v),
            Some(Frame::Object(m, key)) => {
                let k = key.take().expect("object key precedes its value");
                m.insert(k, v);
            }
        }
    }
}
