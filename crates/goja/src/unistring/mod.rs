use gocompat::utf8::{self, Rune};
use gocompat::utf16;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

pub const BOM: u16 = 0xFEFF;

#[derive(Clone)]
enum Repr {
    Bytes(Arc<[u8]>),
    Utf16(Arc<[u16]>),
}

#[derive(Clone)]
pub struct String(Repr);

fn is_utf16_bytes(b: &[u8]) -> bool {
    b.len() >= 4 && b.len() & 1 == 0 && u16::from_le_bytes([b[0], b[1]]) == BOM
}

fn utf16_from_le_bytes(b: &[u8]) -> Vec<u16> {
    b.chunks_exact(2)
        .map(|c| u16::from_le_bytes([c[0], c[1]]))
        .collect()
}

fn utf16_to_le_bytes(u: &[u16]) -> Vec<u8> {
    let mut out = Vec::with_capacity(u.len() * 2);
    for &c in u {
        out.extend_from_slice(&c.to_le_bytes());
    }
    out
}

fn decode_runes(s: &[u8]) -> Vec<Rune> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (r, size) = utf8::decode_rune(&s[i..]);
        out.push(r);
        i += size;
    }
    out
}

pub fn scan(s: &[u8]) -> Option<Vec<u16>> {
    let mut utf16_size = 0;
    while utf16_size < s.len() {
        if s[utf16_size] >= utf8::RUNE_SELF {
            break;
        }
        utf16_size += 1;
    }
    if utf16_size == s.len() {
        return None;
    }
    for chr in decode_runes(&s[utf16_size..]) {
        utf16_size += 1;
        if chr > 0xFFFF {
            utf16_size += 1;
        }
    }
    let mut buf = vec![0u16; utf16_size + 1];
    buf[0] = BOM;
    let mut c = 1;
    for chr in decode_runes(s) {
        if chr <= 0xFFFF {
            buf[c] = chr as u16;
        } else {
            let (first, second) = utf16::encode_rune(chr);
            buf[c] = first as u16;
            c += 1;
            buf[c] = second as u16;
        }
        c += 1;
    }
    Some(buf)
}

pub fn new_from_string(s: &[u8]) -> String {
    match scan(s) {
        Some(buf) => from_utf16(&buf),
        None => String::from_bytes(s),
    }
}

pub fn new_from_runes(s: &[Rune]) -> String {
    let mut ascii = true;
    let mut size = 0;
    for &c in s {
        if c >= Rune::from(utf8::RUNE_SELF) {
            ascii = false;
            if c > 0xFFFF {
                size += 1;
            }
        }
        size += 1;
    }
    if ascii {
        let mut b = Vec::with_capacity(s.len());
        for &c in s {
            utf8::append_rune(&mut b, c);
        }
        return String::from_bytes(&b);
    }
    let mut b = vec![0u16; size + 1];
    b[0] = BOM;
    let mut i = 1;
    for &c in s {
        if c <= 0xFFFF {
            b[i] = c as u16;
        } else {
            let (first, second) = utf16::encode_rune(c);
            b[i] = first as u16;
            i += 1;
            b[i] = second as u16;
        }
        i += 1;
    }
    from_utf16(&b)
}

pub fn from_utf16(b: &[u16]) -> String {
    if b.len() >= 2 && b[0] == BOM {
        String(Repr::Utf16(Arc::from(b)))
    } else {
        String::from_bytes(&utf16_to_le_bytes(b))
    }
}

impl String {
    pub fn from_bytes(b: &[u8]) -> String {
        if is_utf16_bytes(b) {
            String(Repr::Utf16(Arc::from(utf16_from_le_bytes(b))))
        } else {
            String(Repr::Bytes(Arc::from(b)))
        }
    }

    pub fn new(s: &str) -> String {
        String::from_bytes(s.as_bytes())
    }

    pub fn as_utf16(&self) -> Option<&[u16]> {
        match &self.0 {
            Repr::Utf16(u) => Some(u),
            Repr::Bytes(_) => None,
        }
    }

    pub fn ascii_bytes(&self) -> Option<&[u8]> {
        match &self.0 {
            Repr::Bytes(b) => Some(b),
            Repr::Utf16(_) => None,
        }
    }

    pub fn as_bytes(&self) -> Cow<'_, [u8]> {
        match &self.0 {
            Repr::Bytes(b) => Cow::Borrowed(b),
            Repr::Utf16(u) => Cow::Owned(utf16_to_le_bytes(u)),
        }
    }

    pub fn len(&self) -> usize {
        match &self.0 {
            Repr::Bytes(b) => b.len(),
            Repr::Utf16(u) => u.len() * 2,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn go_string(&self) -> Vec<u8> {
        match &self.0 {
            Repr::Bytes(b) => b.to_vec(),
            Repr::Utf16(u) => {
                let mut out = Vec::with_capacity(u.len() * 3);
                for r in utf16::decode(&u[1..]) {
                    utf8::append_rune(&mut out, r);
                }
                out
            }
        }
    }

    pub fn string(&self) -> std::string::String {
        match std::string::String::from_utf8(self.go_string()) {
            Ok(s) => s,
            Err(e) => std::string::String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    }

    fn byte_at(&self, i: usize) -> u8 {
        match &self.0 {
            Repr::Bytes(b) => b[i],
            Repr::Utf16(u) => u[i / 2].to_le_bytes()[i & 1],
        }
    }
}

impl Default for String {
    fn default() -> String {
        String(Repr::Bytes(Arc::from(&b""[..])))
    }
}

impl PartialEq for String {
    fn eq(&self, other: &String) -> bool {
        match (&self.0, &other.0) {
            (Repr::Bytes(a), Repr::Bytes(b)) => a == b,
            (Repr::Utf16(a), Repr::Utf16(b)) => a == b,
            _ => false,
        }
    }
}

impl Eq for String {}

impl PartialEq<str> for String {
    fn eq(&self, other: &str) -> bool {
        matches!(&self.0, Repr::Bytes(b) if &b[..] == other.as_bytes())
    }
}

impl PartialEq<&str> for String {
    fn eq(&self, other: &&str) -> bool {
        self == *other
    }
}

impl Ord for String {
    fn cmp(&self, other: &String) -> Ordering {
        if let (Repr::Bytes(a), Repr::Bytes(b)) = (&self.0, &other.0) {
            return a.cmp(b);
        }
        let n = self.len().min(other.len());
        for i in 0..n {
            match self.byte_at(i).cmp(&other.byte_at(i)) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        self.len().cmp(&other.len())
    }
}

impl PartialOrd for String {
    fn partial_cmp(&self, other: &String) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Hash for String {
    fn hash<H: Hasher>(&self, state: &mut H) {
        match &self.0 {
            Repr::Bytes(b) => {
                state.write_u8(0);
                b.hash(state);
            }
            Repr::Utf16(u) => {
                state.write_u8(1);
                u.hash(state);
            }
        }
    }
}

impl fmt::Display for String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

impl fmt::Debug for String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.0 {
            Repr::Bytes(_) => write!(f, "{:?}", self.string()),
            Repr::Utf16(_) => write!(f, "u{:?}", self.string()),
        }
    }
}

impl From<&str> for String {
    fn from(s: &str) -> String {
        String::new(s)
    }
}
