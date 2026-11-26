use std::fmt;
use std::io::{self, BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{IpAddr, TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::{Duration, SystemTime};

use rustls::pki_types::ServerName;
use rustls::{ClientConfig, ClientConnection, RootCertStore, StreamOwned};

use crate::globals;
use crate::gocompat::base64::STD_ENCODING;
use crate::gocompat::strings::{equal_fold, to_lower, trim_space};
use crate::gocompat::time::{self, Location, RFC1123};
use crate::gocompat::url;

pub(crate) const UNEXPECTED_END_OF_BODY: &str = "unexpected end of the response body";

const MAX_REDIRECTS: usize = 10;
const MAX_HEADER_BYTES: usize = 10 << 20;

trait Stream: Read + Write + Send {}

impl<T: Read + Write + Send> Stream for T {}

#[derive(Debug)]
pub(crate) struct ClientError {
    url: Vec<u8>,
    reason: String,
}

impl fmt::Display for ClientError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "request to {} failed: {}",
            String::from_utf8_lossy(&self.url),
            self.reason
        )
    }
}

impl std::error::Error for ClientError {}

#[derive(Clone)]
pub(crate) struct Request {
    pub(crate) method: &'static str,
    pub(crate) url: Vec<u8>,
    pub(crate) headers: Vec<(&'static str, Vec<u8>)>,
    pub(crate) body: Vec<u8>,
}

impl Request {
    pub(crate) fn new(method: &'static str, url: Vec<u8>) -> Request {
        Request {
            method,
            url,
            headers: Vec::new(),
            body: Vec::new(),
        }
    }

    pub(crate) fn set_header(&mut self, name: &'static str, value: Vec<u8>) {
        self.headers.retain(|(n, _)| !n.eq_ignore_ascii_case(name));
        self.headers.push((name, value));
    }

    pub(crate) fn set_basic_auth(&mut self, user: &[u8], password: &[u8]) {
        self.set_header("Authorization", basic_auth(user, password));
    }

    fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }
}

fn basic_auth(user: &[u8], password: &[u8]) -> Vec<u8> {
    let mut plain = user.to_vec();
    plain.push(b':');
    plain.extend_from_slice(password);
    let mut out = b"Basic ".to_vec();
    out.extend_from_slice(STD_ENCODING.encode_to_string(&plain).as_bytes());
    out
}

pub(crate) struct Response {
    pub(crate) status_code: u16,
    pub(crate) status: Vec<u8>,
    headers: Vec<(Vec<u8>, Vec<u8>)>,
    pub(crate) body: Body,
}

impl Response {
    pub(crate) fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(n, _)| equal_fold(n, name.as_bytes()))
            .map(|(_, v)| v.as_slice())
    }

    fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.headers
            .iter()
            .filter(move |(n, _)| equal_fold(n, name.as_bytes()))
            .map(|(_, v)| v.as_slice())
    }
}

enum Framing {
    Empty,
    Length(u64),
    Chunked { left: u64, done: bool },
    Close,
}

pub(crate) struct Body {
    reader: BufReader<Box<dyn Stream>>,
    framing: Framing,
}

fn end_of_body() -> io::Error {
    io::Error::new(ErrorKind::UnexpectedEof, UNEXPECTED_END_OF_BODY)
}

fn malformed_chunk() -> io::Error {
    io::Error::new(ErrorKind::InvalidData, "malformed chunked encoding")
}

impl Body {
    fn read_chunk_size(&mut self) -> io::Result<u64> {
        let mut line = Vec::new();
        let n = self
            .reader
            .by_ref()
            .take(4096)
            .read_until(b'\n', &mut line)?;
        if n == 0 || line.last() != Some(&b'\n') {
            return Err(end_of_body());
        }

        let line = trim_space(&line);
        let size = match line.iter().position(|&c| c == b';') {
            Some(i) => trim_space(&line[..i]),
            None => line,
        };
        if size.is_empty() || size.len() > 16 {
            return Err(malformed_chunk());
        }

        let mut n: u64 = 0;
        for &c in size {
            let d = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                b'A'..=b'F' => c - b'A' + 10,
                _ => return Err(malformed_chunk()),
            };
            n = (n << 4) | u64::from(d);
        }

        Ok(n)
    }

    fn read_crlf(&mut self) -> io::Result<()> {
        let mut crlf = [0u8; 2];
        match self.reader.read_exact(&mut crlf) {
            Ok(()) if crlf == *b"\r\n" => Ok(()),
            Ok(()) => Err(malformed_chunk()),
            Err(e) if e.kind() == ErrorKind::UnexpectedEof => Err(end_of_body()),
            Err(e) => Err(e),
        }
    }

    fn skip_trailers(&mut self) -> io::Result<()> {
        loop {
            let mut line = Vec::new();
            let n = self
                .reader
                .by_ref()
                .take(4096)
                .read_until(b'\n', &mut line)?;
            if n == 0 {
                return Err(end_of_body());
            }
            if trim_space(&line).is_empty() {
                return Ok(());
            }
        }
    }
}

impl Read for Body {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }

        match self.framing {
            Framing::Empty => Ok(0),
            Framing::Length(0) => Ok(0),
            Framing::Length(left) => {
                let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let n = read_stream(&mut self.reader, &mut buf[..want])?;
                if n == 0 {
                    return Err(end_of_body());
                }
                self.framing = Framing::Length(left - n as u64);
                Ok(n)
            }
            Framing::Close => match read_stream(&mut self.reader, buf) {
                Err(e) if e.kind() == ErrorKind::UnexpectedEof => Ok(0),
                other => other,
            },
            Framing::Chunked { left, done } => {
                if done {
                    return Ok(0);
                }

                let mut left = left;
                if left == 0 {
                    left = self.read_chunk_size()?;
                    if left == 0 {
                        self.skip_trailers()?;
                        self.framing = Framing::Chunked {
                            left: 0,
                            done: true,
                        };
                        return Ok(0);
                    }
                }

                let want = buf.len().min(usize::try_from(left).unwrap_or(usize::MAX));
                let n = read_stream(&mut self.reader, &mut buf[..want])?;
                if n == 0 {
                    return Err(end_of_body());
                }
                left -= n as u64;
                if left == 0 {
                    self.read_crlf()?;
                }
                self.framing = Framing::Chunked { left, done: false };
                Ok(n)
            }
        }
    }
}

