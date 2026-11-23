use super::isprint::{IS_GRAPHIC, IS_NOT_PRINT16, IS_NOT_PRINT32, IS_PRINT16, IS_PRINT32};
use crate::gocompat::utf8::{RUNE_ERROR, RUNE_SELF, Rune, append_rune, decode_rune};

pub(super) static LOWERHEX: &[u8; 16] = b"0123456789abcdef";
pub(super) static UPPERHEX: &[u8; 16] = b"0123456789ABCDEF";

const MAX_RUNE: Rune = 0x10FFFF;
const SURROGATE_MIN: Rune = 0xD800;
const SURROGATE_MAX: Rune = 0xDFFF;

fn valid_rune(r: Rune) -> bool {
    (0..SURROGATE_MIN).contains(&r) || (SURROGATE_MAX + 1..=MAX_RUNE).contains(&r)
}

fn into_string(b: Vec<u8>) -> String {
    match String::from_utf8(b) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

fn quote_with(s: &[u8], quote: u8, ascii_only: bool, graphic_only: bool) -> String {
    let mut buf = Vec::with_capacity(3 * s.len() / 2);
    append_quoted_with(&mut buf, s, quote, ascii_only, graphic_only);
    into_string(buf)
}

fn quote_rune_with(r: Rune, quote: u8, ascii_only: bool, graphic_only: bool) -> String {
    let mut buf = Vec::new();
    append_quoted_rune_with(&mut buf, r, quote, ascii_only, graphic_only);
    into_string(buf)
}

fn append_quoted_with(
    buf: &mut Vec<u8>,
    s: &[u8],
    quote: u8,
    ascii_only: bool,
    graphic_only: bool,
) {
    buf.reserve(s.len() + 2);
    buf.push(quote);
    let mut s = s;
    while !s.is_empty() {
        let mut r = Rune::from(s[0]);
        let mut width = 1;
        if s[0] >= RUNE_SELF {
            (r, width) = decode_rune(s);
        }
        if width == 1 && r == RUNE_ERROR {
            buf.extend_from_slice(b"\\x");
            buf.push(LOWERHEX[usize::from(s[0] >> 4)]);
            buf.push(LOWERHEX[usize::from(s[0] & 0xF)]);
            s = &s[width..];
            continue;
        }
        append_escaped_rune(buf, r, quote, ascii_only, graphic_only);
        s = &s[width..];
    }
    buf.push(quote);
}

fn append_quoted_rune_with(
    buf: &mut Vec<u8>,
    r: Rune,
    quote: u8,
    ascii_only: bool,
    graphic_only: bool,
) {
    buf.push(quote);
    let r = if valid_rune(r) { r } else { RUNE_ERROR };
    append_escaped_rune(buf, r, quote, ascii_only, graphic_only);
    buf.push(quote);
}

fn append_escaped_rune(
    buf: &mut Vec<u8>,
    r: Rune,
    quote: u8,
    ascii_only: bool,
    graphic_only: bool,
) {
    if r == Rune::from(quote) || r == Rune::from(b'\\') {
        buf.push(b'\\');
        buf.push(r as u8);
        return;
    }
    if ascii_only {
        if r < Rune::from(RUNE_SELF) && is_print(r) {
            buf.push(r as u8);
            return;
        }
    } else if is_print(r) || (graphic_only && is_in_graphic_list(r)) {
        append_rune(buf, r);
        return;
    }
    match r {
        0x07 => buf.extend_from_slice(b"\\a"),
        0x08 => buf.extend_from_slice(b"\\b"),
        0x0C => buf.extend_from_slice(b"\\f"),
        0x0A => buf.extend_from_slice(b"\\n"),
        0x0D => buf.extend_from_slice(b"\\r"),
        0x09 => buf.extend_from_slice(b"\\t"),
        0x0B => buf.extend_from_slice(b"\\v"),
        _ => {
            if r < Rune::from(b' ') || r == 0x7F {
                buf.extend_from_slice(b"\\x");
                buf.push(LOWERHEX[usize::from(r as u8 >> 4)]);
                buf.push(LOWERHEX[usize::from(r as u8 & 0xF)]);
            } else {
                let r = if valid_rune(r) { r } else { 0xFFFD };
                if r < 0x10000 {
                    buf.extend_from_slice(b"\\u");
                    for s in (0..=12).rev().step_by(4) {
                        buf.push(LOWERHEX[((r >> s) & 0xF) as usize]);
                    }
                } else {
                    buf.extend_from_slice(b"\\U");
                    for s in (0..=28).rev().step_by(4) {
                        buf.push(LOWERHEX[((r >> s) & 0xF) as usize]);
                    }
                }
            }
        }
    }
}

pub fn quote(s: &[u8]) -> String {
    quote_with(s, b'"', false, false)
}

pub fn append_quote(dst: &mut Vec<u8>, s: &[u8]) {
    append_quoted_with(dst, s, b'"', false, false);
}

pub fn quote_to_ascii(s: &[u8]) -> String {
    quote_with(s, b'"', true, false)
}

pub fn append_quote_to_ascii(dst: &mut Vec<u8>, s: &[u8]) {
    append_quoted_with(dst, s, b'"', true, false);
}

pub fn quote_to_graphic(s: &[u8]) -> String {
    quote_with(s, b'"', false, true)
}

pub fn append_quote_to_graphic(dst: &mut Vec<u8>, s: &[u8]) {
    append_quoted_with(dst, s, b'"', false, true);
}

pub fn quote_rune(r: Rune) -> String {
    quote_rune_with(r, b'\'', false, false)
}

pub fn append_quote_rune(dst: &mut Vec<u8>, r: Rune) {
    append_quoted_rune_with(dst, r, b'\'', false, false);
}

pub fn quote_rune_to_ascii(r: Rune) -> String {
    quote_rune_with(r, b'\'', true, false)
}

pub fn append_quote_rune_to_ascii(dst: &mut Vec<u8>, r: Rune) {
    append_quoted_rune_with(dst, r, b'\'', true, false);
}

pub fn quote_rune_to_graphic(r: Rune) -> String {
    quote_rune_with(r, b'\'', false, true)
}

pub fn append_quote_rune_to_graphic(dst: &mut Vec<u8>, r: Rune) {
    append_quoted_rune_with(dst, r, b'\'', false, true);
}

pub fn can_backquote(s: &[u8]) -> bool {
    let mut s = s;
    while !s.is_empty() {
        let (r, wid) = decode_rune(s);
        s = &s[wid..];
        if wid > 1 {
            if r == 0xFEFF {
                return false;
            }
            continue;
        }
        if r == RUNE_ERROR {
            return false;
        }
        if (r < Rune::from(b' ') && r != Rune::from(b'\t')) || r == Rune::from(b'`') || r == 0x7F {
            return false;
        }
    }
    true
}

fn unhex(b: u8) -> Option<Rune> {
    match b {
        b'0'..=b'9' => Some(Rune::from(b - b'0')),
        b'a'..=b'f' => Some(Rune::from(b - b'a' + 10)),
        b'A'..=b'F' => Some(Rune::from(b - b'A' + 10)),
        _ => None,
    }
}

pub fn unquote_char(s: &[u8], quote: u8) -> Option<(Rune, bool, &[u8])> {
    let &c = s.first()?;
    if c == quote && (quote == b'\'' || quote == b'"') {
        return None;
    }
    if c >= RUNE_SELF {
        let (r, size) = decode_rune(s);
        return Some((r, true, &s[size..]));
    }
    if c != b'\\' {
        return Some((Rune::from(c), false, &s[1..]));
    }

    if s.len() <= 1 {
        return None;
    }
    let c = s[1];
    let mut s = &s[2..];

    let mut multibyte = false;
    let value: Rune = match c {
        b'a' => 0x07,
        b'b' => 0x08,
        b'f' => 0x0C,
        b'n' => 0x0A,
        b'r' => 0x0D,
        b't' => 0x09,
        b'v' => 0x0B,
        b'x' | b'u' | b'U' => {
            let n = match c {
                b'x' => 2,
                b'u' => 4,
                _ => 8,
            };
            if s.len() < n {
                return None;
            }
            let mut v: Rune = 0;
            for &b in &s[..n] {
                let x = unhex(b)?;
                v = (v << 4) | x;
            }
            s = &s[n..];
            if c != b'x' {
                if !valid_rune(v) {
                    return None;
                }
                multibyte = true;
            }
            v
        }
        b'0'..=b'7' => {
            let mut v = Rune::from(c - b'0');
            if s.len() < 2 {
                return None;
            }
            for &b in &s[..2] {
                let x = Rune::from(b) - Rune::from(b'0');
                if !(0..=7).contains(&x) {
                    return None;
                }
                v = (v << 3) | x;
            }
            s = &s[2..];
            if v > 255 {
                return None;
            }
            v
        }
        b'\\' => Rune::from(b'\\'),
        b'\'' | b'"' => {
            if c != quote {
                return None;
            }
            Rune::from(c)
        }
        _ => return None,
    };
    Some((value, multibyte, s))
}

pub fn quoted_prefix(s: &[u8]) -> Option<Vec<u8>> {
    unquote_prefix(s, false).map(|(out, _)| out)
}

pub fn unquote(s: &[u8]) -> Option<Vec<u8>> {
    let (out, rem) = unquote_prefix(s, true)?;
    if !rem.is_empty() {
        return None;
    }
    Some(out)
}

fn unquote_prefix(input: &[u8], unescape: bool) -> Option<(Vec<u8>, &[u8])> {
    if input.len() < 2 {
        return None;
    }
    let quote = input[0];
    let end = input[1..].iter().position(|&b| b == quote)? + 2;

    match quote {
        b'`' => {
            let out = if !unescape {
                input[..end].to_vec()
            } else if !input[..end].contains(&b'\r') {
                input[1..end - 1].to_vec()
            } else {
                input[1..end - 1]
                    .iter()
                    .copied()
                    .filter(|&b| b != b'\r')
                    .collect()
            };
            Some((out, &input[end..]))
        }
        b'"' | b'\'' => {
            let head = &input[..end];
            if !head.contains(&b'\\') && !head.contains(&b'\n') {
                let inner = &input[1..end - 1];
                let valid = if quote == b'"' {
                    std::str::from_utf8(inner).is_ok()
                } else {
                    let (r, n) = decode_rune(inner);
                    1 + n + 1 == end && (r != RUNE_ERROR || n != 1)
                };
                if valid {
                    let out = if unescape {
                        inner.to_vec()
                    } else {
                        head.to_vec()
                    };
                    return Some((out, &input[end..]));
                }
            }

            let in0 = input;
            let mut rest = &input[1..];
            let mut buf = Vec::new();
            while !rest.is_empty() && rest[0] != quote {
                if rest[0] == b'\n' {
                    return None;
                }
                let (r, multibyte, rem) = unquote_char(rest, quote)?;
                rest = rem;

                if unescape {
                    if r < Rune::from(RUNE_SELF) || !multibyte {
                        buf.push(r as u8);
                    } else {
                        append_rune(&mut buf, r);
                    }
                }

                if quote == b'\'' {
                    break;
                }
            }

            if rest.first() != Some(&quote) {
                return None;
            }
            rest = &rest[1..];

            if unescape {
                return Some((buf, rest));
            }
            Some((in0[..in0.len() - rest.len()].to_vec(), rest))
        }
        _ => None,
    }
}

fn bsearch16(s: &[u16], v: u16) -> (usize, bool) {
    let i = s.partition_point(|&x| x < v);
    (i, i < s.len() && s[i] == v)
}

fn bsearch32(s: &[u32], v: u32) -> (usize, bool) {
    let i = s.partition_point(|&x| x < v);
    (i, i < s.len() && s[i] == v)
}

pub fn is_print(r: Rune) -> bool {
    if r <= 0xFF {
        if (0x20..=0x7E).contains(&r) {
            return true;
        }
        if (0xA1..=0xFF).contains(&r) {
            return r != 0xAD;
        }
        return false;
    }

    if (0..1 << 16).contains(&r) {
        let rr = r as u16;
        let (i, _) = bsearch16(IS_PRINT16, rr);
        if i >= IS_PRINT16.len() || rr < IS_PRINT16[i & !1] || IS_PRINT16[i | 1] < rr {
            return false;
        }
        let (_, found) = bsearch16(IS_NOT_PRINT16, rr);
        return !found;
    }

    let rr = r as u32;
    let (i, _) = bsearch32(IS_PRINT32, rr);
    if i >= IS_PRINT32.len() || rr < IS_PRINT32[i & !1] || IS_PRINT32[i | 1] < rr {
        return false;
    }
    if r >= 0x20000 {
        return true;
    }
    let r = r - 0x10000;
    let (_, found) = bsearch16(IS_NOT_PRINT32, r as u16);
    !found
}

pub fn is_graphic(r: Rune) -> bool {
    if is_print(r) {
        return true;
    }
    is_in_graphic_list(r)
}

fn is_in_graphic_list(r: Rune) -> bool {
    if r > 0xFFFF {
        return false;
    }
    let (_, found) = bsearch16(IS_GRAPHIC, r as u16);
    found
}
