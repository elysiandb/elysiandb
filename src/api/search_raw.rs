use crate::docpath;
use crate::gocompat::strings;
use crate::gocompat::utf8;

use super::filter::search_pattern;

const CORE_PREFIX: &[u8] = docpath::CORE_FIELDS_PREFIX.as_bytes();
const RUNE_SELF: u8 = 0x80;
const RUNE_ERROR: u32 = 0xFFFD;

struct RawSearch<'a> {
    b: &'a [u8],
    i: usize,
    pattern: &'a [u8],
    buf: Vec<u8>,
    lower: Vec<u8>,
}

enum Container {
    Object(bool),
    Array(bool),
}

pub fn search_matches_bytes(raw: &[u8], search: &[u8]) -> (bool, bool) {
    search_bytes_lowered(raw, &lowered_search_pattern(search))
}

pub fn lowered_search_pattern(search: &[u8]) -> Vec<u8> {
    strings::to_lower(&search_pattern(search))
}

pub(crate) fn search_bytes_lowered(raw: &[u8], pattern: &[u8]) -> (bool, bool) {
    let mut s = RawSearch {
        b: raw,
        i: 0,
        pattern,
        buf: Vec::new(),
        lower: Vec::new(),
    };

    s.ws();
    if s.i >= s.b.len() || s.b[s.i] != b'{' {
        return (false, false);
    }

    let (matched, ok) = s.document();
    if !ok || matched {
        return (matched, ok);
    }

    s.ws();

    (false, s.i == s.b.len())
}

fn searchable_key(key: &[u8]) -> bool {
    key != b"id" && key != b"@entity" && !key.starts_with(CORE_PREFIX)
}

impl RawSearch<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() {
            match self.b[self.i] {
                b' ' | b'\t' | b'\n' | b'\r' => self.i += 1,
                _ => return,
            }
        }
    }

    fn document(&mut self) -> (bool, bool) {
        let mut stack: Vec<Container> = Vec::new();
        let mut walk = true;

        loop {
            self.ws();
            if self.i >= self.b.len() {
                return (false, false);
            }

            match self.b[self.i] {
                b'{' => {
                    self.i += 1;
                    self.ws();
                    if self.i < self.b.len() && self.b[self.i] == b'}' {
                        self.i += 1;
                    } else {
                        stack.push(Container::Object(walk));
                        match self.key(walk) {
                            Some(searchable) => {
                                walk = searchable;
                                continue;
                            }
                            None => return (false, false),
                        }
                    }
                }
                b'[' => {
                    self.i += 1;
                    self.ws();
                    if self.i < self.b.len() && self.b[self.i] == b']' {
                        self.i += 1;
                    } else {
                        stack.push(Container::Array(walk));
                        continue;
                    }
                }
                b'"' => {
                    let Some(matched) = self.string_matches(walk) else {
                        return (false, false);
                    };
                    if matched {
                        return (true, true);
                    }
                }
                _ => {
                    if !self.literal() {
                        return (false, false);
                    }
                }
            }

            loop {
                let Some(top) = stack.last() else {
                    return (false, true);
                };

                self.ws();
                if self.i >= self.b.len() {
                    return (false, false);
                }

                match (top, self.b[self.i]) {
                    (Container::Object(parent), b',') => {
                        let parent = *parent;
                        self.i += 1;
                        match self.key(parent) {
                            Some(searchable) => {
                                walk = searchable;
                                break;
                            }
                            None => return (false, false),
                        }
                    }
                    (Container::Array(parent), b',') => {
                        walk = *parent;
                        self.i += 1;
                        break;
                    }
                    (Container::Object(_), b'}') | (Container::Array(_), b']') => {
                        self.i += 1;
                        stack.pop();
                    }
                    _ => return (false, false),
                }
            }
        }
    }

    fn key(&mut self, walk: bool) -> Option<bool> {
        self.ws();
        if self.i >= self.b.len() || self.b[self.i] != b'"' {
            return None;
        }

        let searchable = {
            let key = self.str()?;
            walk && searchable_key(key)
        };

        self.ws();
        if self.i >= self.b.len() || self.b[self.i] != b':' {
            return None;
        }
        self.i += 1;

        Some(searchable)
    }

    fn string_matches(&mut self, walk: bool) -> Option<bool> {
        let start = self.i;
        let plain = self.scan_string()?;
        let body_start = start + 1;
        let body_end = self.i - 1;
        if plain {
            let body = &self.b[body_start..body_end];
            return Some(walk && glob(self.pattern, &mut self.lower, body));
        }

        let body = &self.b[body_start..body_end];
        let mut out = std::mem::take(&mut self.buf);
        let ok = unquote(body, &mut out);
        let matched = ok && walk && glob(self.pattern, &mut self.lower, &out);
        self.buf = out;
        if !ok {
            return None;
        }

        Some(matched)
    }

    fn literal(&mut self) -> bool {
        let start = self.i;
        while self.i < self.b.len() {
            match self.b[self.i] {
                b',' | b'}' | b']' | b' ' | b'\t' | b'\n' | b'\r' => return self.i > start,
                _ => {}
            }
            self.i += 1;
        }

        self.i > start
    }

    fn scan_string(&mut self) -> Option<bool> {
        self.i += 1;
        let mut plain = true;
        while self.i < self.b.len() {
            let c = self.b[self.i];
            if c == b'"' {
                break;
            }
            if c == b'\\' {
                plain = false;
                self.i += 2;
                continue;
            }
            if !(0x20..RUNE_SELF).contains(&c) {
                plain = false;
            }
            self.i += 1;
        }

        if self.i >= self.b.len() {
            return None;
        }

        self.i += 1;

        Some(plain)
    }

    fn str(&mut self) -> Option<&[u8]> {
        let start = self.i;
        let plain = self.scan_string()?;
        let body = &self.b[start + 1..self.i - 1];
        if plain {
            return Some(body);
        }

        let mut out = std::mem::take(&mut self.buf);
        if !unquote(body, &mut out) {
            self.buf = out;
            return None;
        }
        self.buf = out;

        Some(&self.buf)
    }
}