fn read_stream(r: &mut BufReader<Box<dyn Stream>>, buf: &mut [u8]) -> io::Result<usize> {
    loop {
        match r.read(buf) {
            Err(e) if e.kind() == ErrorKind::Interrupted => continue,
            other => return other,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Scheme {
    Http,
    Https,
}

#[derive(Clone)]
struct Target {
    scheme: Scheme,
    host: Vec<u8>,
    port: u16,
    authority: Vec<u8>,
    request_uri: Vec<u8>,
    user: Option<(Vec<u8>, Option<Vec<u8>>)>,
}

impl Target {
    fn origin(&self) -> Vec<u8> {
        let mut out = match self.scheme {
            Scheme::Http => b"http://".to_vec(),
            Scheme::Https => b"https://".to_vec(),
        };
        out.extend_from_slice(&self.authority);
        out
    }

    fn absolute(&self) -> Vec<u8> {
        let mut out = self.origin();
        out.extend_from_slice(&self.request_uri);
        out
    }

    fn host_port(&self) -> Vec<u8> {
        let mut out = Vec::new();
        if self.host.contains(&b':') {
            out.push(b'[');
            out.extend_from_slice(&self.host);
            out.push(b']');
        } else {
            out.extend_from_slice(&self.host);
        }
        out.push(b':');
        out.extend_from_slice(self.port.to_string().as_bytes());
        out
    }

    fn path(&self) -> &[u8] {
        let end = self
            .request_uri
            .iter()
            .position(|&c| c == b'?')
            .unwrap_or(self.request_uri.len());
        &self.request_uri[..end]
    }
}

fn split_scheme(raw: &[u8]) -> Option<(&[u8], &[u8])> {
    let i = raw.iter().position(|&c| c == b':')?;
    let scheme = &raw[..i];
    if scheme.is_empty() || !scheme[0].is_ascii_alphabetic() {
        return None;
    }
    if !scheme
        .iter()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, b'+' | b'-' | b'.'))
    {
        return None;
    }
    Some((scheme, &raw[i + 1..]))
}

fn unescape(s: &[u8]) -> Vec<u8> {
    url::path_unescape(s).unwrap_or_else(|_| s.to_vec())
}

fn parse_target(raw: &[u8]) -> Option<Target> {
    url::parse(raw).ok()?;
    let (scheme, rest) = split_scheme(raw)?;
    let scheme = match to_lower(scheme).as_slice() {
        b"http" => Scheme::Http,
        b"https" => Scheme::Https,
        _ => return None,
    };
    let rest = rest.strip_prefix(b"//")?;
    let rest = match rest.iter().position(|&c| c == b'#') {
        Some(i) => &rest[..i],
        None => rest,
    };
    let end = rest
        .iter()
        .position(|&c| c == b'/' || c == b'?')
        .unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(end);

    let (user, authority) = match authority.iter().rposition(|&c| c == b'@') {
        Some(i) => {
            let info = &authority[..i];
            let user = match info.iter().position(|&c| c == b':') {
                Some(j) => (unescape(&info[..j]), Some(unescape(&info[j + 1..]))),
                None => (unescape(info), None),
            };
            (Some(user), &authority[i + 1..])
        }
        None => (None, authority),
    };

    let (host, port) = if let Some(stripped) = authority.strip_prefix(b"[") {
        let close = stripped.iter().position(|&c| c == b']')?;
        let host = &stripped[..close];
        let after = &stripped[close + 1..];
        let port = match after {
            [] => None,
            [b':', p @ ..] => Some(p),
            _ => return None,
        };
        (host, port)
    } else {
        match authority.iter().rposition(|&c| c == b':') {
            Some(i) => (&authority[..i], Some(&authority[i + 1..])),
            None => (authority, None),
        }
    };

    if host.is_empty() {
        return None;
    }

    let port = match port {
        None | Some([]) => match scheme {
            Scheme::Http => 80,
            Scheme::Https => 443,
        },
        Some(p) => std::str::from_utf8(p).ok()?.parse::<u16>().ok()?,
    };

    let mut request_uri = escape_request_uri(tail);
    if request_uri.is_empty() || request_uri[0] == b'?' {
        request_uri.insert(0, b'/');
    }

    Some(Target {
        scheme,
        host: to_lower(host),
        port,
        authority: authority.to_vec(),
        request_uri,
        user,
    })
}

fn escape_request_uri(tail: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(tail.len());
    let mut i = 0;
    while i < tail.len() {
        let c = tail[i];
        let keep = c.is_ascii_alphanumeric()
            || b"-_.~!$&'()*+,;=:@/?[]".contains(&c)
            || (c == b'%'
                && i + 2 < tail.len() + 1
                && tail.get(i + 1).is_some_and(u8::is_ascii_hexdigit)
                && tail.get(i + 2).is_some_and(u8::is_ascii_hexdigit));
        if keep {
            out.push(c);
        } else {
            out.extend_from_slice(format!("%{c:02X}").as_bytes());
        }
        i += 1;
    }
    out
}

fn redacted(raw: &[u8]) -> Vec<u8> {
    let Some((_, rest)) = split_scheme(raw) else {
        return raw.to_vec();
    };
    let Some(rest) = rest.strip_prefix(b"//") else {
        return raw.to_vec();
    };
    let start = raw.len() - rest.len();
    let end = rest
        .iter()
        .position(|&c| c == b'/' || c == b'?' || c == b'#')
        .unwrap_or(rest.len());
    let authority = &rest[..end];
    let Some(at) = authority.iter().rposition(|&c| c == b'@') else {
        return raw.to_vec();
    };
    let info = &authority[..at];
    let Some(colon) = info.iter().position(|&c| c == b':') else {
        return raw.to_vec();
    };
    let mut out = raw[..start + colon + 1].to_vec();
    out.extend_from_slice(b"***");
    out.extend_from_slice(&raw[start + at..]);
    out
}

fn resolve_reference(base: &Target, loc: &[u8]) -> Option<Vec<u8>> {
    if split_scheme(loc).is_some_and(|(_, rest)| rest.starts_with(b"//")) {
        return Some(loc.to_vec());
    }

    if let Some(rest) = loc.strip_prefix(b"//") {
        let mut out = match base.scheme {
            Scheme::Http => b"http://".to_vec(),
            Scheme::Https => b"https://".to_vec(),
        };
        out.extend_from_slice(rest);
        return Some(out);
    }

    let mut out = base.origin();
    if loc.starts_with(b"/") {
        out.extend_from_slice(loc);
        return Some(out);
    }

    if loc.starts_with(b"?") {
        out.extend_from_slice(base.path());
        out.extend_from_slice(loc);
        return Some(out);
    }

    if loc.is_empty() {
        out.extend_from_slice(&base.request_uri);
        return Some(out);
    }

    let path = base.path();
    let dir = match path.iter().rposition(|&c| c == b'/') {
        Some(i) => &path[..=i],
        None => b"/",
    };
    let mut joined = dir.to_vec();
    joined.extend_from_slice(loc);
    let (p, q) = match joined.iter().position(|&c| c == b'?') {
        Some(i) => (joined[..i].to_vec(), joined[i..].to_vec()),
        None => (joined, Vec::new()),
    };
    out.extend_from_slice(&remove_dot_segments(&p));
    out.extend_from_slice(&q);
    Some(out)
}

fn remove_dot_segments(path: &[u8]) -> Vec<u8> {
    let mut parts: Vec<&[u8]> = Vec::new();
    let segments: Vec<&[u8]> = path.split(|&c| c == b'/').collect();
    let last = segments.len().saturating_sub(1);
    let mut trailing = false;
    for (i, seg) in segments.iter().enumerate() {
        match *seg {
            b"." => trailing = i == last,
            b".." => {
                if parts.len() > 1 {
                    parts.pop();
                }
                trailing = i == last;
            }
            s => {
                parts.push(s);
                trailing = false;
            }
        }
    }
    let mut out = parts.join(&b'/');
    if trailing {
        out.push(b'/');
    }
    if !out.starts_with(b"/") {
        out.insert(0, b'/');
    }
    out
}

fn is_domain_or_subdomain(sub: &[u8], parent: &[u8]) -> bool {
    if sub == parent {
        return true;
    }
    if sub.contains(&b':') || sub.contains(&b'%') {
        return false;
    }
    sub.len() > parent.len() && sub.ends_with(parent) && sub[sub.len() - parent.len() - 1] == b'.'
}

#[derive(Clone)]
struct Cookie {
    name: Vec<u8>,
    value: Vec<u8>,
    domain: Vec<u8>,
    host_only: bool,
    path: Vec<u8>,
    secure: bool,
    expires: Option<SystemTime>,
    created: u64,
}

#[derive(Default)]
struct Jar {
    cookies: Vec<Cookie>,
    next: u64,
}

fn is_ip(host: &[u8]) -> bool {
    std::str::from_utf8(host)
        .ok()
        .and_then(|h| h.parse::<IpAddr>().ok())
        .is_some()
}

fn default_path(path: &[u8]) -> Vec<u8> {
    if path.is_empty() || path[0] != b'/' {
        return b"/".to_vec();
    }
    match path.iter().rposition(|&c| c == b'/') {
        Some(0) | None => b"/".to_vec(),
        Some(i) => path[..i].to_vec(),
    }
}

fn path_match(request: &[u8], cookie: &[u8]) -> bool {
    if request == cookie {
        return true;
    }
    request.starts_with(cookie)
        && (cookie.ends_with(b"/") || request.get(cookie.len()) == Some(&b'/'))
}

impl Jar {
    fn set_cookies(&mut self, target: &Target, values: &[&[u8]]) {
        let now = SystemTime::now();
        for raw in values {
            let Some(mut c) = parse_set_cookie(raw, now) else {
                continue;
            };

            if c.domain.is_empty() {
                c.domain = target.host.clone();
                c.host_only = true;
            } else {
                let domain = to_lower(c.domain.strip_prefix(b".").unwrap_or(&c.domain));
                if domain.is_empty() || domain.ends_with(b".") {
                    continue;
                }
                if is_ip(&target.host) {
                    if domain != target.host {
                        continue;
                    }
                    c.host_only = true;
                } else if domain != target.host && !is_domain_or_subdomain(&target.host, &domain) {
                    continue;
                } else {
                    c.host_only = false;
                }
                c.domain = domain;
            }

            if c.path.is_empty() || c.path[0] != b'/' {
                c.path = default_path(target.path());
            }

            let expired = c.expires.is_some_and(|t| t <= now);
            if let Some(i) = self
                .cookies
                .iter()
                .position(|e| e.name == c.name && e.domain == c.domain && e.path == c.path)
            {
                if expired {
                    self.cookies.remove(i);
                    continue;
                }
                c.created = self.cookies[i].created;
                self.cookies[i] = c;
                continue;
            }

            if expired {
                continue;
            }

            c.created = self.next;
            self.next += 1;
            self.cookies.push(c);
        }
    }

    fn cookies(&mut self, target: &Target) -> Option<Vec<u8>> {
        let now = SystemTime::now();
        self.cookies.retain(|c| c.expires.is_none_or(|t| t > now));
        let https = target.scheme == Scheme::Https;
        let path = target.path();
        let mut selected: Vec<&Cookie> = self
            .cookies
            .iter()
            .filter(|c| {
                let host_ok = if c.host_only {
                    c.domain == target.host
                } else {
                    is_domain_or_subdomain(&target.host, &c.domain)
                };
                host_ok && path_match(path, &c.path) && (!c.secure || https)
            })
            .collect();
        if selected.is_empty() {
            return None;
        }

        selected.sort_by(|a, b| {
            b.path
                .len()
                .cmp(&a.path.len())
                .then(a.created.cmp(&b.created))
        });
        let mut out = Vec::new();
        for (i, c) in selected.iter().enumerate() {
            if i > 0 {
                out.extend_from_slice(b"; ");
            }
            out.extend_from_slice(&c.name);
            out.push(b'=');
            out.extend_from_slice(&c.value);
        }
        Some(out)
    }
}

fn is_token(b: &[u8]) -> bool {
    !b.is_empty()
        && b.iter()
            .all(|&c| c > 0x20 && c < 0x7f && !b"\"(),/:;<=>?@[\\]{}".contains(&c))
}

fn parse_set_cookie(raw: &[u8], now: SystemTime) -> Option<Cookie> {
    let mut parts = raw.split(|&c| c == b';');
    let first = trim_space(parts.next()?);
    let eq = first.iter().position(|&c| c == b'=')?;
    let name = trim_space(&first[..eq]);
    if !is_token(name) {
        return None;
    }
    let mut value = trim_space(&first[eq + 1..]);
    if value.len() > 1 && value[0] == b'"' && value[value.len() - 1] == b'"' {
        value = &value[1..value.len() - 1];
    }

    let mut c = Cookie {
        name: name.to_vec(),
        value: value.to_vec(),
        domain: Vec::new(),
        host_only: true,
        path: Vec::new(),
        secure: false,
        expires: None,
        created: 0,
    };

    let mut max_age: Option<i64> = None;
    let mut expires: Option<SystemTime> = None;
    for attr in parts {
        let attr = trim_space(attr);
        if attr.is_empty() {
            continue;
        }
        let (key, val) = match attr.iter().position(|&c| c == b'=') {
            Some(i) => (trim_space(&attr[..i]), trim_space(&attr[i + 1..])),
            None => (attr, &b""[..]),
        };
        match to_lower(key).as_slice() {
            b"secure" => c.secure = true,
            b"domain" => c.domain = val.to_vec(),
            b"path" => c.path = val.to_vec(),
            b"max-age" => {
                if let Some(n) = std::str::from_utf8(val)
                    .ok()
                    .and_then(|v| v.parse::<i64>().ok())
                    && (n == 0 || val.first() != Some(&b'0'))
                {
                    max_age = Some(n);
                }
            }
            b"expires" => {
                if let Some(t) = parse_http_date(val) {
                    expires = Some(t);
                }
            }
            _ => {}
        }
    }

    c.expires = match max_age {
        Some(n) if n <= 0 => Some(SystemTime::UNIX_EPOCH),
        Some(n) => Some(now + Duration::from_secs(n as u64)),
        None => expires,
    };

    Some(c)
}

fn parse_http_date(val: &[u8]) -> Option<SystemTime> {
    let t = time::parse_in_location(RFC1123, val, &Location::utc())
        .or_else(|_| time::parse("Mon, 02-Jan-2006 15:04:05 MST", val))
        .ok()?;
    let secs = t.unix();
    if secs >= 0 {
        Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs as u64))
    } else {
        Some(SystemTime::UNIX_EPOCH)
    }
}

