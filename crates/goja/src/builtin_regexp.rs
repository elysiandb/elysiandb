use crate::deps::regexp;
use crate::deps::regexp::syntax::ErrorCode;
use crate::parser::{TransformError, transform_reg_exp};
use crate::regexp::{RegexpPattern, RegexpWrapper, compile_regexp2};
use gocompat::utf8;
use gocompat::utf16;
use std::sync::Arc;

const HEX: &[u8] = b"0123456789abcdef";

fn is_utf16_first_surrogate(c: u16) -> bool {
    (0xD800..=0xDBFF).contains(&c)
}

fn is_utf16_second_surrogate(c: u16) -> bool {
    (0xDC00..=0xDFFF).contains(&c)
}

fn decode_hex(s: &[u8]) -> Option<i64> {
    let mut hex: i64 = 0;
    for &chr in s {
        let n = match chr {
            b'0'..=b'9' => chr - b'0',
            b'a'..=b'f' => chr - b'a' + 10,
            b'A'..=b'F' => chr - b'A' + 10,
            _ => return None,
        };
        hex = hex * 16 + i64::from(n);
    }
    Some(hex)
}

fn write_hex4(b: &mut Vec<u8>, i: i64) {
    b.push(HEX[(i >> 12) as usize]);
    b.push(HEX[((i >> 8) & 0xF) as usize]);
    b.push(HEX[((i >> 4) & 0xF) as usize]);
    b.push(HEX[(i & 0xF) as usize]);
}

pub fn convert_regexp_to_unicode(pattern_str: &[u8]) -> Vec<u8> {
    let mut sb = Vec::new();
    let mut pos = 0usize;
    let mut i = 0usize;
    while (i as isize) < pattern_str.len() as isize - 11 {
        let (r, size) = utf8::decode_rune(&pattern_str[i..]);
        if r == '\\' as i32 {
            i += 1;
            if pattern_str[i] == b'u'
                && pattern_str[i + 5] == b'\\'
                && pattern_str[i + 6] == b'u'
                && let Some(first) = decode_hex(&pattern_str[i + 1..i + 5])
                && is_utf16_first_surrogate(first as u16)
                && let Some(second) = decode_hex(&pattern_str[i + 7..i + 11])
                && is_utf16_second_surrogate(second as u16)
            {
                let r = utf16::decode_rune(first as i32, second as i32);
                sb.extend_from_slice(&pattern_str[pos..i - 1]);
                utf8::append_rune(&mut sb, r);
                i += 11;
                pos = i;
                continue;
            }
            i += 1;
        } else {
            i += size;
        }
    }
    if pos > 0 {
        sb.extend_from_slice(&pattern_str[pos..]);
        return sb;
    }
    pattern_str.to_vec()
}

pub fn convert_regexp_to_utf16(pattern_str: &[u8]) -> Vec<u8> {
    let mut sb = Vec::new();
    let mut pos = 0usize;
    let mut prev_rune = 0i32;
    let mut i = 0usize;
    while i < pattern_str.len() {
        let (r, size) = utf8::decode_rune(&pattern_str[i..]);
        if r > 0xFFFF {
            sb.extend_from_slice(&pattern_str[pos..i]);
            if prev_rune == '\\' as i32 {
                sb.push(b'\\');
            }
            let (first, second) = utf16::encode_rune(r);
            sb.extend_from_slice(b"\\u");
            write_hex4(&mut sb, i64::from(first));
            sb.extend_from_slice(b"\\u");
            write_hex4(&mut sb, i64::from(second));
            pos = i + size;
        }
        i += size;
        prev_rune = r;
    }
    if pos > 0 {
        sb.extend_from_slice(&pattern_str[pos..]);
        return sb;
    }
    pattern_str.to_vec()
}

fn invalid_pattern(pattern: &[u8], reason: &str) -> String {
    format!(
        "Invalid regular expression: /{}/: {}",
        String::from_utf8_lossy(pattern),
        reason
    )
}

pub fn compile_regexp(pattern: &[u8], flags: &[u8]) -> Result<RegexpPattern, String> {
    let (mut global, mut ignore_case, mut multiline, mut dot_all, mut sticky, mut unicode) =
        (false, false, false, false, false, false);
    if !flags.is_empty() {
        let invalid_flags = || {
            format!(
                "Invalid flags supplied to RegExp constructor '{}'",
                String::from_utf8_lossy(flags)
            )
        };
        let mut i = 0;
        while i < flags.len() {
            let (chr, size) = utf8::decode_rune(&flags[i..]);
            i += size;
            let flag = match u8::try_from(chr) {
                Ok(c) => c,
                Err(_) => return Err(invalid_flags()),
            };
            let seen = match flag {
                b'g' => &mut global,
                b'm' => &mut multiline,
                b's' => &mut dot_all,
                b'i' => &mut ignore_case,
                b'y' => &mut sticky,
                b'u' => {
                    unicode = true;
                    continue;
                }
                _ => return Err(invalid_flags()),
            };
            if *seen {
                return Err(invalid_flags());
            }
            *seen = true;
        }
    }

    let pattern_str = if unicode {
        convert_regexp_to_unicode(pattern)
    } else {
        convert_regexp_to_utf16(pattern)
    };

    let mut wrapper = None;
    match transform_reg_exp(&pattern_str, dot_all, unicode) {
        Ok(re2_str) => {
            let mut re2_flags = String::new();
            if multiline {
                re2_flags.push('m');
            }
            if dot_all {
                re2_flags.push('s');
            }
            if ignore_case {
                re2_flags.push('i');
            }
            let re2_str = if !re2_flags.is_empty() {
                let mut s = format!("(?{re2_flags}:").into_bytes();
                s.extend_from_slice(&re2_str);
                s.push(b')');
                s
            } else {
                re2_str
            };
            match regexp::compile(&re2_str) {
                Ok(p) => wrapper = Some(Arc::new(RegexpWrapper(p))),
                Err(e) => {
                    if e.code != ErrorCode::InvalidRepeatSize {
                        return Err(invalid_pattern(pattern, e.code.reason()));
                    }
                }
            }
        }
        Err(e) => {
            if let TransformError::Syntax(_) = e {
                return Err(e.message().to_string());
            }
        }
    }

    let mut wrapper2 = None;
    if wrapper.is_none() {
        match compile_regexp2(&pattern_str, multiline, dot_all, ignore_case, unicode) {
            Ok(w) => wrapper2 = Some(w),
            Err(e) => return Err(invalid_pattern(pattern, &e.reason())),
        }
    }

    Ok(RegexpPattern {
        src: pattern_str,
        global,
        ignore_case,
        multiline,
        dot_all,
        sticky,
        unicode,
        regexp_wrapper: wrapper,
        regexp2_wrapper: wrapper2,
    })
}
