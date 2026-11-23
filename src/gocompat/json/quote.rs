use crate::gocompat::utf8::{RUNE_ERROR, RUNE_SELF, decode_rune};

const HEX: &[u8; 16] = b"0123456789abcdef";

fn push_u00(dst: &mut String, b: u8) {
    dst.push_str("\\u00");
    dst.push(char::from(HEX[usize::from(b >> 4)]));
    dst.push(char::from(HEX[usize::from(b & 0xF)]));
}

pub fn append_quote_text(dst: &mut String, s: &[u8]) {
    dst.reserve(s.len() + 2);
    dst.push('"');
    let mut i = 0;
    while i < s.len() {
        let b = s[i];
        if b < RUNE_SELF {
            match b {
                b'\\' | b'"' => {
                    dst.push('\\');
                    dst.push(char::from(b));
                }
                0x08 => dst.push_str("\\b"),
                0x0C => dst.push_str("\\f"),
                b'\n' => dst.push_str("\\n"),
                b'\r' => dst.push_str("\\r"),
                b'\t' => dst.push_str("\\t"),
                c if c < 0x20 => push_u00(dst, c),
                _ => dst.push(char::from(b)),
            }
            i += 1;
            continue;
        }
        let (c, size) = decode_rune(&s[i..]);
        if c == RUNE_ERROR && size == 1 {
            dst.push_str("\\ufffd");
            i += 1;
            continue;
        }
        if c == 0x2028 || c == 0x2029 {
            dst.push_str("\\u202");
            dst.push(char::from(HEX[(c & 0xF) as usize]));
            i += size;
            continue;
        }
        dst.push(char::from_u32(c as u32).unwrap_or(char::REPLACEMENT_CHARACTER));
        i += size;
    }
    dst.push('"');
}

pub fn quote_text(s: &[u8]) -> String {
    let mut dst = String::new();
    append_quote_text(&mut dst, s);
    dst
}