struct Proxy {
    scheme: ProxyScheme,
    target: Target,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ProxyScheme {
    Http,
    Https,
    Socks5,
}

fn env_any(names: &[&str]) -> Vec<u8> {
    for name in names {
        if let Some(v) = std::env::var_os(name) {
            let v = std::os::unix::ffi::OsStrExt::as_bytes(v.as_os_str()).to_vec();
            if !v.is_empty() {
                return v;
            }
        }
    }
    Vec::new()
}

fn parse_proxy(raw: &[u8]) -> Option<Proxy> {
    if raw.is_empty() {
        return None;
    }

    let with_scheme = |raw: &[u8]| -> Option<Proxy> {
        let (scheme, rest) = split_scheme(raw)?;
        let scheme = match to_lower(scheme).as_slice() {
            b"http" => ProxyScheme::Http,
            b"https" => ProxyScheme::Https,
            b"socks5" | b"socks5h" => ProxyScheme::Socks5,
            _ => return None,
        };
        let mut as_http = b"http:".to_vec();
        as_http.extend_from_slice(rest);
        let mut target = parse_target(&as_http)?;
        if scheme == ProxyScheme::Https && rest.starts_with(b"//") {
            let mut as_https = b"https:".to_vec();
            as_https.extend_from_slice(rest);
            target = parse_target(&as_https)?;
        }
        if scheme == ProxyScheme::Socks5 && !target.authority.contains(&b':') {
            target.port = 1080;
        }
        Some(Proxy { scheme, target })
    };

    if let Some(p) = with_scheme(raw) {
        return Some(p);
    }

    let mut prefixed = b"http://".to_vec();
    prefixed.extend_from_slice(raw);
    with_scheme(&prefixed)
}

enum NoProxy {
    All,
    Cidr(IpAddr, u8),
    Ip(IpAddr, Vec<u8>),
    Domain {
        host: Vec<u8>,
        port: Vec<u8>,
        match_host: bool,
    },
}

fn split_host_port(s: &[u8]) -> Option<(Vec<u8>, Vec<u8>)> {
    if let Some(rest) = s.strip_prefix(b"[") {
        let close = rest.iter().position(|&c| c == b']')?;
        let after = &rest[close + 1..];
        let port = after.strip_prefix(b":")?;
        return Some((rest[..close].to_vec(), port.to_vec()));
    }
    let i = s.iter().rposition(|&c| c == b':')?;
    if s[..i].contains(&b':') {
        return None;
    }
    Some((s[..i].to_vec(), s[i + 1..].to_vec()))
}

fn parse_ip(s: &[u8]) -> Option<IpAddr> {
    std::str::from_utf8(s).ok()?.parse::<IpAddr>().ok()
}

fn parse_no_proxy(raw: &[u8]) -> Vec<NoProxy> {
    let mut out = Vec::new();
    for p in raw.split(|&c| c == b',') {
        let p = to_lower(trim_space(p));
        if p.is_empty() {
            continue;
        }
        if p == b"*" {
            return vec![NoProxy::All];
        }
        if let Some(i) = p.iter().position(|&c| c == b'/')
            && let Some(ip) = parse_ip(&p[..i])
            && let Some(bits) = std::str::from_utf8(&p[i + 1..])
                .ok()
                .and_then(|b| b.parse::<u8>().ok())
        {
            let max = if ip.is_ipv4() { 32 } else { 128 };
            if bits <= max {
                out.push(NoProxy::Cidr(ip, bits));
                continue;
            }
        }
        let (mut host, port) = match split_host_port(&p) {
            Some((h, port)) => {
                if h.is_empty() {
                    continue;
                }
                (h, port)
            }
            None => (p.clone(), Vec::new()),
        };
        if let Some(ip) = parse_ip(&host) {
            out.push(NoProxy::Ip(ip, port));
            continue;
        }
        if host.is_empty() {
            continue;
        }
        if host.starts_with(b"*.") {
            host.remove(0);
        }
        let mut match_host = false;
        if host[0] != b'.' {
            match_host = true;
            host.insert(0, b'.');
        }
        out.push(NoProxy::Domain {
            host,
            port,
            match_host,
        });
    }
    out
}

fn cidr_contains(net: &IpAddr, bits: u8, ip: &IpAddr) -> bool {
    match (net, ip) {
        (IpAddr::V4(n), IpAddr::V4(i)) => {
            let mask = if bits == 0 {
                0
            } else {
                u32::MAX << (32 - u32::from(bits))
            };
            (u32::from(*n) & mask) == (u32::from(*i) & mask)
        }
        (IpAddr::V6(n), IpAddr::V6(i)) => {
            let mask = if bits == 0 {
                0
            } else {
                u128::MAX << (128 - u32::from(bits))
            };
            (u128::from(*n) & mask) == (u128::from(*i) & mask)
        }
        _ => false,
    }
}

fn use_proxy(target: &Target, rules: &[NoProxy]) -> bool {
    let host = &target.host;
    let port = target.port.to_string().into_bytes();
    if host == b"localhost" {
        return false;
    }
    let ip = parse_ip(host);
    if ip.is_some_and(|ip| ip.is_loopback()) {
        return false;
    }
    for rule in rules {
        let matched = match rule {
            NoProxy::All => true,
            NoProxy::Cidr(net, bits) => ip.is_some_and(|ip| cidr_contains(net, *bits, &ip)),
            NoProxy::Ip(want, p) => {
                ip.is_some_and(|ip| ip == *want && (p.is_empty() || *p == port))
            }
            NoProxy::Domain {
                host: suffix,
                port: p,
                match_host,
            } => {
                ip.is_none()
                    && (host.ends_with(suffix) || (*match_host && host[..] == suffix[1..]))
                    && (p.is_empty() || *p == port)
            }
        };
        if matched {
            return false;
        }
    }
    true
}

fn proxy_for(target: &Target) -> Option<Proxy> {
    let raw = match target.scheme {
        Scheme::Https => env_any(&["HTTPS_PROXY", "https_proxy"]),
        Scheme::Http => {
            if std::env::var_os("REQUEST_METHOD").is_some() {
                return None;
            }
            env_any(&["HTTP_PROXY", "http_proxy"])
        }
    };
    let proxy = parse_proxy(&raw)?;
    let rules = parse_no_proxy(&env_any(&["NO_PROXY", "no_proxy"]));
    if !use_proxy(target, &rules) {
        return None;
    }
    Some(proxy)
}

fn connect_reason(e: &io::Error) -> String {
    match e.kind() {
        ErrorKind::ConnectionRefused => "connection refused".to_string(),
        ErrorKind::TimedOut => "timed out".to_string(),
        _ => io_reason(e),
    }
}

fn io_reason(e: &io::Error) -> String {
    if let Some(inner) = e.get_ref()
        && let Some(tls) = inner.downcast_ref::<rustls::Error>()
    {
        return format!("TLS error: {tls}");
    }
    if e.kind() == ErrorKind::TimedOut {
        return "timed out".to_string();
    }
    e.to_string()
}

fn dial(host: &[u8], port: u16) -> Result<TcpStream, String> {
    let host = std::str::from_utf8(host).map_err(|_| "invalid URL".to_string())?;
    let addrs = (host, port)
        .to_socket_addrs()
        .map_err(|e| connect_reason(&e))?;
    let mut last: Option<io::Error> = None;
    for addr in addrs {
        match TcpStream::connect(addr) {
            Ok(s) => {
                let _ = s.set_nodelay(true);
                return Ok(s);
            }
            Err(e) => last = Some(e),
        }
    }
    Err(match last {
        Some(e) => connect_reason(&e),
        None => io_reason(&io::Error::from(ErrorKind::AddrNotAvailable)),
    })
}

pub(crate) struct Client {
    jar: Jar,
    tls: Option<Arc<ClientConfig>>,
}

impl Client {
    pub(crate) fn new() -> Client {
        Client {
            jar: Jar::default(),
            tls: None,
        }
    }

