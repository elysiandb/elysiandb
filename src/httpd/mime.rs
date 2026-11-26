use std::collections::BTreeMap;

use crate::gocompat::strings::{to_lower, trim_space};
use crate::gocompat::unicode::is_space;
use crate::gocompat::utf8::decode_rune;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaTypeError {
    NoMediaType,
    ExpectedSlash,
    ExpectedToken,
    UnexpectedContent,
    InvalidParameter,
    DuplicateParameter,
}

impl std::fmt::Display for MediaTypeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            MediaTypeError::NoMediaType => "no media type",
            MediaTypeError::ExpectedSlash => "expected slash after first token",
            MediaTypeError::ExpectedToken => "expected token after slash",
            MediaTypeError::UnexpectedContent => "unexpected content after media subtype",
            MediaTypeError::InvalidParameter => "invalid media parameter",
            MediaTypeError::DuplicateParameter => "duplicate parameter name",
        })
    }
}

impl std::error::Error for MediaTypeError {}

fn is_tspecial(c: u8) -> bool {
    b"()<>@,;:\\\"/[]?=".contains(&c)
}

fn is_token_char(c: u8) -> bool {
    c > 0x20 && c < 0x7f && !is_tspecial(c)
}

fn trim_left_space(mut s: &[u8]) -> &[u8] {
    while !s.is_empty() {
        let (r, size) = decode_rune(s);
        if !is_space(r) {
            break;
        }
        s = &s[size..];
    }
    s
}

fn consume_token(v: &[u8]) -> (&[u8], &[u8]) {
    match v.iter().position(|&c| !is_token_char(c)) {
        None => (v, &[]),
        Some(0) => (&[], v),
        Some(n) => (&v[..n], &v[n..]),
    }
}

fn consume_value(v: &[u8]) -> (Vec<u8>, &[u8]) {
    if v.is_empty() {
        return (Vec::new(), v);
    }
    if v[0] != b'"' {
        let (t, rest) = consume_token(v);
        return (t.to_vec(), rest);
    }
    let mut buffer = Vec::new();
    let mut i = 1;
    while i < v.len() {
        let r = v[i];
        if r == b'"' {
            return (buffer, &v[i + 1..]);
        }
        if r == b'\\' && i + 1 < v.len() && is_tspecial(v[i + 1]) {
            buffer.push(v[i + 1]);
            i += 2;
            continue;
        }
        if r == b'\r' || r == b'\n' {
            return (Vec::new(), v);
        }
        buffer.push(v[i]);
        i += 1;
    }
    (Vec::new(), v)
}

fn consume_media_param(v: &[u8]) -> (Vec<u8>, Vec<u8>, &[u8]) {
    let rest = trim_left_space(v);
    let Some(rest) = rest.strip_prefix(b";") else {
        return (Vec::new(), Vec::new(), v);
    };
    let rest = trim_left_space(rest);
    let (param, rest) = consume_token(rest);
    let param = to_lower(param);
    if param.is_empty() {
        return (Vec::new(), Vec::new(), v);
    }
    let rest = trim_left_space(rest);
    let Some(rest) = rest.strip_prefix(b"=") else {
        return (Vec::new(), Vec::new(), v);
    };
    let rest = trim_left_space(rest);
    let (value, rest2) = consume_value(rest);
    if value.is_empty() && rest2.len() == rest.len() {
        return (Vec::new(), Vec::new(), v);
    }
    (param, value, rest2)
}

