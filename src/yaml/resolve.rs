use super::yaml::Error;
use super::yamlh::{
    BINARY_TAG, BOOL_TAG, FLOAT_TAG, INT_TAG, MERGE_TAG, NULL_TAG, STR_TAG, TIMESTAMP_TAG,
};
use crate::gocompat::strconv::{parse_float, parse_int, parse_uint};
use crate::gocompat::time::{Location, parse_in_location};

const LONG_TAG_PREFIX: &str = "tag:yaml.org,2002:";

const GO_NAN: u64 = 0x7FF8_0000_0000_0001;

#[derive(Clone, Debug, PartialEq)]
pub enum Resolved {
    Null,
    Bool(bool),
    Int(i64),
    Uint(u64),
    Float(f64),
    Str(Vec<u8>),
    Timestamp,
}

fn resolve_table(c: u8) -> u8 {
    match c {
        b'+' | b'-' => b'S',
        b'0'..=b'9' => b'D',
        b'y' | b'Y' | b'n' | b'N' | b't' | b'T' | b'f' | b'F' | b'o' | b'O' | b'~' => b'M',
        b'.' => b'.',
        _ => 0,
    }
}

fn resolve_map(s: &str) -> Option<(&'static str, Resolved)> {
    let item = match s {
        "y" | "Y" | "yes" | "Yes" | "YES" | "true" | "True" | "TRUE" | "on" | "On" | "ON" => {
            (BOOL_TAG, Resolved::Bool(true))
        }
        "n" | "N" | "no" | "No" | "NO" | "false" | "False" | "FALSE" | "off" | "Off" | "OFF" => {
            (BOOL_TAG, Resolved::Bool(false))
        }
        "" | "~" | "null" | "Null" | "NULL" => (NULL_TAG, Resolved::Null),
        ".nan" | ".NaN" | ".NAN" => (FLOAT_TAG, Resolved::Float(f64::from_bits(GO_NAN))),
        ".inf" | ".Inf" | ".INF" | "+.inf" | "+.Inf" | "+.INF" => {
            (FLOAT_TAG, Resolved::Float(f64::INFINITY))
        }
        "-.inf" | "-.Inf" | "-.INF" => (FLOAT_TAG, Resolved::Float(f64::NEG_INFINITY)),
        "<<" => (MERGE_TAG, Resolved::Str(b"<<".to_vec())),
        _ => return None,
    };
    Some(item)
}

pub(super) fn short_tag(tag: &str) -> String {
    match tag.strip_prefix(LONG_TAG_PREFIX) {
        Some(rest) => format!("!!{rest}"),
        None => tag.to_string(),
    }
}

fn resolvable_tag(tag: &str) -> bool {
    matches!(
        tag,
        "" | STR_TAG | BOOL_TAG | INT_TAG | FLOAT_TAG | NULL_TAG | TIMESTAMP_TAG
    )
}

fn yaml_style_float(s: &[u8]) -> bool {
    let n = s.len();
    let mut i = 0;
    let digits = |i: &mut usize| {
        let start = *i;
        while *i < n && s[*i].is_ascii_digit() {
            *i += 1;
        }
        *i > start
    };
    if i < n && (s[i] == b'+' || s[i] == b'-') {
        i += 1;
    }
    if i < n && s[i] == b'.' {
        i += 1;
        if !digits(&mut i) {
            return false;
        }
    } else {
        if !digits(&mut i) {
            return false;
        }
        if i < n && s[i] == b'.' {
            i += 1;
            digits(&mut i);
        }
    }
    if i < n && (s[i] == b'e' || s[i] == b'E') {
        i += 1;
        if i < n && (s[i] == b'+' || s[i] == b'-') {
            i += 1;
        }
        if !digits(&mut i) {
            return false;
        }
    }
    i == n
}

const TIMESTAMP_FORMATS: [&str; 4] = [
    "2006-1-2T15:4:5.999999999Z07:00",
    "2006-1-2t15:4:5.999999999Z07:00",
    "2006-1-2 15:4:5.999999999",
    "2006-1-2",
];

fn parse_timestamp(s: &str) -> bool {
    let b = s.as_bytes();
    let i = b.iter().take_while(|c| c.is_ascii_digit()).count();
    if i != 4 || i == b.len() || b[i] != b'-' {
        return false;
    }
    let utc = Location::utc();
    TIMESTAMP_FORMATS
        .iter()
        .any(|layout| parse_in_location(layout, b, &utc).is_ok())
}

pub(super) fn resolve(tag: &str, input: &str) -> Result<(String, Resolved), Error> {
    if !resolvable_tag(tag) {
        return Ok((tag.to_string(), Resolved::Str(input.as_bytes().to_vec())));
    }
    let (rtag, out) = resolve_inner(tag, input);
    if tag.is_empty() || tag == rtag || tag == STR_TAG || tag == BINARY_TAG {
        return Ok((rtag.to_string(), out));
    }
    if tag == FLOAT_TAG
        && rtag == INT_TAG
        && let Resolved::Int(v) = out
    {
        return Ok((FLOAT_TAG.to_string(), Resolved::Float(v as f64)));
    }
    Err(Error::TagMismatch {
        resolved: short_tag(rtag),
        tag: short_tag(tag),
        text: input.to_string(),
    })
}

fn resolve_inner(tag: &str, input: &str) -> (&'static str, Resolved) {
    let hint = match input.as_bytes().first() {
        None => b'N',
        Some(&c) => resolve_table(c),
    };
    if hint != 0 && tag != STR_TAG && tag != BINARY_TAG {
        if let Some(item) = resolve_map(input) {
            return item;
        }
        match hint {
            b'.' => {
                if let Ok(f) = parse_float(input.as_bytes(), 64) {
                    return (FLOAT_TAG, Resolved::Float(f));
                }
            }
            b'D' | b'S' => {
                if (tag.is_empty() || tag == TIMESTAMP_TAG) && parse_timestamp(input) {
                    return (TIMESTAMP_TAG, Resolved::Timestamp);
                }
                let plain: Vec<u8> = input.bytes().filter(|&c| c != b'_').collect();
                if let Ok(v) = parse_int(&plain, 0, 64) {
                    return (INT_TAG, Resolved::Int(v));
                }
                if let Ok(v) = parse_uint(&plain, 0, 64) {
                    return (INT_TAG, Resolved::Uint(v));
                }
                if yaml_style_float(&plain)
                    && let Ok(f) = parse_float(&plain, 64)
                {
                    return (FLOAT_TAG, Resolved::Float(f));
                }
                if let Some(rest) = plain.strip_prefix(b"0b") {
                    if let Ok(v) = parse_int(rest, 2, 64) {
                        return (INT_TAG, Resolved::Int(v));
                    }
                    if let Ok(v) = parse_uint(rest, 2, 64) {
                        return (INT_TAG, Resolved::Uint(v));
                    }
                } else if let Some(rest) = plain.strip_prefix(b"-0b") {
                    let mut negative = b"-".to_vec();
                    negative.extend_from_slice(rest);
                    if let Ok(v) = parse_int(&negative, 2, 64) {
                        return (INT_TAG, Resolved::Int(v));
                    }
                }
            }
            _ => {}
        }
    }
    (STR_TAG, Resolved::Str(input.as_bytes().to_vec()))
}