    fn tls_config(&mut self) -> Result<Arc<ClientConfig>, String> {
        if let Some(cfg) = &self.tls {
            return Ok(cfg.clone());
        }

        let mut roots = RootCertStore::empty();
        for cert in rustls_native_certs::load_native_certs().certs {
            let _ = roots.add(cert);
        }
        let provider = Arc::new(rustls::crypto::ring::default_provider());
        let cfg = ClientConfig::builder_with_provider(provider)
            .with_safe_default_protocol_versions()
            .map_err(|e| format!("TLS error: {e}"))?
            .with_root_certificates(roots)
            .with_no_client_auth();
        let cfg = Arc::new(cfg);
        self.tls = Some(cfg.clone());
        Ok(cfg)
    }

    fn tls_wrap(&mut self, tcp: Box<dyn Stream>, host: &[u8]) -> Result<Box<dyn Stream>, String> {
        let cfg = self.tls_config()?;
        let name = std::str::from_utf8(host)
            .ok()
            .and_then(|h| ServerName::try_from(h.to_string()).ok())
            .ok_or_else(|| "TLS error: invalid server name".to_string())?;
        let mut conn = ClientConnection::new(cfg, name).map_err(|e| format!("TLS error: {e}"))?;
        let mut tcp = tcp;
        while conn.is_handshaking() {
            conn.complete_io(&mut tcp).map_err(|e| match e.kind() {
                ErrorKind::UnexpectedEof => "connection closed by the server".to_string(),
                _ => io_reason(&e),
            })?;
        }
        Ok(Box::new(StreamOwned::new(conn, tcp)))
    }

