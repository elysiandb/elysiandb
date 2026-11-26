use std::cell::OnceCell;

use super::args::{ArgsKV, append_arg, del_all_args, peek_all_args, peek_arg, set_arg};
use super::bytesconv::{
    append_uint, case_insensitive_compare, index_byte, parse_uint_buf, to_lower, to_upper,
    valid_header_field_byte, valid_header_value_byte, valid_method_byte,
};
use super::cookie::{
    Cookie, append_request_cookie_bytes, append_response_cookie_bytes, find_request_cookie,
    get_cookie_key, parse_request_cookies,
};
use super::date::server_date;
use super::headerscanner::{HeaderScanner, ScanError};
use super::status::format_status_line;

pub const HEADER_AUTHORIZATION: &str = "Authorization";
pub const HEADER_CONTENT_TYPE: &str = "Content-Type";
pub const HEADER_CONTENT_LENGTH: &str = "Content-Length";
pub const HEADER_CONTENT_ENCODING: &str = "Content-Encoding";
pub const HEADER_CONNECTION: &str = "Connection";
pub const HEADER_COOKIE: &str = "Cookie";
pub const HEADER_SET_COOKIE: &str = "Set-Cookie";
pub const HEADER_HOST: &str = "Host";
pub const HEADER_USER_AGENT: &str = "User-Agent";
pub const HEADER_SERVER: &str = "Server";
pub const HEADER_DATE: &str = "Date";
pub const HEADER_TRANSFER_ENCODING: &str = "Transfer-Encoding";
pub const HEADER_TRAILER: &str = "Trailer";
pub const HEADER_LOCATION: &str = "Location";
pub const HEADER_EXPECT: &str = "Expect";
pub const HEADER_REFERER: &str = "Referer";

pub const METHOD_GET: &str = "GET";
pub const METHOD_HEAD: &str = "HEAD";
pub const METHOD_POST: &str = "POST";
pub const METHOD_PUT: &str = "PUT";
pub const METHOD_PATCH: &str = "PATCH";
pub const METHOD_DELETE: &str = "DELETE";
pub const METHOD_CONNECT: &str = "CONNECT";
pub const METHOD_OPTIONS: &str = "OPTIONS";
pub const METHOD_TRACE: &str = "TRACE";

