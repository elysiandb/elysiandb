use crate::gocompat::time::{RFC1123, Time};

pub(crate) const UPPER_HEX: &[u8; 16] = b"0123456789ABCDEF";
pub(crate) const LOWER_HEX: &[u8; 16] = b"0123456789abcdef";

pub(crate) const MAX_HEX_INT_CHARS: usize = 15;

pub(crate) fn hex2int(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 16,
    }
}

pub(crate) fn to_lower(c: u8) -> u8 {
    c.to_ascii_lowercase()
}

pub(crate) fn to_upper(c: u8) -> u8 {
    c.to_ascii_uppercase()
}

pub(crate) fn lowercase_bytes(b: &mut [u8]) {
    for c in b.iter_mut() {
        *c = to_lower(*c);
    }
}

fn is_tchar(c: u8) -> bool {
    c.is_ascii_alphanumeric()
        || matches!(
            c,
            b'!' | b'#'
                | b'$'
                | b'%'
                | b'&'
                | b'\''
                | b'*'
                | b'+'
                | b'-'
                | b'.'
                | b'^'
                | b'_'
                | b'`'
                | b'|'
                | b'~'
        )
}

pub(crate) fn valid_header_field_byte(c: u8) -> bool {
    c < 128 && is_tchar(c)
}

pub(crate) fn valid_header_value_byte(c: u8) -> bool {
    (0x21..=0x7e).contains(&c) || c == 0x20 || c == 0x09 || c >= 0x80
}

pub(crate) fn valid_method_byte(c: u8) -> bool {
    is_tchar(c)
}

fn quoted_arg_should_escape(c: u8) -> bool {
    !(c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.' | b'~'))
}

fn quoted_path_should_escape(c: u8) -> bool {
    quoted_arg_should_escape(c)
        && !matches!(
            c,
            b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'@'
        )
}

pub(crate) fn case_insensitive_compare(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter().zip(b).all(|(x, y)| x | 0x20 == y | 0x20)
}

pub(crate) fn parse_uint_buf(b: &[u8]) -> (i64, usize, bool) {
    if b.is_empty() {
        return (-1, 0, false);
    }
    let mut v: i64 = 0;
    for (i, &c) in b.iter().enumerate() {
        let k = c.wrapping_sub(b'0');
        if k > 9 {
            if i == 0 {
                return (-1, i, false);
            }
            return (v, i, true);
        }
        let v_new = v.wrapping_mul(10).wrapping_add(i64::from(k));
        if v_new < v {
            return (-1, i, false);
        }
        v = v_new;
    }
    (v, b.len(), true)
}

pub fn parse_uint(buf: &[u8]) -> Option<i64> {
    let (v, n, ok) = parse_uint_buf(buf);
    if n != buf.len() || !ok {
        return None;
    }
    Some(v)
}

pub(crate) fn append_uint(dst: &mut Vec<u8>, n: i64) {
    assert!(n >= 0, "BUG: int must be positive");
    dst.extend_from_slice(n.to_string().as_bytes());
}

pub fn append_http_date(dst: &mut Vec<u8>, date: &Time) {
    let start = dst.len();
    date.utc().append_format(dst, RFC1123);
    let end = dst.len();
    if end - start >= 3 {
        dst[end - 3..end].copy_from_slice(b"GMT");
    }
}

pub(crate) fn append_quoted_arg(dst: &mut Vec<u8>, src: &[u8]) {
    for &c in src {
        if c == b' ' {
            dst.push(b'+');
        } else if quoted_arg_should_escape(c) {
            dst.extend_from_slice(&[
                b'%',
                UPPER_HEX[usize::from(c >> 4)],
                UPPER_HEX[usize::from(c & 0xf)],
            ]);
        } else {
            dst.push(c);
        }
    }
}

pub(crate) fn append_quoted_path(dst: &mut Vec<u8>, src: &[u8]) {
    if src == b"*" {
        dst.push(b'*');
        return;
    }
    for &c in src {
        if quoted_path_should_escape(c) {
            dst.extend_from_slice(&[
                b'%',
                UPPER_HEX[usize::from(c >> 4)],
                UPPER_HEX[usize::from(c & 0xf)],
            ]);
        } else {
            dst.push(c);
        }
    }
}

pub(crate) fn decode_arg_append(dst: &mut Vec<u8>, src: &[u8]) {
    let idx_percent = src.iter().position(|&c| c == b'%');
    let idx_plus = src.iter().position(|&c| c == b'+');
    let idx = match (idx_percent, idx_plus) {
        (None, None) => {
            dst.extend_from_slice(src);
            return;
        }
        (None, Some(p)) => p,
        (Some(p), None) => p,
        (Some(a), Some(b)) => a.min(b),
    };
    dst.extend_from_slice(&src[..idx]);
    let mut i = idx;
    while i < src.len() {
        let c = src[i];
        match c {
            b'%' => {
                if i + 2 >= src.len() {
                    dst.extend_from_slice(&src[i..]);
                    return;
                }
                let x2 = hex2int(src[i + 2]);
                let x1 = hex2int(src[i + 1]);
                if x1 == 16 || x2 == 16 {
                    dst.push(b'%');
                } else {
                    dst.push((x1 << 4) | x2);
                    i += 2;
                }
            }
            b'+' => dst.push(b' '),
            _ => dst.push(c),
        }
        i += 1;
    }
}

pub(crate) fn decode_arg_append_no_plus(dst: &mut Vec<u8>, src: &[u8]) {
    let Some(idx) = src.iter().position(|&c| c == b'%') else {
        dst.extend_from_slice(src);
        return;
    };
    dst.extend_from_slice(&src[..idx]);
    let mut i = idx;
    while i < src.len() {
        let c = src[i];
        if c == b'%' {
            if i + 2 >= src.len() {
                dst.extend_from_slice(&src[i..]);
                return;
            }
            let x2 = hex2int(src[i + 2]);
            let x1 = hex2int(src[i + 1]);
            if x1 == 16 || x2 == 16 {
                dst.push(b'%');
            } else {
                dst.push((x1 << 4) | x2);
                i += 2;
            }
        } else {
            dst.push(c);
        }
        i += 1;
    }
}

pub(crate) fn index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

pub(crate) fn last_index_of(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(haystack.len());
    }
    if needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).rposition(|w| w == needle)
}

pub(crate) fn index_byte(b: &[u8], c: u8) -> Option<usize> {
    b.iter().position(|&x| x == c)
}

pub(crate) fn last_index_byte(b: &[u8], c: u8) -> Option<usize> {
    b.iter().rposition(|&x| x == c)
}
