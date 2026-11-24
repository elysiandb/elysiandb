use super::args::ArgsKV;
use super::bytesconv::{
    append_http_date, append_uint, case_insensitive_compare, index_byte, parse_uint,
};
use super::uri::normalize_path;
use crate::gocompat::time::{self, Location, Time};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CookieSameSite {
    #[default]
    Disabled,
    DefaultMode,
    LaxMode,
    StrictMode,
    NoneMode,
}

#[derive(Clone, Debug, Default)]
pub struct Cookie {
    expire: Option<Time>,
    key: Vec<u8>,
    value: Vec<u8>,
    domain: Vec<u8>,
    path: Vec<u8>,
    max_age: i64,
    same_site: CookieSameSite,
    http_only: bool,
    secure: bool,
    partitioned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CookieError {
    NoCookies,
    InvalidMaxAge,
    InvalidExpires,
}

impl std::fmt::Display for CookieError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CookieError::NoCookies => f.write_str("no cookies found"),
            CookieError::InvalidMaxAge => f.write_str("invalid max-age"),
            CookieError::InvalidExpires => f.write_str("invalid expires"),
        }
    }
}

impl std::error::Error for CookieError {}

impl Cookie {
    pub fn new() -> Cookie {
        Cookie::default()
    }

    pub fn reset(&mut self) {
        *self = Cookie::default();
    }

    pub fn http_only(&self) -> bool {
        self.http_only
    }

    pub fn set_http_only(&mut self, http_only: bool) {
        self.http_only = http_only;
    }

    pub fn secure(&self) -> bool {
        self.secure
    }

    pub fn set_secure(&mut self, secure: bool) {
        self.secure = secure;
    }

    pub fn same_site(&self) -> CookieSameSite {
        self.same_site
    }

    pub fn set_same_site(&mut self, mode: CookieSameSite) {
        self.same_site = mode;
        if mode == CookieSameSite::NoneMode {
            self.set_secure(true);
        }
    }

    pub fn partitioned(&self) -> bool {
        self.partitioned
    }

    pub fn set_partitioned(&mut self, partitioned: bool) {
        self.partitioned = partitioned;
        if partitioned {
            self.set_secure(true);
            self.set_path("/");
        }
    }

    pub fn path(&self) -> &[u8] {
        &self.path
    }

    pub fn set_path(&mut self, path: impl AsRef<[u8]>) {
        self.path = normalize_path(path.as_ref());
    }

    pub fn domain(&self) -> &[u8] {
        &self.domain
    }

    pub fn set_domain(&mut self, domain: impl AsRef<[u8]>) {
        self.domain = domain.as_ref().to_vec();
    }

    pub fn max_age(&self) -> i64 {
        self.max_age
    }

    pub fn set_max_age(&mut self, seconds: i64) {
        self.max_age = seconds;
    }

    pub fn expire(&self) -> Option<&Time> {
        self.expire.as_ref()
    }

    pub fn set_expire(&mut self, expire: Option<Time>) {
        self.expire = expire.filter(|t| !t.is_zero());
    }

    pub fn value(&self) -> &[u8] {
        &self.value
    }

    pub fn set_value(&mut self, value: impl AsRef<[u8]>) {
        self.value = value.as_ref().to_vec();
    }

    pub fn key(&self) -> &[u8] {
        &self.key
    }

    pub fn set_key(&mut self, key: impl AsRef<[u8]>) {
        self.key = key.as_ref().to_vec();
    }

    pub fn append_bytes(&self, dst: &mut Vec<u8>) {
        if !self.key.is_empty() {
            dst.extend_from_slice(&self.key);
            dst.push(b'=');
        }
        dst.extend_from_slice(&self.value);

        if self.max_age != 0 {
            dst.extend_from_slice(b"; max-age=");
            append_uint(dst, self.max_age.max(0));
        } else if let Some(expire) = &self.expire {
            dst.extend_from_slice(b"; expires=");
            append_http_date(dst, expire);
        }
        if !self.domain.is_empty() {
            append_cookie_part(dst, b"domain", &self.domain);
        }
        if !self.path.is_empty() {
            append_cookie_part(dst, b"path", &self.path);
        }
        if self.http_only {
            dst.extend_from_slice(b"; HttpOnly");
        }
        if self.secure {
            dst.extend_from_slice(b"; secure");
        }
        match self.same_site {
            CookieSameSite::Disabled => {}
            CookieSameSite::DefaultMode => dst.extend_from_slice(b"; SameSite"),
            CookieSameSite::LaxMode => dst.extend_from_slice(b"; SameSite=Lax"),
            CookieSameSite::StrictMode => dst.extend_from_slice(b"; SameSite=Strict"),
            CookieSameSite::NoneMode => dst.extend_from_slice(b"; SameSite=None"),
        }
        if self.partitioned {
            dst.extend_from_slice(b"; Partitioned");
        }
    }

    pub fn cookie(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        self.append_bytes(&mut dst);
        dst
    }