pub(crate) const DEFAULT_CONTENT_TYPE: &[u8] = b"text/plain; charset=utf-8";
const STR_HTTP11: &[u8] = b"HTTP/1.1";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ParseError {
    NeedMore,
    RequestLine,
    Version,
    HeaderLine,
    DuplicateContentLength,
    InvalidContentLength,
    BadTrailer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BadTrailer;

pub(crate) fn normalize_header_key(b: &mut [u8], disable_normalizing: bool) {
    if disable_normalizing || b.is_empty() {
        return;
    }
    if !b.iter().all(|&c| valid_header_field_byte(c)) {
        return;
    }
    let mut upper = true;
    for c in b.iter_mut() {
        *c = if upper { to_upper(*c) } else { to_lower(*c) };
        upper = *c == b'-';
    }
}

fn header_key_bytes(key: &[u8], disable_normalizing: bool) -> Vec<u8> {
    let mut k = key.to_vec();
    let disable = disable_normalizing || index_byte(&k, b' ').is_some();
    normalize_header_key(&mut k, disable);
    k
}

fn remove_new_lines(raw: &mut [u8]) {
    let found_r = index_byte(raw, b'\r');
    let found_n = index_byte(raw, b'\n');
    let start = match (found_n, found_r) {
        (Some(n), Some(r)) => {
            if r > n {
                n
            } else {
                r
            }
        }
        (Some(n), None) => {
            let _ = n;
            0
        }
        (None, Some(r)) => r,
        (None, None) => return,
    };
    for c in raw[start..].iter_mut() {
        if *c == b'\r' || *c == b'\n' {
            *c = b' ';
        }
    }
}

fn parse_content_length(b: &[u8]) -> Result<i64, ()> {
    let (v, n, ok) = parse_uint_buf(b);
    if !ok || n != b.len() {
        return Err(());
    }
    Ok(v)
}

fn has_header_value(s: &[u8], value: &[u8]) -> bool {
    s.split(|&c| c == b',')
        .map(strip_space)
        .any(|v| case_insensitive_compare(v, value))
        && !s.is_empty()
}

fn strip_space(mut b: &[u8]) -> &[u8] {
    while let Some((&b' ', rest)) = b.split_first() {
        b = rest;
    }
    while let Some((&b' ', rest)) = b.split_last() {
        b = rest;
    }
    b
}

fn next_line(b: &[u8]) -> Result<(&[u8], &[u8]), ParseError> {
    let Some(n_next) = index_byte(b, b'\n') else {
        return Err(ParseError::NeedMore);
    };
    let mut n = n_next;
    if n > 0 && b[n - 1] == b'\r' {
        n -= 1;
    }
    Ok((&b[..n], &b[n_next + 1..]))
}

fn read_raw_headers(dst: &mut Vec<u8>, buf: &[u8]) -> Result<usize, ParseError> {
    let Some(n) = index_byte(buf, b'\n') else {
        dst.clear();
        return Err(ParseError::NeedMore);
    };
    if (n == 1 && buf[0] == b'\r') || n == 0 {
        return Ok(n + 1);
    }
    let mut n = n + 1;
    let mut m = n;
    let mut b = buf;
    loop {
        b = &b[m..];
        let Some(idx) = index_byte(b, b'\n') else {
            return Err(ParseError::NeedMore);
        };
        m = idx + 1;
        n += m;
        if (m == 2 && b[0] == b'\r') || m == 1 {
            dst.extend_from_slice(&buf[..n]);
            return Ok(n);
        }
    }
}

fn is_bad_trailer(key: &[u8]) -> bool {
    if key.is_empty() {
        return true;
    }
    let cic = case_insensitive_compare;
    match key[0] | 0x20 {
        b'a' => cic(key, b"Authorization"),
        b'c' => {
            if key.len() >= HEADER_CONTENT_TYPE.len() && cic(&key[..8], &b"Content-Type"[..8]) {
                return cic(&key[8..], &b"Content-Encoding"[8..])
                    || cic(&key[8..], &b"Content-Length"[8..])
                    || cic(&key[8..], &b"Content-Type"[8..])
                    || cic(&key[8..], &b"Content-Range"[8..]);
            }
            cic(key, b"Connection") || cic(key, b"Cookie")
        }
        b'e' => cic(key, b"Expect"),
        b'h' => cic(key, b"Host"),
        b'k' => cic(key, b"Keep-Alive"),
        b'l' => cic(key, b"Location"),
        b'm' => cic(key, b"Max-Forwards"),
        b'p' => {
            if key.len() >= "Proxy-Connection".len() && cic(&key[..6], &b"Proxy-Connection"[..6]) {
                return cic(&key[6..], &b"Proxy-Connection"[6..])
                    || cic(&key[6..], &b"Proxy-Authenticate"[6..])
                    || cic(&key[6..], &b"Proxy-Authorization"[6..]);
            }
            false
        }
        b'r' => cic(key, b"Range"),
        b's' => cic(key, b"Set-Cookie"),
        b't' => cic(key, b"TE") || cic(key, b"Trailer") || cic(key, b"Transfer-Encoding"),
        b'w' => cic(key, b"WWW-Authenticate"),
        b'x' => {
            (key.len() >= 11 && cic(&key[..11], b"x-forwarded"))
                || (key.len() >= 9 && cic(&key[..9], b"x-real-ip"))
        }
        _ => false,
    }
}

fn add_trailer_bytes(
    trailer: &mut Vec<Vec<u8>>,
    value: &[u8],
    disable_normalizing: bool,
) -> Result<(), BadTrailer> {
    let mut err = Ok(());
    let mut rest = value;
    let mut i: isize = -1;
    while i + 1 < rest.len() as isize {
        rest = &rest[(i + 1) as usize..];
        let end = index_byte(rest, b',').unwrap_or(rest.len());
        i = end as isize;
        let key = strip_space(&rest[..end]);
        if is_bad_trailer(key) {
            err = Err(BadTrailer);
            continue;
        }
        trailer.push(header_key_bytes(key, disable_normalizing));
    }
    err
}

fn append_trailer_bytes(dst: &mut Vec<u8>, trailer: &[Vec<u8>], sep: &[u8]) {
    for (i, t) in trailer.iter().enumerate() {
        dst.extend_from_slice(t);
        if i + 1 < trailer.len() {
            dst.extend_from_slice(sep);
        }
    }
}

fn append_header_line(dst: &mut Vec<u8>, key: &[u8], value: &[u8]) {
    dst.extend_from_slice(key);
    dst.extend_from_slice(b": ");
    dst.extend_from_slice(value);
    dst.extend_from_slice(b"\r\n");
}

#[derive(Clone, Debug, Default)]
struct Common {
    h: Vec<ArgsKV>,
    cookies: Vec<ArgsKV>,
    content_length_bytes: Vec<u8>,
    content_type: Vec<u8>,
    protocol: Vec<u8>,
    trailer: Vec<Vec<u8>>,
    content_length: i64,
    disable_normalizing: bool,
    no_http11: bool,
    connection_close: bool,
    no_default_content_type: bool,
    cookie_bytes: OnceCell<Vec<u8>>,
    trailer_bytes: OnceCell<Vec<u8>>,
}

impl Common {
    fn set_non_special(&mut self, key: &[u8], value: &[u8]) {
        set_arg(&mut self.h, key, value, false);
    }

    fn reset_connection_close(&mut self) {
        if self.connection_close {
            self.connection_close = false;
            del_all_args(&mut self.h, HEADER_CONNECTION.as_bytes());
        }
    }

    fn set_trailer_bytes(&mut self, trailer: &[u8]) -> Result<(), BadTrailer> {
        self.trailer.clear();
        self.trailer_bytes = OnceCell::new();
        add_trailer_bytes(&mut self.trailer, trailer, self.disable_normalizing)
    }

    fn trailer_value(&self) -> &[u8] {
        self.trailer_bytes.get_or_init(|| {
            let mut v = Vec::new();
            append_trailer_bytes(&mut v, &self.trailer, b", ");
            v
        })
    }

    fn touch_cookies(&mut self) {
        self.cookie_bytes = OnceCell::new();
    }
}

#[derive(Clone, Debug, Default)]
pub struct RequestHeader {
    c: Common,
    method: Vec<u8>,
    request_uri: Vec<u8>,
    host: Vec<u8>,
    user_agent: Vec<u8>,
    raw_headers: Vec<u8>,
    cookies_collected: bool,
}

impl RequestHeader {
    pub fn new() -> RequestHeader {
        RequestHeader::default()
    }

    pub fn reset(&mut self) {
        *self = RequestHeader::default();
    }

    pub fn disable_normalizing(&mut self) {
        self.c.disable_normalizing = true;
    }

    pub fn enable_normalizing(&mut self) {
        self.c.disable_normalizing = false;
    }

    pub fn connection_close(&self) -> bool {
        self.c.connection_close
    }

    pub fn set_connection_close(&mut self) {
        self.c.connection_close = true;
    }

    pub fn reset_connection_close(&mut self) {
        self.c.reset_connection_close();
    }

    pub fn content_length(&self) -> i64 {
        self.c.content_length
    }

    pub fn set_content_length(&mut self, content_length: i64) {
        self.c.content_length = content_length;
        self.c.content_length_bytes.clear();
        if content_length >= 0 {
            append_uint(&mut self.c.content_length_bytes, content_length);
            del_all_args(&mut self.c.h, HEADER_TRANSFER_ENCODING.as_bytes());
        } else {
            set_arg(
                &mut self.c.h,
                HEADER_TRANSFER_ENCODING.as_bytes(),
                b"chunked",
                false,
            );
        }
    }

    pub fn content_type(&self) -> &[u8] {
        &self.c.content_type
    }

    pub fn set_content_type(&mut self, content_type: impl AsRef<[u8]>) {
        self.c.content_type = content_type.as_ref().to_vec();
    }

    pub fn content_encoding(&self) -> &[u8] {
        peek_arg(&self.c.h, HEADER_CONTENT_ENCODING.as_bytes()).unwrap_or(&[])
    }

    pub fn set_multipart_form_boundary(&mut self, boundary: impl AsRef<[u8]>) {
        let mut b = b"multipart/form-data; boundary=".to_vec();
        b.extend_from_slice(boundary.as_ref());
        self.set_content_type(b);
    }

    pub fn multipart_form_boundary(&self) -> &[u8] {
        let b = self.content_type();
        let Some(mut b) = b.strip_prefix(b"multipart/form-data".as_slice()) else {
            return &[];
        };
        if b.first() != Some(&b';') {
            return &[];
        }
        while !b.is_empty() {
            let mut n = 1;
            while b.len() > n && b[n] == b' ' {
                n += 1;
            }
            b = &b[n..];
            let Some(rest) = b.strip_prefix(b"boundary".as_slice()) else {
                match index_byte(b, b';') {
                    None => return &[],
                    Some(i) => {
                        b = &b[i..];
                        continue;
                    }
                }
            };
            let mut b = rest;
            if b.first() != Some(&b'=') {
                return &[];
            }
            b = &b[1..];
            if let Some(i) = index_byte(b, b';') {
                b = &b[..i];
            }
            if b.len() > 1 && b[0] == b'"' && b[b.len() - 1] == b'"' {
                b = &b[1..b.len() - 1];
            }
            return b;
        }
        &[]
    }

    pub fn host(&self) -> &[u8] {
        &self.host
    }

    pub fn set_host(&mut self, host: impl AsRef<[u8]>) {
        self.host = host.as_ref().to_vec();
    }

    pub fn user_agent(&self) -> &[u8] {
        &self.user_agent
    }

    pub fn set_user_agent(&mut self, user_agent: impl AsRef<[u8]>) {
        self.user_agent = user_agent.as_ref().to_vec();
    }

    pub fn referer(&self) -> &[u8] {
        self.peek(HEADER_REFERER)
    }

    pub fn method(&self) -> &[u8] {
        if self.method.is_empty() {
            METHOD_GET.as_bytes()
        } else {
            &self.method
        }
    }

    pub fn set_method(&mut self, method: impl AsRef<[u8]>) {
        self.method = method.as_ref().to_vec();
    }

    pub fn protocol(&self) -> &[u8] {
        if self.c.protocol.is_empty() {
            STR_HTTP11
        } else {
            &self.c.protocol
        }
    }

    pub fn set_protocol(&mut self, protocol: impl AsRef<[u8]>) {
        self.c.protocol = protocol.as_ref().to_vec();
        self.c.no_http11 = self.c.protocol != STR_HTTP11;
    }

    pub fn request_uri(&self) -> &[u8] {
        if self.request_uri.is_empty() {
            b"/"
        } else {
            &self.request_uri
        }
    }

    pub fn set_request_uri(&mut self, request_uri: impl AsRef<[u8]>) {
        self.request_uri = request_uri.as_ref().to_vec();
    }

    pub fn is_get(&self) -> bool {
        self.method() == METHOD_GET.as_bytes()
    }

    pub fn is_post(&self) -> bool {
        self.method() == METHOD_POST.as_bytes()
    }

    pub fn is_put(&self) -> bool {
        self.method() == METHOD_PUT.as_bytes()
    }

    pub fn is_head(&self) -> bool {
        self.method() == METHOD_HEAD.as_bytes()
    }

    pub fn is_delete(&self) -> bool {
        self.method() == METHOD_DELETE.as_bytes()
    }

    pub fn is_connect(&self) -> bool {
        self.method() == METHOD_CONNECT.as_bytes()
    }

    pub fn is_options(&self) -> bool {
        self.method() == METHOD_OPTIONS.as_bytes()
    }

    pub fn is_trace(&self) -> bool {
        self.method() == METHOD_TRACE.as_bytes()
    }

    pub fn is_patch(&self) -> bool {
        self.method() == METHOD_PATCH.as_bytes()
    }

    pub fn is_http11(&self) -> bool {
        !self.c.no_http11
    }

    pub fn raw_headers(&self) -> &[u8] {
        &self.raw_headers
    }

    pub fn trailers(&self) -> &[Vec<u8>] {
        &self.c.trailer
    }

    fn set_special_header(&mut self, key: &[u8], value: &[u8]) -> bool {
        if key.is_empty() {
            return false;
        }
        match key[0] | 0x20 {
            b'c' => {
                if case_insensitive_compare(b"Content-Type", key) {
                    self.set_content_type(value);
                    return true;
                }
                if case_insensitive_compare(b"Content-Length", key) {
                    if let Ok(cl) = parse_content_length(value) {
                        self.c.content_length = cl;
                        self.c.content_length_bytes = value.to_vec();
                    }
                    return true;
                }
                if case_insensitive_compare(b"Connection", key) {
                    if value == b"close" {
                        self.set_connection_close();
                    } else {
                        self.c.reset_connection_close();
                        self.c.set_non_special(key, value);
                    }
                    return true;
                }
                if case_insensitive_compare(b"Cookie", key) {
                    self.collect_cookies();
                    parse_request_cookies(&mut self.c.cookies, value);
                    self.c.touch_cookies();
                    return true;
                }
            }
            b't' => {
                if case_insensitive_compare(b"Transfer-Encoding", key) {
                    return true;
                }
                if case_insensitive_compare(b"Trailer", key) {
                    let _ = self.c.set_trailer_bytes(value);
                    return true;
                }
            }
            b'h' => {
                if case_insensitive_compare(b"Host", key) {
                    self.set_host(value);
                    return true;
                }
            }
            b'u' => {
                if case_insensitive_compare(b"User-Agent", key) {
                    self.set_user_agent(value);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    pub fn add(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        let (key, value) = (key.as_ref(), value.as_ref());
        if self.set_special_header(key, value) {
            return;
        }
        let k = header_key_bytes(key, self.c.disable_normalizing);
        append_arg(&mut self.c.h, &k, value, false);
    }

    pub fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        let mut v = value.as_ref().to_vec();
        remove_new_lines(&mut v);
        self.set_canonical(&k, &v);
    }

    pub fn set_canonical(&mut self, key: &[u8], value: &[u8]) {
        if self.set_special_header(key, value) {
            return;
        }
        self.c.set_non_special(key, value);
    }

    pub fn del(&mut self, key: impl AsRef<[u8]>) {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        match k.as_slice() {
            b"Host" => self.host.clear(),
            b"Content-Type" => self.c.content_type.clear(),
            b"User-Agent" => self.user_agent.clear(),
            b"Cookie" => {
                self.c.cookies.clear();
                self.c.touch_cookies();
            }
            b"Content-Length" => {
                self.c.content_length = 0;
                self.c.content_length_bytes.clear();
            }
            b"Connection" => self.c.connection_close = false,
            b"Trailer" => {
                self.c.trailer.clear();
                self.c.trailer_bytes = OnceCell::new();
            }
            _ => {}
        }
        del_all_args(&mut self.c.h, &k);
    }

    pub fn peek(&self, key: impl AsRef<[u8]>) -> &[u8] {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        self.peek_canonical(&k)
    }

    fn peek_canonical(&self, key: &[u8]) -> &[u8] {
        match key {
            b"Host" => self.host(),
            b"Content-Type" => self.content_type(),
            b"User-Agent" => self.user_agent(),
            b"Connection" => {
                if self.connection_close() {
                    b"close"
                } else {
                    peek_arg(&self.c.h, key).unwrap_or(&[])
                }
            }
            b"Content-Length" => &self.c.content_length_bytes,
            b"Cookie" => {
                if self.cookies_collected {
                    self.collected_cookie_bytes()
                } else {
                    peek_arg(&self.c.h, key).unwrap_or(&[])
                }
            }
            b"Trailer" => self.c.trailer_value(),
            _ => peek_arg(&self.c.h, key).unwrap_or(&[]),
        }
    }

    fn collected_cookie_bytes(&self) -> &[u8] {
        self.c.cookie_bytes.get_or_init(|| {
            let mut v = Vec::new();
            append_request_cookie_bytes(&mut v, &self.c.cookies);
            v
        })
    }

    pub fn peek_all(&self, key: impl AsRef<[u8]>) -> Vec<&[u8]> {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        let mut out = Vec::new();
        match k.as_slice() {
            b"Host" => {
                if !self.host.is_empty() {
                    out.push(self.host());
                }
            }
            b"Content-Type" => {
                if !self.c.content_type.is_empty() {
                    out.push(self.content_type());
                }
            }
            b"User-Agent" => {
                if !self.user_agent.is_empty() {
                    out.push(self.user_agent());
                }
            }
            b"Connection" => {
                if self.connection_close() {
                    out.push(b"close".as_slice());
                } else {
                    out = peek_all_args(&self.c.h, &k);
                }
            }
            b"Content-Length" => out.push(self.c.content_length_bytes.as_slice()),
            b"Cookie" => {
                if self.cookies_collected {
                    out.push(self.collected_cookie_bytes());
                } else {
                    out = peek_all_args(&self.c.h, &k);
                }
            }
            b"Trailer" => out.push(self.c.trailer_value()),
            _ => out = peek_all_args(&self.c.h, &k),
        }
        out
    }

    pub fn has(&self, key: impl AsRef<[u8]>) -> bool {
        !self.peek_all(key).is_empty()
    }

    fn collect_cookies(&mut self) {
        if self.cookies_collected {
            return;
        }
        let mut i = 0;
        while i < self.c.h.len() {
            if case_insensitive_compare(&self.c.h[i].key, b"Cookie") {
                let kv = self.c.h.remove(i);
                parse_request_cookies(&mut self.c.cookies, &kv.value);
            } else {
                i += 1;
            }
        }
        self.cookies_collected = true;
        self.c.touch_cookies();
    }

    pub fn cookie(&self, key: impl AsRef<[u8]>) -> &[u8] {
        let key = key.as_ref();
        if self.cookies_collected {
            return peek_arg(&self.c.cookies, key).unwrap_or(&[]);
        }
        for kv in &self.c.h {
            if case_insensitive_compare(&kv.key, b"Cookie")
                && let Some(v) = find_request_cookie(&kv.value, key)
            {
                return v;
            }
        }
        &[]
    }

    pub fn cookies(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut list = Vec::new();
        if self.cookies_collected {
            list.extend(self.c.cookies.iter().cloned());
        } else {
            for kv in &self.c.h {
                if case_insensitive_compare(&kv.key, b"Cookie") {
                    parse_request_cookies(&mut list, &kv.value);
                }
            }
        }
        list.into_iter().map(|kv| (kv.key, kv.value)).collect()
    }

    pub fn set_cookie(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        self.collect_cookies();
        set_arg(&mut self.c.cookies, key.as_ref(), value.as_ref(), false);
        self.c.touch_cookies();
    }

    pub fn del_cookie(&mut self, key: impl AsRef<[u8]>) {
        self.collect_cookies();
        del_all_args(&mut self.c.cookies, key.as_ref());
        self.c.touch_cookies();
    }

    pub fn del_all_cookies(&mut self) {
        self.collect_cookies();
        self.c.cookies.clear();
        self.c.touch_cookies();
    }

    pub fn len(&self) -> usize {
        self.all().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn all(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        if !self.c.content_length_bytes.is_empty() {
            out.push((
                HEADER_CONTENT_LENGTH.as_bytes().to_vec(),
                self.c.content_length_bytes.clone(),
            ));
        }
        if !self.host.is_empty() {
            out.push((HEADER_HOST.as_bytes().to_vec(), self.host.clone()));
        }
        if !self.c.content_type.is_empty() {
            out.push((
                HEADER_CONTENT_TYPE.as_bytes().to_vec(),
                self.c.content_type.clone(),
            ));
        }
        if !self.user_agent.is_empty() {
            out.push((
                HEADER_USER_AGENT.as_bytes().to_vec(),
                self.user_agent.clone(),
            ));
        }
        if !self.c.trailer.is_empty() {
            out.push((
                HEADER_TRAILER.as_bytes().to_vec(),
                self.c.trailer_value().to_vec(),
            ));
        }
        for kv in &self.c.h {
            out.push((kv.key.clone(), kv.value.clone()));
        }
        if !self.c.cookies.is_empty() {
            out.push((
                HEADER_COOKIE.as_bytes().to_vec(),
                self.collected_cookie_bytes().to_vec(),
            ));
        }
        if self.connection_close() {
            out.push((HEADER_CONNECTION.as_bytes().to_vec(), b"close".to_vec()));
        }
        out
    }

    pub fn header(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        self.append_bytes(&mut dst);
        dst
    }

    pub fn append_bytes(&self, dst: &mut Vec<u8>) {
        dst.extend_from_slice(self.method());
        dst.push(b' ');
        dst.extend_from_slice(self.request_uri());
        dst.push(b' ');
        dst.extend_from_slice(self.protocol());
        dst.extend_from_slice(b"\r\n");
        for (k, v) in self.all() {
            append_header_line(dst, &k, &v);
        }
        dst.extend_from_slice(b"\r\n");
    }

    pub(crate) fn may_continue(&self) -> bool {
        peek_arg(&self.c.h, HEADER_EXPECT.as_bytes()) == Some(b"100-continue".as_slice())
    }

    pub(crate) fn ignore_body(&self) -> bool {
        self.is_get() || self.is_head()
    }

    pub(crate) fn parse(&mut self, buf: &[u8]) -> Result<usize, ParseError> {
        let m = self.parse_first_line(buf)?;
        self.raw_headers.clear();
        let mut raw = Vec::new();
        read_raw_headers(&mut raw, &buf[m..])?;
        self.raw_headers = raw;
        let n = self.parse_headers(&buf[m..])?;
        Ok(m + n)
    }

    fn parse_first_line(&mut self, buf: &[u8]) -> Result<usize, ParseError> {
        let mut b_next = buf;
        let mut b: &[u8] = &[];
        while b.is_empty() {
            let (line, next) = next_line(b_next)?;
            b = line;
            b_next = next;
        }

        let n = match index_byte(b, b' ') {
            Some(n) if n > 0 => n,
            _ => return Err(ParseError::RequestLine),
        };
        self.method = b[..n].to_vec();
        if !self.method.iter().all(|&c| valid_method_byte(c)) {
            return Err(ParseError::RequestLine);
        }
        let b = &b[n + 1..];

        let n = match b.iter().rposition(|&c| c == b' ') {
            None | Some(0) => return Err(ParseError::RequestLine),
            Some(n) => n,
        };
        let proto = &b[n + 1..];
        if proto.len() != STR_HTTP11.len() || !proto.starts_with(&STR_HTTP11[..5]) {
            return Err(ParseError::Version);
        }
        if !proto[5].is_ascii_digit() || !proto[7].is_ascii_digit() {
            return Err(ParseError::Version);
        }
        self.c.no_http11 = proto != STR_HTTP11;
        self.c.protocol = proto.to_vec();
        self.request_uri = b[..n].to_vec();

        Ok(buf.len() - b_next.len())
    }

    fn parse_headers(&mut self, buf: &[u8]) -> Result<usize, ParseError> {
        self.c.content_length = -2;
        let mut content_length_seen = false;
        let mut s = HeaderScanner::new(buf);
        while let Some((mut key, value)) = s.next() {
            if key.is_empty() {
                self.c.connection_close = true;
                return Err(ParseError::HeaderLine);
            }
            let mut disable = self.c.disable_normalizing;
            for &ch in &key {
                if !valid_header_field_byte(ch) {
                    if ch == b' ' {
                        disable = true;
                        continue;
                    }
                    self.c.connection_close = true;
                    return Err(ParseError::HeaderLine);
                }
            }
            normalize_header_key(&mut key, disable);

            if !value.iter().all(|&ch| valid_header_value_byte(ch)) {
                self.c.connection_close = true;
                return Err(ParseError::HeaderLine);
            }

            match key[0] | 0x20 {
                b'h' if case_insensitive_compare(&key, b"Host") => {
                    self.host = value;
                    continue;
                }
                b'u' if case_insensitive_compare(&key, b"User-Agent") => {
                    self.user_agent = value;
                    continue;
                }
                b'c' => {
                    if case_insensitive_compare(&key, b"Content-Type") {
                        self.c.content_type = value;
                        continue;
                    }
                    if case_insensitive_compare(&key, b"Content-Length") {
                        if content_length_seen {
                            self.c.connection_close = true;
                            return Err(ParseError::DuplicateContentLength);
                        }
                        content_length_seen = true;
                        if self.c.content_length != -1 {
                            match parse_content_length(&value) {
                                Ok(cl) => {
                                    self.c.content_length = cl;
                                    self.c.content_length_bytes = value;
                                }
                                Err(()) => {
                                    self.c.content_length = -2;
                                    self.c.connection_close = true;
                                    return Err(ParseError::InvalidContentLength);
                                }
                            }
                        }
                        continue;
                    }
                    if case_insensitive_compare(&key, b"Connection") {
                        if value == b"close" {
                            self.c.connection_close = true;
                        } else {
                            self.c.connection_close = false;
                            append_arg(&mut self.c.h, &key, &value, false);
                        }
                        continue;
                    }
                }
                b't' => {
                    if case_insensitive_compare(&key, b"Transfer-Encoding") {
                        let is_identity = case_insensitive_compare(&value, b"identity");
                        let is_chunked = case_insensitive_compare(&value, b"chunked");
                        if !is_identity && !is_chunked {
                            self.c.connection_close = true;
                            return Err(ParseError::HeaderLine);
                        }
                        if is_chunked {
                            self.c.content_length = -1;
                            set_arg(
                                &mut self.c.h,
                                HEADER_TRANSFER_ENCODING.as_bytes(),
                                b"chunked",
                                false,
                            );
                        }
                        continue;
                    }
                    if case_insensitive_compare(&key, b"Trailer") {
                        if self.c.set_trailer_bytes(&value).is_err() {
                            self.c.connection_close = true;
                            return Err(ParseError::BadTrailer);
                        }
                        continue;
                    }
                }
                _ => {}
            }
            append_arg(&mut self.c.h, &key, &value, false);
        }
        if let Some(e) = s.err {
            self.c.connection_close = true;
            return Err(match e {
                ScanError::NeedMore => ParseError::NeedMore,
                ScanError::Malformed => ParseError::HeaderLine,
            });
        }

        if self.c.content_length < 0 {
            self.c.content_length_bytes.clear();
        }
        if self.c.no_http11 && !self.c.connection_close {
            let v = peek_arg(&self.c.h, HEADER_CONNECTION.as_bytes()).unwrap_or(&[]);
            self.c.connection_close = !has_header_value(v, b"keep-alive");
        }
        Ok(s.r)
    }

    pub(crate) fn parse_trailer_section(&mut self, src: &[u8]) -> Result<usize, TrailerError> {
        let mut src = src;
        if src.first() == Some(&b'0') {
            if src.len() < 3 {
                return Err(TrailerError::Eof);
            }
            src = &src[3..];
        }
        let mut s = HeaderScanner::new(src);
        while let Some((mut key, value)) = s.next() {
            if key.is_empty() {
                continue;
            }
            let mut disable = self.c.disable_normalizing;
            for &ch in &key {
                if !valid_header_field_byte(ch) {
                    if ch == b' ' {
                        disable = true;
                        continue;
                    }
                    return Err(TrailerError::Invalid);
                }
            }
            if is_bad_trailer(&key) {
                return Err(TrailerError::Invalid);
            }
            normalize_header_key(&mut key, disable);
            append_arg(&mut self.c.h, &key, &value, false);
        }
        if let Some(e) = s.err {
            return Err(match e {
                ScanError::NeedMore => TrailerError::NeedMore,
                ScanError::Malformed => TrailerError::Invalid,
            });
        }
        Ok(s.r)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TrailerError {
    NeedMore,
    Eof,
    Invalid,
}

#[derive(Clone, Debug, Default)]
pub struct ResponseHeader {
    c: Common,
    status_message: Vec<u8>,
    content_encoding: Vec<u8>,
    server: Vec<u8>,
    status_code: u16,
    no_default_date: bool,
}

impl ResponseHeader {
    pub fn new() -> ResponseHeader {
        ResponseHeader::default()
    }

    pub fn reset(&mut self) {
        *self = ResponseHeader::default();
    }

    pub fn status_code(&self) -> u16 {
        if self.status_code == 0 {
            200
        } else {
            self.status_code
        }
    }

    pub fn set_status_code(&mut self, status_code: u16) {
        self.status_code = status_code;
    }

    pub fn status_message(&self) -> &[u8] {
        &self.status_message
    }

    pub fn set_status_message(&mut self, status_message: impl AsRef<[u8]>) {
        self.status_message = status_message.as_ref().to_vec();
    }

    pub fn protocol(&self) -> &[u8] {
        if self.c.protocol.is_empty() {
            STR_HTTP11
        } else {
            &self.c.protocol
        }
    }

    pub fn set_protocol(&mut self, protocol: impl AsRef<[u8]>) {
        self.c.protocol = protocol.as_ref().to_vec();
    }

    pub fn connection_close(&self) -> bool {
        self.c.connection_close
    }

    pub fn set_connection_close(&mut self) {
        self.c.connection_close = true;
    }

    pub fn reset_connection_close(&mut self) {
        self.c.reset_connection_close();
    }

    pub fn set_no_default_content_type(&mut self, v: bool) {
        self.c.no_default_content_type = v;
    }

    pub fn set_no_default_date(&mut self, v: bool) {
        self.no_default_date = v;
    }

    pub fn disable_normalizing(&mut self) {
        self.c.disable_normalizing = true;
    }

    pub fn content_length(&self) -> i64 {
        self.c.content_length
    }

    fn must_skip_content_length(&self) -> bool {
        let status_code = self.status_code();
        if status_code < 100 || status_code == 200 {
            return false;
        }
        status_code == 304 || status_code == 204 || status_code < 200
    }

    pub fn set_content_length(&mut self, content_length: i64) {
        if self.must_skip_content_length() {
            return;
        }
        self.c.content_length = content_length;
        if content_length >= 0 {
            self.c.content_length_bytes.clear();
            append_uint(&mut self.c.content_length_bytes, content_length);
            del_all_args(&mut self.c.h, HEADER_TRANSFER_ENCODING.as_bytes());
            return;
        }
        if content_length == -1 {
            self.c.content_length_bytes.clear();
            set_arg(
                &mut self.c.h,
                HEADER_TRANSFER_ENCODING.as_bytes(),
                b"chunked",
                false,
            );
            return;
        }
        self.set_connection_close();
    }

    pub fn content_type(&self) -> &[u8] {
        if !self.c.no_default_content_type && self.c.content_type.is_empty() {
            return DEFAULT_CONTENT_TYPE;
        }
        &self.c.content_type
    }

    pub fn set_content_type(&mut self, content_type: impl AsRef<[u8]>) {
        self.c.content_type = content_type.as_ref().to_vec();
    }

    pub fn content_encoding(&self) -> &[u8] {
        &self.content_encoding
    }

    pub fn set_content_encoding(&mut self, content_encoding: impl AsRef<[u8]>) {
        self.content_encoding = content_encoding.as_ref().to_vec();
    }

    pub fn server(&self) -> &[u8] {
        &self.server
    }

    pub fn set_server(&mut self, server: impl AsRef<[u8]>) {
        self.server = server.as_ref().to_vec();
    }

    fn set_special_header(&mut self, key: &[u8], value: &[u8]) -> bool {
        if key.is_empty() {
            return false;
        }
        match key[0] | 0x20 {
            b'c' => {
                if case_insensitive_compare(b"Content-Type", key) {
                    self.set_content_type(value);
                    return true;
                }
                if case_insensitive_compare(b"Content-Length", key) {
                    if let Ok(cl) = parse_content_length(value) {
                        self.c.content_length = cl;
                        self.c.content_length_bytes = value.to_vec();
                    }
                    return true;
                }
                if case_insensitive_compare(b"Content-Encoding", key) {
                    self.set_content_encoding(value);
                    return true;
                }
                if case_insensitive_compare(b"Connection", key) {
                    if value == b"close" {
                        self.set_connection_close();
                    } else {
                        self.c.reset_connection_close();
                        self.c.set_non_special(key, value);
                    }
                    return true;
                }
            }
            b's' => {
                if case_insensitive_compare(b"Server", key) {
                    self.set_server(value);
                    return true;
                }
                if case_insensitive_compare(b"Set-Cookie", key) {
                    self.c.cookies.push(ArgsKV {
                        key: get_cookie_key(value).to_vec(),
                        value: value.to_vec(),
                        no_value: false,
                    });
                    self.c.touch_cookies();
                    return true;
                }
            }
            b't' => {
                if case_insensitive_compare(b"Transfer-Encoding", key) {
                    return true;
                }
                if case_insensitive_compare(b"Trailer", key) {
                    let _ = self.c.set_trailer_bytes(value);
                    return true;
                }
            }
            b'd' => {
                if case_insensitive_compare(b"Date", key) {
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    pub(crate) fn set_non_special(&mut self, key: &[u8], value: &[u8]) {
        self.c.set_non_special(key, value);
    }

    pub fn add(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        let (key, value) = (key.as_ref(), value.as_ref());
        if self.set_special_header(key, value) {
            return;
        }
        let k = header_key_bytes(key, self.c.disable_normalizing);
        append_arg(&mut self.c.h, &k, value, false);
    }

    pub fn set(&mut self, key: impl AsRef<[u8]>, value: impl AsRef<[u8]>) {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        let mut v = value.as_ref().to_vec();
        remove_new_lines(&mut v);
        self.set_canonical(&k, &v);
    }

    pub fn set_canonical(&mut self, key: &[u8], value: &[u8]) {
        if self.set_special_header(key, value) {
            return;
        }
        self.c.set_non_special(key, value);
    }

    pub fn del(&mut self, key: impl AsRef<[u8]>) {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        match k.as_slice() {
            b"Content-Type" => self.c.content_type.clear(),
            b"Content-Encoding" => self.content_encoding.clear(),
            b"Server" => self.server.clear(),
            b"Set-Cookie" => {
                self.c.cookies.clear();
                self.c.touch_cookies();
            }
            b"Content-Length" => {
                self.c.content_length = 0;
                self.c.content_length_bytes.clear();
            }
            b"Connection" => self.c.connection_close = false,
            b"Trailer" => {
                self.c.trailer.clear();
                self.c.trailer_bytes = OnceCell::new();
            }
            _ => {}
        }
        del_all_args(&mut self.c.h, &k);
    }

    pub fn peek(&self, key: impl AsRef<[u8]>) -> &[u8] {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        match k.as_slice() {
            b"Content-Type" => self.content_type(),
            b"Content-Encoding" => self.content_encoding(),
            b"Server" => self.server(),
            b"Connection" => {
                if self.connection_close() {
                    b"close"
                } else {
                    peek_arg(&self.c.h, &k).unwrap_or(&[])
                }
            }
            b"Content-Length" => &self.c.content_length_bytes,
            b"Set-Cookie" => self.response_cookie_bytes(),
            b"Trailer" => self.c.trailer_value(),
            _ => peek_arg(&self.c.h, &k).unwrap_or(&[]),
        }
    }

    fn response_cookie_bytes(&self) -> &[u8] {
        self.c.cookie_bytes.get_or_init(|| {
            let mut v = Vec::new();
            append_response_cookie_bytes(&mut v, &self.c.cookies);
            v
        })
    }

    pub fn peek_all(&self, key: impl AsRef<[u8]>) -> Vec<&[u8]> {
        let k = header_key_bytes(key.as_ref(), self.c.disable_normalizing);
        let mut out = Vec::new();
        match k.as_slice() {
            b"Content-Type" => {
                let ct = self.content_type();
                if !ct.is_empty() {
                    out.push(ct);
                }
            }
            b"Content-Encoding" => {
                if !self.content_encoding.is_empty() {
                    out.push(self.content_encoding());
                }
            }
            b"Server" => {
                if !self.server.is_empty() {
                    out.push(self.server());
                }
            }
            b"Connection" => {
                if self.connection_close() {
                    out.push(b"close".as_slice());
                } else {
                    out = peek_all_args(&self.c.h, &k);
                }
            }
            b"Content-Length" => out.push(self.c.content_length_bytes.as_slice()),
            b"Set-Cookie" => out.push(self.response_cookie_bytes()),
            b"Trailer" => out.push(self.c.trailer_value()),
            _ => out = peek_all_args(&self.c.h, &k),
        }
        out
    }

    pub fn peek_cookie(&self, key: impl AsRef<[u8]>) -> &[u8] {
        peek_arg(&self.c.cookies, key.as_ref()).unwrap_or(&[])
    }

    pub fn set_cookie(&mut self, cookie: &Cookie) {
        set_arg(&mut self.c.cookies, cookie.key(), &cookie.cookie(), false);
        self.c.touch_cookies();
    }

    pub fn cookie(&self, cookie: &mut Cookie) -> bool {
        match peek_arg(&self.c.cookies, cookie.key()) {
            None => false,
            Some(v) => {
                let v = v.to_vec();
                let _ = cookie.parse(v);
                true
            }
        }
    }

    pub fn del_cookie(&mut self, key: impl AsRef<[u8]>) {
        del_all_args(&mut self.c.cookies, key.as_ref());
        self.c.touch_cookies();
    }

    pub fn del_all_cookies(&mut self) {
        self.c.cookies.clear();
        self.c.touch_cookies();
    }

    pub fn all(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
        let mut out = Vec::new();
        if !self.c.content_length_bytes.is_empty() {
            out.push((
                HEADER_CONTENT_LENGTH.as_bytes().to_vec(),
                self.c.content_length_bytes.clone(),
            ));
        }
        let ct = self.content_type();
        if !ct.is_empty() {
            out.push((HEADER_CONTENT_TYPE.as_bytes().to_vec(), ct.to_vec()));
        }
        if !self.content_encoding.is_empty() {
            out.push((
                HEADER_CONTENT_ENCODING.as_bytes().to_vec(),
                self.content_encoding.clone(),
            ));
        }
        if !self.server.is_empty() {
            out.push((HEADER_SERVER.as_bytes().to_vec(), self.server.clone()));
        }
        for kv in &self.c.cookies {
            out.push((HEADER_SET_COOKIE.as_bytes().to_vec(), kv.value.clone()));
        }
        if !self.c.trailer.is_empty() {
            out.push((
                HEADER_TRAILER.as_bytes().to_vec(),
                self.c.trailer_value().to_vec(),
            ));
        }
        for kv in &self.c.h {
            out.push((kv.key.clone(), kv.value.clone()));
        }
        if self.connection_close() {
            out.push((HEADER_CONNECTION.as_bytes().to_vec(), b"close".to_vec()));
        }
        out
    }

    pub fn len(&self) -> usize {
        self.all().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn header(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        self.append_bytes(&mut dst);
        dst
    }

    pub fn append_bytes(&self, dst: &mut Vec<u8>) {
        format_status_line(
            dst,
            self.protocol(),
            self.status_code(),
            &self.status_message,
        );

        if !self.server.is_empty() {
            append_header_line(dst, b"Server", &self.server);
        }
        if !self.no_default_date {
            append_header_line(dst, b"Date", &server_date());
        }
        if self.content_length() != 0 || !self.c.content_type.is_empty() {
            let ct = self.content_type();
            if !ct.is_empty() {
                append_header_line(dst, b"Content-Type", ct);
            }
        }
        if !self.content_encoding.is_empty() {
            append_header_line(dst, b"Content-Encoding", &self.content_encoding);
        }
        if !self.c.content_length_bytes.is_empty() {
            append_header_line(dst, b"Content-Length", &self.c.content_length_bytes);
        }
        for kv in &self.c.h {
            let excluded = self.c.trailer.contains(&kv.key);
            if !excluded && (self.no_default_date || kv.key != b"Date") {
                append_header_line(dst, &kv.key, &kv.value);
            }
        }
        if !self.c.trailer.is_empty() {
            append_header_line(dst, b"Trailer", self.c.trailer_value());
        }
        for kv in &self.c.cookies {
            append_header_line(dst, b"Set-Cookie", &kv.value);
        }
        if self.connection_close() {
            append_header_line(dst, b"Connection", b"close");
        }
        dst.extend_from_slice(b"\r\n");
    }

    pub(crate) fn trailer_header(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        for t in &self.c.trailer {
            if let Some(v) = peek_arg(&self.c.h, t) {
                append_header_line(&mut dst, t, v);
            }
        }
        dst.extend_from_slice(b"\r\n");
        dst
    }
}
