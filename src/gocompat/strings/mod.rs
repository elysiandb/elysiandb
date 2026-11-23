use crate::gocompat::unicode;
use crate::gocompat::utf8::{self, RUNE_ERROR, RUNE_SELF, Rune, UTF_MAX};

static ASCII_SPACE: [bool; 256] = {
    let mut t = [false; 256];
    t[b'\t' as usize] = true;
    t[b'\n' as usize] = true;
    t[0x0B] = true;
    t[0x0C] = true;
    t[b'\r' as usize] = true;
    t[b' ' as usize] = true;
    t
};

pub fn map(mapping: impl Fn(Rune) -> Rune, s: &[u8]) -> Vec<u8> {
    let mut i = 0;
    while i < s.len() {
        let (c, width) = utf8::decode_rune(&s[i..]);
        let r = mapping(c);
        if r == c && (c != RUNE_ERROR || width != 1) {
            i += width;
            continue;
        }
        let mut b = Vec::with_capacity(s.len() + UTF_MAX);
        b.extend_from_slice(&s[..i]);
        if r >= 0 {
            utf8::append_rune(&mut b, r);
        }
        let mut j = i + width;
        while j < s.len() {
            let (c, width) = utf8::decode_rune(&s[j..]);
            let r = mapping(c);
            if r >= 0 {
                utf8::append_rune(&mut b, r);
            }
            j += width;
        }
        return b;
    }
    s.to_vec()
}

pub fn to_upper(s: &[u8]) -> Vec<u8> {
    if s.is_ascii() {
        return s.iter().map(|&c| c.to_ascii_uppercase()).collect();
    }
    map(unicode::to_upper, s)
}

pub fn to_lower(s: &[u8]) -> Vec<u8> {
    if s.is_ascii() {
        return s.iter().map(|&c| c.to_ascii_lowercase()).collect();
    }
    map(unicode::to_lower, s)
}

pub fn to_title(s: &[u8]) -> Vec<u8> {
    map(unicode::to_title, s)
}

pub fn to_upper_special(c: &unicode::SpecialCase, s: &[u8]) -> Vec<u8> {
    map(|r| c.to_upper(r), s)
}

pub fn to_lower_special(c: &unicode::SpecialCase, s: &[u8]) -> Vec<u8> {
    map(|r| c.to_lower(r), s)
}

pub fn to_title_special(c: &unicode::SpecialCase, s: &[u8]) -> Vec<u8> {
    map(|r| c.to_title(r), s)
}

pub fn index_func(s: &[u8], f: impl Fn(Rune) -> bool) -> Option<usize> {
    index_func_truth(s, &f, true)
}

pub fn last_index_func(s: &[u8], f: impl Fn(Rune) -> bool) -> Option<usize> {
    last_index_func_truth(s, &f, true)
}

fn index_func_truth(s: &[u8], f: &impl Fn(Rune) -> bool, truth: bool) -> Option<usize> {
    let mut i = 0;
    while i < s.len() {
        let (r, width) = utf8::decode_rune(&s[i..]);
        if f(r) == truth {
            return Some(i);
        }
        i += width;
    }
    None
}

fn last_index_func_truth(s: &[u8], f: &impl Fn(Rune) -> bool, truth: bool) -> Option<usize> {
    let mut i = s.len();
    while i > 0 {
        let (r, size) = utf8::decode_last_rune(&s[..i]);
        i -= size;
        if f(r) == truth {
            return Some(i);
        }
    }
    None
}

pub fn trim_left_func(s: &[u8], f: impl Fn(Rune) -> bool) -> &[u8] {
    match index_func_truth(s, &f, false) {
        Some(i) => &s[i..],
        None => &s[s.len()..],
    }
}

pub fn trim_right_func(s: &[u8], f: impl Fn(Rune) -> bool) -> &[u8] {
    match last_index_func_truth(s, &f, false) {
        Some(i) if s[i] >= RUNE_SELF => &s[..i + utf8::decode_rune(&s[i..]).1],
        Some(i) => &s[..i + 1],
        None => &s[..0],
    }
}