fn glob(pattern: &[u8], lower: &mut Vec<u8>, s: &[u8]) -> bool {
    if s.iter().any(|&c| c >= RUNE_SELF) {
        return docpath::match_glob_lower(pattern, &strings::to_lower(s));
    }

    lower.clear();
    lower.extend(s.iter().map(u8::to_ascii_lowercase));

    docpath::match_glob_lower(pattern, lower)
}

fn hex4(b: &[u8], at: usize) -> Option<u32> {
    if at + 4 > b.len() {
        return None;
    }

    let mut r: u32 = 0;
    for &c in &b[at..at + 4] {
        let d = match c {
            b'0'..=b'9' => c - b'0',
            b'a'..=b'f' => c - b'a' + 10,
            b'A'..=b'F' => c - b'A' + 10,
            _ => return None,
        };
        r = r * 16 + u32::from(d);
    }

    Some(r)
}

fn is_surrogate(r: u32) -> bool {
    (0xD800..0xE000).contains(&r)
}

fn decode_surrogates(r1: u32, r2: u32) -> u32 {
    if (0xD800..0xDC00).contains(&r1) && (0xDC00..0xE000).contains(&r2) {
        return (((r1 - 0xD800) << 10) | (r2 - 0xDC00)) + 0x10000;
    }

    RUNE_ERROR
}

fn append_rune(out: &mut Vec<u8>, r: u32) {
    let c = char::from_u32(r).unwrap_or('\u{FFFD}');
    let mut tmp = [0u8; 4];
    out.extend_from_slice(c.encode_utf8(&mut tmp).as_bytes());
}

fn unquote(body: &[u8], out: &mut Vec<u8>) -> bool {
    out.clear();
    let mut r = 0;
    while r < body.len() {
        let c = body[r];
        if c == b'\\' {
            if r + 1 >= body.len() {
                return false;
            }
            match body[r + 1] {
                b'"' | b'\\' | b'/' => {
                    out.push(body[r + 1]);
                    r += 2;
                }
                b'b' => {
                    out.push(0x08);
                    r += 2;
                }
                b'f' => {
                    out.push(0x0c);
                    r += 2;
                }
                b'n' => {
                    out.push(b'\n');
                    r += 2;
                }
                b'r' => {
                    out.push(b'\r');
                    r += 2;
                }
                b't' => {
                    out.push(b'\t');
                    r += 2;
                }
                b'u' => {
                    let Some(mut rr) = hex4(body, r + 2) else {
                        return false;
                    };
                    r += 6;
                    if is_surrogate(rr) {
                        let mut second = None;
                        if r + 6 <= body.len() && body[r] == b'\\' && body[r + 1] == b'u' {
                            second = hex4(body, r + 2);
                        }
                        if let Some(rr1) = second {
                            let dec = decode_surrogates(rr, rr1);
                            if dec != RUNE_ERROR {
                                r += 6;
                                append_rune(out, dec);
                                continue;
                            }
                        }
                        rr = RUNE_ERROR;
                    }
                    append_rune(out, rr);
                }
                _ => return false,
            }
        } else if c < 0x20 {
            return false;
        } else if c < RUNE_SELF {
            out.push(c);
            r += 1;
        } else {
            let (rr, size) = utf8::decode_rune(&body[r..]);
            r += size;
            append_rune(out, rr as u32);
        }
    }

    true
}