    fn connect(
        &mut self,
        target: &Target,
        proxy: Option<&Proxy>,
    ) -> Result<Box<dyn Stream>, String> {
        let Some(proxy) = proxy else {
            let tcp: Box<dyn Stream> = Box::new(dial(&target.host, target.port)?);
            return match target.scheme {
                Scheme::Http => Ok(tcp),
                Scheme::Https => self.tls_wrap(tcp, &target.host),
            };
        };

        let mut conn: Box<dyn Stream> = Box::new(dial(&proxy.target.host, proxy.target.port)?);
        if proxy.scheme == ProxyScheme::Https {
            conn = self.tls_wrap(conn, &proxy.target.host)?;
        }

        match (proxy.scheme, target.scheme) {
            (ProxyScheme::Socks5, _) => {
                socks5_connect(&mut conn, proxy, target)?;
                match target.scheme {
                    Scheme::Http => Ok(conn),
                    Scheme::Https => self.tls_wrap(conn, &target.host),
                }
            }
            (_, Scheme::Http) => Ok(conn),
            (_, Scheme::Https) => {
                let hp = target.host_port();
                let mut req = b"CONNECT ".to_vec();
                req.extend_from_slice(&hp);
                req.extend_from_slice(b" HTTP/1.1\r\nHost: ");
                req.extend_from_slice(&hp);
                req.extend_from_slice(b"\r\nUser-Agent: ");
                req.extend_from_slice(user_agent().as_bytes());
                req.extend_from_slice(b"\r\n");
                if let Some(auth) = proxy_authorization(proxy) {
                    req.extend_from_slice(b"Proxy-Authorization: ");
                    req.extend_from_slice(&auth);
                    req.extend_from_slice(b"\r\n");
                }
                req.extend_from_slice(b"\r\n");
                conn.write_all(&req).map_err(|e| io_reason(&e))?;
                conn.flush().map_err(|e| io_reason(&e))?;
                let head = read_head_unbuffered(&mut conn)?;
                let (code, _) = parse_status_line(head.first().map(Vec::as_slice).unwrap_or(b""))?;
                if code != 200 {
                    return Err(format!("unexpected status {code}"));
                }
                self.tls_wrap(conn, &target.host)
            }
        }
    }