pub fn trim_func(s: &[u8], f: impl Fn(Rune) -> bool) -> &[u8] {
    trim_right_func(trim_left_func(s, &f), &f)
}

pub fn trim_space(s: &[u8]) -> &[u8] {
    let mut start = 0;
    while start < s.len() {
        let c = s[start];
        if c >= RUNE_SELF {
            return trim_func(&s[start..], unicode::is_space);
        }
        if !ASCII_SPACE[c as usize] {
            break;
        }
        start += 1;
    }
    let mut stop = s.len();
    while stop > start {
        let c = s[stop - 1];
        if c >= RUNE_SELF {
            return trim_right_func(&s[start..stop], unicode::is_space);
        }
        if !ASCII_SPACE[c as usize] {
            break;
        }
        stop -= 1;
    }
    &s[start..stop]
}

pub fn fields(s: &[u8]) -> Vec<&[u8]> {
    if !s.is_ascii() {
        return fields_func(s, unicode::is_space);
    }
    s.split(|&c| ASCII_SPACE[c as usize])
        .filter(|f| !f.is_empty())
        .collect()
}

pub fn fields_func(s: &[u8], f: impl Fn(Rune) -> bool) -> Vec<&[u8]> {
    let mut spans = Vec::new();
    let mut start: Option<usize> = None;
    let mut end = 0;
    while end < s.len() {
        let (r, width) = utf8::decode_rune(&s[end..]);
        if f(r) {
            if let Some(st) = start.take() {
                spans.push(&s[st..end]);
            }
        } else if start.is_none() {
            start = Some(end);
        }
        end += width;
    }
    if let Some(st) = start {
        spans.push(&s[st..]);
    }
    spans
}

pub fn to_valid_utf8(s: &[u8], replacement: &[u8]) -> Vec<u8> {
    let mut b = Vec::with_capacity(s.len() + replacement.len());
    let mut invalid = false;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if c < RUNE_SELF {
            i += 1;
            invalid = false;
            b.push(c);
            continue;
        }
        let wid = utf8::decode_rune(&s[i..]).1;
        if wid == 1 {
            i += 1;
            if !invalid {
                invalid = true;
                b.extend_from_slice(replacement);
            }
            continue;
        }
        invalid = false;
        b.extend_from_slice(&s[i..i + wid]);
        i += wid;
    }
    b
}

fn ascii_fold_equal(sr: Rune, tr: Rune) -> bool {
    let (sr, tr) = if tr < sr { (tr, sr) } else { (sr, tr) };
    (0x41..=0x5A).contains(&sr) && tr == sr + 0x20
}

pub fn equal_fold(s: &[u8], t: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() && i < t.len() {
        let sr = s[i];
        let tr = t[i];
        if (sr | tr) >= RUNE_SELF {
            return equal_fold_unicode(&s[i..], &t[i..]);
        }
        if tr != sr && !ascii_fold_equal(Rune::from(sr), Rune::from(tr)) {
            return false;
        }
        i += 1;
    }
    s.len() == t.len()
}

fn equal_fold_unicode(s: &[u8], mut t: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let (sr, width) = utf8::decode_rune(&s[i..]);
        i += width;
        if t.is_empty() {
            return false;
        }
        let tr = if t[0] < RUNE_SELF {
            let r = Rune::from(t[0]);
            t = &t[1..];
            r
        } else {
            let (r, size) = utf8::decode_rune(t);
            t = &t[size..];
            r
        };
        if tr == sr {
            continue;
        }
        let (sr, tr) = if tr < sr { (tr, sr) } else { (sr, tr) };
        if tr < Rune::from(RUNE_SELF) {
            if ascii_fold_equal(sr, tr) {
                continue;
            }
            return false;
        }
        let mut r = unicode::simple_fold(sr);
        while r != sr && r < tr {
            r = unicode::simple_fold(r);
        }
        if r != tr {
            return false;
        }
    }
    t.is_empty()
}