    pub fn parse(&mut self, src: impl AsRef<[u8]>) -> Result<(), CookieError> {
        self.reset();
        let mut s = src.as_ref();
        let Some((k, v)) = next_cookie(&mut s) else {
            return Err(CookieError::NoCookies);
        };
        self.key = k.to_vec();
        self.value = v.to_vec();

        while let Some((k, v)) = next_cookie(&mut s) {
            if !k.is_empty() {
                match k[0] | 0x20 {
                    b'm' if case_insensitive_compare(b"max-age", k) => {
                        self.max_age = parse_uint(v).ok_or(CookieError::InvalidMaxAge)?;
                    }
                    b'e' if case_insensitive_compare(b"expires", k) => {
                        let text = String::from_utf8_lossy(v);
                        let exptime = time::parse_in_location(
                            time::RFC1123,
                            text.as_bytes(),
                            &Location::utc(),
                        )
                        .or_else(|_| time::parse("Mon, 02-Jan-2006 15:04:05 MST", text.as_bytes()))
                        .map_err(|_| CookieError::InvalidExpires)?;
                        self.expire = Some(exptime).filter(|t| !t.is_zero());
                    }
                    b'd' if case_insensitive_compare(b"domain", k) => {
                        self.domain.extend_from_slice(v);
                    }
                    b'p' if case_insensitive_compare(b"path", k) => {
                        self.path.extend_from_slice(v);
                    }
                    b's' if case_insensitive_compare(b"SameSite", k) => {
                        if let Some(first) = v.first() {
                            match first | 0x20 {
                                b'l' if case_insensitive_compare(b"Lax", v) => {
                                    self.same_site = CookieSameSite::LaxMode;
                                }
                                b's' if case_insensitive_compare(b"Strict", v) => {
                                    self.same_site = CookieSameSite::StrictMode;
                                }
                                b'n' if case_insensitive_compare(b"None", v) => {
                                    self.same_site = CookieSameSite::NoneMode;
                                }
                                _ => {}
                            }
                        }
                    }
                    _ => {}
                }
            } else if let Some(first) = v.first() {
                match first | 0x20 {
                    b'h' if case_insensitive_compare(b"HttpOnly", v) => self.http_only = true,
                    b's' => {
                        if case_insensitive_compare(b"secure", v) {
                            self.secure = true;
                        } else if case_insensitive_compare(b"SameSite", v) {
                            self.same_site = CookieSameSite::DefaultMode;
                        }
                    }
                    b'p' if case_insensitive_compare(b"Partitioned", v) => self.partitioned = true,
                    _ => {}
                }
            }
        }
        Ok(())
    }
}

fn append_cookie_part(dst: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    dst.extend_from_slice(b"; ");
    dst.extend_from_slice(key);
    dst.push(b'=');
    dst.extend_from_slice(value);
}

pub(crate) fn get_cookie_key(src: &[u8]) -> &[u8] {
    let src = match index_byte(src, b'=') {
        Some(n) => &src[..n],
        None => src,
    };
    decode_cookie_arg(src, false)
}

pub(crate) fn append_request_cookie_bytes(dst: &mut Vec<u8>, cookies: &[ArgsKV]) {
    for (i, kv) in cookies.iter().enumerate() {
        if !kv.key.is_empty() {
            dst.extend_from_slice(&kv.key);
            dst.push(b'=');
        }
        dst.extend_from_slice(&kv.value);
        if i + 1 < cookies.len() {
            dst.extend_from_slice(b"; ");
        }
    }
}

pub(crate) fn append_response_cookie_bytes(dst: &mut Vec<u8>, cookies: &[ArgsKV]) {
    for (i, kv) in cookies.iter().enumerate() {
        dst.extend_from_slice(&kv.value);
        if i + 1 < cookies.len() {
            dst.extend_from_slice(b"; ");
        }
    }
}

pub(crate) fn parse_request_cookies(cookies: &mut Vec<ArgsKV>, src: &[u8]) {
    let mut s = src;
    while let Some((k, v)) = next_cookie(&mut s) {
        if !k.is_empty() || !v.is_empty() {
            cookies.push(ArgsKV {
                key: k.to_vec(),
                value: v.to_vec(),
                no_value: false,
            });
        }
    }
}

pub(crate) fn find_request_cookie<'a>(src: &'a [u8], key: &[u8]) -> Option<&'a [u8]> {
    let mut s = src;
    while let Some((k, v)) = next_cookie(&mut s) {
        if (!k.is_empty() || !v.is_empty()) && k == key {
            return Some(v);
        }
    }
    None
}

fn next_cookie<'a>(b: &mut &'a [u8]) -> Option<(&'a [u8], &'a [u8])> {
    let s: &'a [u8] = b;
    if s.is_empty() {
        return None;
    }
    let mut key: &'a [u8] = &[];
    let mut is_key = true;
    let mut k = 0;
    for (i, &c) in s.iter().enumerate() {
        match c {
            b'=' => {
                if is_key {
                    is_key = false;
                    key = decode_cookie_arg(&s[..i], false);
                    k = i + 1;
                }
            }
            b';' => {
                if is_key {
                    key = &[];
                }
                let val = decode_cookie_arg(&s[k..i], true);
                *b = &s[i + 1..];
                return Some((key, val));
            }
            _ => {}
        }
    }
    if is_key {
        key = &[];
    }
    let val = decode_cookie_arg(&s[k..], true);
    *b = &s[s.len()..];
    Some((key, val))
}

fn decode_cookie_arg(src: &[u8], skip_quotes: bool) -> &[u8] {
    let mut src = src;
    while let Some((&b' ', rest)) = src.split_first() {
        src = rest;
    }
    while let Some((&b' ', rest)) = src.split_last() {
        src = rest;
    }
    if skip_quotes && src.len() > 1 && src[0] == b'"' && src[src.len() - 1] == b'"' {
        src = &src[1..src.len() - 1];
    }
    src
}
