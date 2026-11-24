use crate::utf8::{RUNE_ERROR, RUNE_SELF, UTF_MAX, decode_rune};

pub(crate) const HEX: &[u8; 16] = b"0123456789abcdef";

fn safe(b: u8, escape_html: bool) -> bool {
    if b < 0x20 || b == b'"' || b == b'\\' {
        return false;
    }
    !(escape_html && (b == b'<' || b == b'>' || b == b'&'))
}

pub fn append_string(dst: &mut Vec<u8>, src: &[u8], escape_html: bool) {
    dst.reserve(src.len() + 2);
    dst.push(b'"');
    let mut start = 0;
    let mut i = 0;
    while i < src.len() {
        let b = src[i];
        if b < RUNE_SELF {
            if safe(b, escape_html) {
                i += 1;
                continue;
            }
            dst.extend_from_slice(&src[start..i]);
            match b {
                b'\\' | b'"' => {
                    dst.push(b'\\');
                    dst.push(b);
                }
                0x08 => dst.extend_from_slice(b"\\b"),
                0x0C => dst.extend_from_slice(b"\\f"),
                b'\n' => dst.extend_from_slice(b"\\n"),
                b'\r' => dst.extend_from_slice(b"\\r"),
                b'\t' => dst.extend_from_slice(b"\\t"),
                _ => {
                    dst.extend_from_slice(b"\\u00");
                    dst.push(HEX[usize::from(b >> 4)]);
                    dst.push(HEX[usize::from(b & 0xF)]);
                }
            }
            i += 1;
            start = i;
            continue;
        }
        let end = (i + UTF_MAX).min(src.len());
        let (c, size) = decode_rune(&src[i..end]);
        if c == RUNE_ERROR && size == 1 {
            dst.extend_from_slice(&src[start..i]);
            dst.extend_from_slice(b"\\ufffd");
            i += size;
            start = i;
            continue;
        }
        if c == 0x2028 || c == 0x2029 {
            dst.extend_from_slice(&src[start..i]);
            dst.extend_from_slice(b"\\u202");
            dst.push(HEX[(c & 0xF) as usize]);
            i += size;
            start = i;
            continue;
        }
        i += size;
    }
    dst.extend_from_slice(&src[start..]);
    dst.push(b'"');
}

pub(crate) fn append_html_escape(dst: &mut Vec<u8>, src: &[u8]) {
    let mut start = 0;
    for (i, &c) in src.iter().enumerate() {
        if c == b'<' || c == b'>' || c == b'&' {
            dst.extend_from_slice(&src[start..i]);
            dst.extend_from_slice(b"\\u00");
            dst.push(HEX[usize::from(c >> 4)]);
            dst.push(HEX[usize::from(c & 0xF)]);
            start = i + 1;
        }
        if c == 0xE2 && i + 2 < src.len() && src[i + 1] == 0x80 && src[i + 2] & !1 == 0xA8 {
            dst.extend_from_slice(&src[start..i]);
            dst.extend_from_slice(b"\\u202");
            dst.push(HEX[usize::from(src[i + 2] & 0xF)]);
            start = i + 3;
        }
    }
    dst.extend_from_slice(&src[start..]);
}

fn into_text(bytes: Vec<u8>) -> String {
    match String::from_utf8(bytes) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

pub fn append_quote_text(dst: &mut String, s: &[u8]) {
    let mut out = Vec::with_capacity(s.len() + 2);
    append_string(&mut out, s, false);
    dst.push_str(&into_text(out));
}

pub fn quote_text(s: &[u8]) -> String {
    let mut out = Vec::with_capacity(s.len() + 2);
    append_string(&mut out, s, false);
    into_text(out)
}