fn check_media_type_disposition(s: &[u8]) -> Result<(), MediaTypeError> {
    let (typ, rest) = consume_token(s);
    if typ.is_empty() {
        return Err(MediaTypeError::NoMediaType);
    }
    if rest.is_empty() {
        return Ok(());
    }
    let Some(rest) = rest.strip_prefix(b"/") else {
        return Err(MediaTypeError::ExpectedSlash);
    };
    let (subtype, rest) = consume_token(rest);
    if subtype.is_empty() {
        return Err(MediaTypeError::ExpectedToken);
    }
    if !rest.is_empty() {
        return Err(MediaTypeError::UnexpectedContent);
    }
    Ok(())
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

fn percent_hex_unescape(s: &[u8]) -> Option<Vec<u8>> {
    let mut i = 0;
    let mut percents = 0;
    while i < s.len() {
        if s[i] != b'%' {
            i += 1;
            continue;
        }
        percents += 1;
        if i + 2 >= s.len() || !ishex(s[i + 1]) || !ishex(s[i + 2]) {
            return None;
        }
        i += 3;
    }
    if percents == 0 {
        return Some(s.to_vec());
    }
    let mut t = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            t.push((unhex(s[i + 1]) << 4) | unhex(s[i + 2]));
            i += 3;
        } else {
            t.push(s[i]);
            i += 1;
        }
    }
    Some(t)
}

fn decode_2231_enc(v: &[u8]) -> Option<Vec<u8>> {
    let mut parts = v.splitn(3, |&c| c == b'\'');
    let charset = parts.next()?;
    let _lang = parts.next()?;
    let encoded = parts.next()?;
    let charset = to_lower(charset);
    if charset.is_empty() {
        return None;
    }
    if charset != b"us-ascii" && charset != b"utf-8" {
        return None;
    }
    percent_hex_unescape(encoded)
}

pub type MediaParams = BTreeMap<Vec<u8>, Vec<u8>>;

pub fn parse_media_type(v: &[u8]) -> Result<(Vec<u8>, MediaParams), MediaTypeError> {
    let base_len = v.iter().position(|&c| c == b';').unwrap_or(v.len());
    let base = &v[..base_len];
    let mediatype = trim_space(&to_lower(base)).to_vec();
    check_media_type_disposition(&mediatype)?;

    let mut params: MediaParams = BTreeMap::new();
    let mut continuation: BTreeMap<Vec<u8>, MediaParams> = BTreeMap::new();
    let mut v = &v[base_len..];
    while !v.is_empty() {
        v = trim_left_space(v);
        if v.is_empty() {
            break;
        }
        let (key, value, rest) = consume_media_param(v);
        if key.is_empty() {
            if trim_space(rest) == b";" {
                break;
            }
            return Err(MediaTypeError::InvalidParameter);
        }
        let pmap = match key.iter().position(|&c| c == b'*') {
            Some(star) => continuation.entry(key[..star].to_vec()).or_default(),
            None => &mut params,
        };
        if let Some(existing) = pmap.get(&key)
            && *existing != value
        {
            return Err(MediaTypeError::DuplicateParameter);
        }
        pmap.insert(key, value);
        v = rest;
    }

    for (key, piece_map) in &continuation {
        let mut single_part_key = key.clone();
        single_part_key.push(b'*');
        if let Some(v) = piece_map.get(&single_part_key) {
            if let Some(decv) = decode_2231_enc(v) {
                params.insert(key.clone(), decv);
            }
            continue;
        }
        let mut buf = Vec::new();
        let mut valid = false;
        let mut n = 0usize;
        loop {
            let mut simple_part = key.clone();
            simple_part.push(b'*');
            simple_part.extend_from_slice(n.to_string().as_bytes());
            if let Some(v) = piece_map.get(&simple_part) {
                valid = true;
                buf.extend_from_slice(v);
                n += 1;
                continue;
            }
            let mut encoded_part = simple_part;
            encoded_part.push(b'*');
            let Some(v) = piece_map.get(&encoded_part) else {
                break;
            };
            valid = true;
            if n == 0 {
                if let Some(decv) = decode_2231_enc(v) {
                    buf.extend_from_slice(&decv);
                }
            } else if let Some(decv) = percent_hex_unescape(v) {
                buf.extend_from_slice(&decv);
            }
            n += 1;
        }
        if valid {
            params.insert(key.clone(), buf);
        }
    }
    Ok((mediatype, params))
}