    fn round_trip(&mut self, req: &Request, target: &Target) -> Result<Response, String> {
        let proxy = proxy_for(target);
        let mut stream = self.connect(target, proxy.as_ref())?;

        let mut out = Vec::new();
        out.extend_from_slice(req.method.as_bytes());
        out.push(b' ');
        let through_http_proxy = proxy
            .as_ref()
            .is_some_and(|p| p.scheme != ProxyScheme::Socks5 && target.scheme == Scheme::Http);
        if through_http_proxy {
            out.extend_from_slice(&target.absolute());
        } else {
            out.extend_from_slice(&target.request_uri);
        }
        out.extend_from_slice(b" HTTP/1.1\r\nHost: ");
        out.extend_from_slice(&target.authority);
        out.extend_from_slice(b"\r\nUser-Agent: ");
        out.extend_from_slice(user_agent().as_bytes());
        out.extend_from_slice(b"\r\n");
        if !req.body.is_empty() || req.method == "POST" || req.method == "PUT" {
            out.extend_from_slice(format!("Content-Length: {}\r\n", req.body.len()).as_bytes());
        }
        let mut headers: Vec<(&str, Vec<u8>)> =
            req.headers.iter().map(|(n, v)| (*n, v.clone())).collect();
        if req.header("Authorization").is_none()
            && let Some((user, password)) = &target.user
        {
            headers.push((
                "Authorization",
                basic_auth(user, password.as_deref().unwrap_or(b"")),
            ));
        }
        if through_http_proxy && let Some(auth) = proxy.as_ref().and_then(proxy_authorization) {
            headers.push(("Proxy-Authorization", auth));
        }
        headers.sort_by(|a, b| a.0.cmp(b.0));
        for (name, value) in &headers {
            out.extend_from_slice(name.as_bytes());
            out.extend_from_slice(b": ");
            out.extend_from_slice(value);
            out.extend_from_slice(b"\r\n");
        }
        if let Some(cookies) = self.jar.cookies(target) {
            out.extend_from_slice(b"Cookie: ");
            out.extend_from_slice(&cookies);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(&req.body);

        stream.write_all(&out).map_err(|e| io_reason(&e))?;
        stream.flush().map_err(|e| io_reason(&e))?;

        let mut reader = BufReader::with_capacity(64 << 10, stream);
        loop {
            let head = read_head(&mut reader)?;
            let (code, status) = parse_status_line(&head[0])?;
            let headers = parse_headers(&head[1..]);
            if (100..200).contains(&code) && code != 101 {
                continue;
            }

            let mut resp = Response {
                status_code: code,
                status,
                headers,
                body: Body {
                    reader,
                    framing: Framing::Empty,
                },
            };
            resp.body.framing = framing(req.method, &resp)?;
            return Ok(resp);
        }
    }

    pub(crate) fn send(&mut self, req: Request) -> Result<Response, ClientError> {
        let first = req.clone();
        let mut req = req;
        let mut sent = 0usize;
        loop {
            let fail = |url: &[u8], reason: String| ClientError {
                url: redacted(url),
                reason,
            };
            let Some(target) = parse_target(&req.url) else {
                return Err(fail(&req.url, "invalid URL".to_string()));
            };

            let resp = self
                .round_trip(&req, &target)
                .map_err(|reason| fail(&req.url, reason))?;
            sent += 1;

            let set_cookies: Vec<Vec<u8>> = resp
                .header_values("Set-Cookie")
                .map(<[u8]>::to_vec)
                .collect();
            let refs: Vec<&[u8]> = set_cookies.iter().map(Vec::as_slice).collect();
            self.jar.set_cookies(&target, &refs);

            let (method, redirect, include_body) = match resp.status_code {
                301..=303 => {
                    let m = if req.method == "GET" || req.method == "HEAD" {
                        req.method
                    } else {
                        "GET"
                    };
                    (m, true, false)
                }
                307 | 308 => (req.method, true, true),
                _ => (req.method, false, false),
            };
            if !redirect {
                return Ok(resp);
            }

            let Some(loc) = resp.header("Location").map(<[u8]>::to_vec) else {
                return Ok(resp);
            };
            let Some(next_url) = resolve_reference(&target, &loc) else {
                return Err(fail(&req.url, "invalid URL".to_string()));
            };

            if sent >= MAX_REDIRECTS {
                return Err(fail(&loc, "too many redirects".to_string()));
            }
            drop(resp);

            let first_host = parse_target(&first.url).map(|t| t.host).unwrap_or_default();
            let next_host = parse_target(&next_url).map(|t| t.host).unwrap_or_default();
            let same_domain = is_domain_or_subdomain(&next_host, &first_host);
            let mut next = Request::new(method, next_url);
            for (name, value) in &first.headers {
                let sensitive = ["Authorization", "Www-Authenticate", "Cookie", "Cookie2"]
                    .iter()
                    .any(|s| s.eq_ignore_ascii_case(name));
                if sensitive && !same_domain {
                    continue;
                }
                next.headers.push((name, value.clone()));
            }
            if target.scheme == Scheme::Https
                && parse_target(&next.url).is_some_and(|t| t.scheme == Scheme::Http)
            {
            } else if first.header("Referer").is_none() {
                let mut referer = target.origin();
                if let Some(at) = referer.iter().rposition(|&c| c == b'@') {
                    let scheme_end = referer.iter().position(|&c| c == b'/').unwrap_or(0) + 2;
                    referer.drain(scheme_end..=at);
                }
                referer.extend_from_slice(&target.request_uri);
                next.set_header("Referer", referer);
            }
            if include_body {
                next.body = first.body.clone();
            }
            req = next;
        }
    }
}

fn user_agent() -> String {
    format!("ElysianDB/{}", globals::version())
}

fn proxy_authorization(proxy: &Proxy) -> Option<Vec<u8>> {
    let (user, password) = proxy.target.user.as_ref()?;
    Some(basic_auth(user, password.as_deref().unwrap_or(b"")))
}

fn socks5_connect(
    conn: &mut Box<dyn Stream>,
    proxy: &Proxy,
    target: &Target,
) -> Result<(), String> {
    let io_err = |e: io::Error| match e.kind() {
        ErrorKind::UnexpectedEof => "connection closed by the server".to_string(),
        _ => io_reason(&e),
    };
    let auth = proxy.target.user.as_ref();
    let greeting: &[u8] = if auth.is_some() {
        &[5, 2, 0, 2]
    } else {
        &[5, 1, 0]
    };
    conn.write_all(greeting).map_err(io_err)?;
    let mut reply = [0u8; 2];
    conn.read_exact(&mut reply).map_err(io_err)?;
    if reply[0] != 5 {
        return Err("unexpected SOCKS version".to_string());
    }
    match reply[1] {
        0 => {}
        2 => {
            let (user, password) = auth.ok_or_else(|| "SOCKS authentication failed".to_string())?;
            let password = password.as_deref().unwrap_or(b"");
            if user.len() > 255 || password.len() > 255 {
                return Err("SOCKS authentication failed".to_string());
            }
            let mut msg = vec![1, user.len() as u8];
            msg.extend_from_slice(user);
            msg.push(password.len() as u8);
            msg.extend_from_slice(password);
            conn.write_all(&msg).map_err(io_err)?;
            let mut status = [0u8; 2];
            conn.read_exact(&mut status).map_err(io_err)?;
            if status[1] != 0 {
                return Err("SOCKS authentication failed".to_string());
            }
        }
        _ => return Err("no acceptable SOCKS authentication method".to_string()),
    }

    let mut msg = vec![5, 1, 0];
    match parse_ip(&target.host) {
        Some(IpAddr::V4(ip)) => {
            msg.push(1);
            msg.extend_from_slice(&ip.octets());
        }
        Some(IpAddr::V6(ip)) => {
            msg.push(4);
            msg.extend_from_slice(&ip.octets());
        }
        None => {
            if target.host.len() > 255 {
                return Err("invalid URL".to_string());
            }
            msg.push(3);
            msg.push(target.host.len() as u8);
            msg.extend_from_slice(&target.host);
        }
    }
    msg.extend_from_slice(&target.port.to_be_bytes());
    conn.write_all(&msg).map_err(io_err)?;
    let mut head = [0u8; 4];
    conn.read_exact(&mut head).map_err(io_err)?;
    if head[1] != 0 {
        return Err(format!("SOCKS connection failed with code {}", head[1]));
    }
    let skip = match head[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut len = [0u8; 1];
            conn.read_exact(&mut len).map_err(io_err)?;
            usize::from(len[0])
        }
        _ => return Err("unexpected SOCKS reply".to_string()),
    };
    let mut rest = vec![0u8; skip + 2];
    conn.read_exact(&mut rest).map_err(io_err)?;
    Ok(())
}

fn read_head_unbuffered(conn: &mut Box<dyn Stream>) -> Result<Vec<Vec<u8>>, String> {
    let mut data = Vec::new();
    let mut b = [0u8; 1];
    while !data.ends_with(b"\r\n\r\n") && !data.ends_with(b"\n\n") {
        if data.len() > MAX_HEADER_BYTES {
            return Err("the response head is too large".to_string());
        }
        match conn.read(&mut b) {
            Ok(0) => return Err("connection closed by the server".to_string()),
            Ok(_) => data.push(b[0]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(io_reason(&e)),
        }
    }
    Ok(split_head(&data))
}

fn split_head(data: &[u8]) -> Vec<Vec<u8>> {
    data.split(|&c| c == b'\n')
        .map(|l| l.strip_suffix(b"\r").unwrap_or(l).to_vec())
        .filter(|l| !l.is_empty())
        .collect()
}

fn read_head<R: BufRead>(r: &mut R) -> Result<Vec<Vec<u8>>, String> {
    let mut lines = Vec::new();
    let mut total = 0usize;
    loop {
        let mut line = Vec::new();
        let n = r.read_until(b'\n', &mut line).map_err(|e| match e.kind() {
            ErrorKind::UnexpectedEof => "connection closed by the server".to_string(),
            _ => io_reason(&e),
        })?;
        if n == 0 {
            return Err("connection closed by the server".to_string());
        }
        total += n;
        if total > MAX_HEADER_BYTES {
            return Err("the response head is too large".to_string());
        }
        let line = line
            .strip_suffix(b"\n")
            .map(|l| l.strip_suffix(b"\r").unwrap_or(l))
            .unwrap_or(&line)
            .to_vec();
        if line.is_empty() {
            if lines.is_empty() {
                continue;
            }
            return Ok(lines);
        }
        lines.push(line);
    }
}

fn parse_status_line(line: &[u8]) -> Result<(u16, Vec<u8>), String> {
    let malformed = || "malformed HTTP response".to_string();
    let sp = line.iter().position(|&c| c == b' ').ok_or_else(malformed)?;
    let proto = &line[..sp];
    if !proto.starts_with(b"HTTP/") {
        return Err(malformed());
    }
    let mut status = &line[sp + 1..];
    while let [b' ', rest @ ..] = status {
        status = rest;
    }
    let code_end = status
        .iter()
        .position(|&c| c == b' ')
        .unwrap_or(status.len());
    let code = &status[..code_end];
    if code.len() != 3 || !code.iter().all(u8::is_ascii_digit) {
        return Err(malformed());
    }
    let code = std::str::from_utf8(code)
        .ok()
        .and_then(|c| c.parse::<u16>().ok())
        .ok_or_else(malformed)?;
    Ok((code, status.to_vec()))
}

fn parse_headers(lines: &[Vec<u8>]) -> Vec<(Vec<u8>, Vec<u8>)> {
    let mut out: Vec<(Vec<u8>, Vec<u8>)> = Vec::new();
    for line in lines {
        if matches!(line.first(), Some(b' ' | b'\t')) {
            if let Some(last) = out.last_mut() {
                last.1.push(b' ');
                last.1.extend_from_slice(trim_space(line));
            }
            continue;
        }
        let Some(colon) = line.iter().position(|&c| c == b':') else {
            continue;
        };
        let name = trim_space(&line[..colon]).to_vec();
        let value = trim_space(&line[colon + 1..]).to_vec();
        out.push((name, value));
    }
    out
}

fn framing(method: &str, resp: &Response) -> Result<Framing, String> {
    if method == "HEAD"
        || (100..200).contains(&resp.status_code)
        || resp.status_code == 204
        || resp.status_code == 304
    {
        return Ok(Framing::Empty);
    }

    if let Some(te) = resp.header("Transfer-Encoding") {
        let last = te
            .split(|&c| c == b',')
            .map(trim_space)
            .filter(|c| !c.is_empty())
            .next_back();
        if last.is_some_and(|c| equal_fold(c, b"chunked")) {
            return Ok(Framing::Chunked {
                left: 0,
                done: false,
            });
        }
        return Ok(Framing::Close);
    }

    let mut length: Option<u64> = None;
    for v in resp.header_values("Content-Length") {
        for part in v.split(|&c| c == b',') {
            let part = trim_space(part);
            if part.is_empty() || !part.iter().all(u8::is_ascii_digit) {
                return Err("malformed HTTP response".to_string());
            }
            let n = std::str::from_utf8(part)
                .ok()
                .and_then(|s| s.parse::<u64>().ok())
                .ok_or_else(|| "malformed HTTP response".to_string())?;
            if length.is_some_and(|l| l != n) {
                return Err("malformed HTTP response".to_string());
            }
            length = Some(n);
        }
    }

    Ok(match length {
        Some(n) => Framing::Length(n),
        None => Framing::Close,
    })
}
