use std::cell::OnceCell;
use std::fmt;

use super::args::Args;
use super::bytesconv::{
    append_quoted_path, decode_arg_append_no_plus, hex2int, index_byte, index_of, last_index_byte,
    last_index_of, lowercase_bytes,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UriError {
    Invalid,
    Escape(Vec<u8>),
    InvalidHost(Vec<u8>),
    MissingBracket,
    InvalidPort(Vec<u8>),
}

impl fmt::Display for UriError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            UriError::Invalid => f.write_str("invalid uri"),
            UriError::Escape(s) => write!(f, "invalid URL escape {:?}", String::from_utf8_lossy(s)),
            UriError::InvalidHost(s) => write!(
                f,
                "invalid character {:?} in host name",
                String::from_utf8_lossy(s)
            ),
            UriError::MissingBracket => f.write_str("missing ']' in host"),
            UriError::InvalidPort(s) => write!(
                f,
                "invalid port {:?} after host",
                String::from_utf8_lossy(s)
            ),
        }
    }
}

impl std::error::Error for UriError {}

#[derive(Clone, Debug, Default)]
pub struct URI {
    query_args: OnceCell<Args>,
    path_original: Vec<u8>,
    scheme: Vec<u8>,
    path: Vec<u8>,
    query_string: Vec<u8>,
    hash: Vec<u8>,
    host: Vec<u8>,
    username: Vec<u8>,
    password: Vec<u8>,
    pub disable_path_normalizing: bool,
}

impl URI {
    pub fn new() -> URI {
        URI::default()
    }

    pub fn reset(&mut self) {
        self.path_original.clear();
        self.scheme.clear();
        self.path.clear();
        self.query_string.clear();
        self.hash.clear();
        self.username.clear();
        self.password.clear();
        self.host.clear();
        self.query_args = OnceCell::new();
        self.disable_path_normalizing = false;
    }

    pub fn copy_to(&self, dst: &mut URI) {
        *dst = self.clone();
    }

    pub fn hash(&self) -> &[u8] {
        &self.hash
    }

    pub fn set_hash(&mut self, hash: impl AsRef<[u8]>) {
        self.hash = hash.as_ref().to_vec();
    }

    pub fn username(&self) -> &[u8] {
        &self.username
    }

    pub fn password(&self) -> &[u8] {
        &self.password
    }

    pub fn query_string(&self) -> &[u8] {
        &self.query_string
    }

    pub fn set_query_string(&mut self, query_string: impl AsRef<[u8]>) {
        self.query_string = query_string.as_ref().to_vec();
        self.query_args = OnceCell::new();
    }

    pub fn path(&self) -> &[u8] {
        if self.path.is_empty() {
            b"/"
        } else {
            &self.path
        }
    }

    pub fn set_path(&mut self, path: impl AsRef<[u8]>) {
        self.path_original = path.as_ref().to_vec();
        self.path = normalize_path(&self.path_original);
    }

    pub fn path_original(&self) -> &[u8] {
        &self.path_original
    }

    pub fn scheme(&self) -> &[u8] {
        if self.scheme.is_empty() {
            b"http"
        } else {
            &self.scheme
        }
    }

    pub fn set_scheme(&mut self, scheme: impl AsRef<[u8]>) {
        self.scheme = scheme.as_ref().to_vec();
        lowercase_bytes(&mut self.scheme);
    }

    pub fn host(&self) -> &[u8] {
        &self.host
    }

    pub fn set_host(&mut self, host: impl AsRef<[u8]>) {
        self.host = host.as_ref().to_vec();
        lowercase_bytes(&mut self.host);
    }

    pub fn query_args(&self) -> &Args {
        self.query_args.get_or_init(|| {
            let mut a = Args::new();
            a.parse(&self.query_string);
            a
        })
    }

    pub fn query_args_mut(&mut self) -> &mut Args {
        self.query_args();
        match self.query_args.get_mut() {
            Some(a) => a,
            None => unreachable!("query args are initialized"),
        }
    }

    pub fn parse(&mut self, host: &[u8], uri: &[u8]) -> Result<(), UriError> {
        self.parse_tls(host, uri, false)
    }

    pub(crate) fn parse_tls(
        &mut self,
        host: &[u8],
        uri: &[u8],
        is_tls: bool,
    ) -> Result<(), UriError> {
        self.reset();

        if string_contains_ctl_byte(uri) {
            return Err(UriError::Invalid);
        }

        let mut host = host.to_vec();
        let mut uri = uri.to_vec();
        if host.is_empty() || index_of(&uri, b"://").is_some() {
            let (scheme, new_host, new_uri) = split_host_uri(&host, &uri);
            self.set_scheme(&scheme);
            host = new_host;
            uri = new_uri;
        }

        if is_tls {
            self.set_scheme(b"https");
        }

        if let Some(n) = index_byte(&host, b'@') {
            let auth = host[..n].to_vec();
            host = host[n + 1..].to_vec();
            if let Some(m) = index_byte(&auth, b':') {
                self.username = auth[..m].to_vec();
                self.password = auth[m + 1..].to_vec();
            } else {
                self.username = auth;
                self.password.clear();
            }
        }

        self.host.extend_from_slice(&host);
        let parsed_host = parse_host(&self.host)?;
        self.host = parsed_host;
        lowercase_bytes(&mut self.host);

        let b = &uri;
        let mut query_index = index_byte(b, b'?');
        let fragment_index = index_byte(b, b'#');
        if let (Some(f), Some(q)) = (fragment_index, query_index)
            && q > f
        {
            query_index = None;
        }

        match (query_index, fragment_index) {
            (None, None) => {
                self.path_original.extend_from_slice(b);
                self.path = normalize_path(&self.path_original);
            }
            (Some(q), f) => {
                self.path_original.extend_from_slice(&b[..q]);
                self.path = normalize_path(&self.path_original);
                match f {
                    None => self.query_string.extend_from_slice(&b[q + 1..]),
                    Some(f) => {
                        self.query_string.extend_from_slice(&b[q + 1..f]);
                        self.hash.extend_from_slice(&b[f + 1..]);
                    }
                }
            }
            (None, Some(f)) => {
                self.path_original.extend_from_slice(&b[..f]);
                self.path = normalize_path(&self.path_original);
                self.hash.extend_from_slice(&b[f + 1..]);
            }
        }
        Ok(())
    }

    pub fn request_uri(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        if self.disable_path_normalizing {
            dst.extend_from_slice(self.path_original());
        } else {
            append_quoted_path(&mut dst, self.path());
        }
        match self.query_args.get() {
            Some(a) if !a.is_empty() => {
                dst.push(b'?');
                a.append_bytes(&mut dst);
            }
            _ => {
                if !self.query_string.is_empty() {
                    dst.push(b'?');
                    dst.extend_from_slice(&self.query_string);
                }
            }
        }
        dst
    }

    pub fn last_path_segment(&self) -> &[u8] {
        let path = self.path();
        match last_index_byte(path, b'/') {
            None => path,
            Some(n) => &path[n + 1..],
        }
    }

    pub fn update(&mut self, new_uri: impl AsRef<[u8]>) {
        let new_uri = new_uri.as_ref();
        if new_uri.is_empty() {
            return;
        }

        if index_of(new_uri, b"//").is_some() {
            let scheme_original = self.scheme.clone();
            if self.parse(&[], new_uri).is_err() {
                return;
            }
            if !scheme_original.is_empty() && self.scheme.is_empty() {
                self.scheme = scheme_original;
            }
            return;
        }

        if new_uri[0] == b'/' {
            let mut buf = Vec::new();
            self.append_scheme_host(&mut buf);
            buf.extend_from_slice(new_uri);
            let _ = self.parse(&[], &buf);
            return;
        }

        match new_uri[0] {
            b'?' => self.set_query_string(&new_uri[1..]),
            b'#' => self.set_hash(&new_uri[1..]),
            _ => {
                let path = self.path().to_vec();
                let Some(n) = last_index_byte(&path, b'/') else {
                    panic!(
                        "BUG: path must contain at least one slash: {:?} {:?}",
                        String::from_utf8_lossy(&path),
                        String::from_utf8_lossy(new_uri)
                    );
                };
                let mut buf = Vec::new();
                self.append_scheme_host(&mut buf);
                append_quoted_path(&mut buf, &path[..n + 1]);
                buf.extend_from_slice(new_uri);
                let _ = self.parse(&[], &buf);
            }
        }
    }

    pub fn full_uri(&self) -> Vec<u8> {
        let mut dst = Vec::new();
        self.append_bytes(&mut dst);
        dst
    }

    pub fn append_bytes(&self, dst: &mut Vec<u8>) {
        self.append_scheme_host(dst);
        dst.extend_from_slice(&self.request_uri());
        if !self.hash.is_empty() {
            dst.push(b'#');
            dst.extend_from_slice(&self.hash);
        }
    }

    fn append_scheme_host(&self, dst: &mut Vec<u8>) {
        dst.extend_from_slice(self.scheme());
        dst.extend_from_slice(b"://");
        dst.extend_from_slice(self.host());
    }
}

impl fmt::Display for URI {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&String::from_utf8_lossy(&self.full_uri()))
    }
}

fn string_contains_ctl_byte(s: &[u8]) -> bool {
    s.iter().any(|&b| b < b' ' || b == 0x7f)
}

fn split_host_uri(host: &[u8], uri: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let Some(n) = index_of(uri, b"//") else {
        return (b"http".to_vec(), host.to_vec(), uri.to_vec());
    };
    let mut scheme = &uri[..n];
    if index_byte(scheme, b'/').is_some() {
        return (b"http".to_vec(), host.to_vec(), uri.to_vec());
    }
    if let Some((&b':', rest)) = scheme.split_last() {
        scheme = rest;
    }
    let uri = &uri[n + 2..];
    let mut n = index_byte(uri, b'/');
    if let Some(nq) = index_byte(uri, b'?')
        && n.is_none_or(|n| nq < n)
    {
        n = Some(nq);
    }
    if let Some(nh) = index_byte(uri, b'#')
        && n.is_none_or(|n| nh < n)
    {
        n = Some(nh);
    }
    match n {
        None => (scheme.to_vec(), uri.to_vec(), b"/".to_vec()),
        Some(n) => (scheme.to_vec(), uri[..n].to_vec(), uri[n..].to_vec()),
    }
}

fn parse_host(host: &[u8]) -> Result<Vec<u8>, UriError> {
    if host.first() == Some(&b'[') {
        let Some(i) = last_index_byte(host, b']') else {
            return Err(UriError::MissingBracket);
        };
        let colon_port = &host[i + 1..];
        if !valid_optional_port(colon_port) {
            return Err(UriError::InvalidPort(colon_port.to_vec()));
        }
        if let Some(zone) = index_of(&host[..i], b"%25") {
            let mut host1 = unescape(&host[..zone], Encoding::Host)?;
            let host2 = unescape(&host[zone..i], Encoding::Zone)?;
            let host3 = unescape(&host[i..], Encoding::Host)?;
            host1.extend_from_slice(&host2);
            host1.extend_from_slice(&host3);
            return Ok(host1);
        }
    } else if let Some(i) = last_index_byte(host, b':') {
        let colon_port = &host[i..];
        if !valid_optional_port(colon_port) {
            return Err(UriError::InvalidPort(colon_port.to_vec()));
        }
    }
    unescape(host, Encoding::Host)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Host,
    Zone,
}

fn unescape(s: &[u8], mode: Encoding) -> Result<Vec<u8>, UriError> {
    let mut n = 0;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                n += 1;
                if i + 2 >= s.len() || !ishex(s[i + 1]) || !ishex(s[i + 2]) {
                    let rest = &s[i..];
                    let rest = &rest[..rest.len().min(3)];
                    return Err(UriError::Escape(rest.to_vec()));
                }
                if mode == Encoding::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                    return Err(UriError::Escape(s[i..i + 3].to_vec()));
                }
                if mode == Encoding::Zone {
                    let v = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                    if &s[i..i + 3] != b"%25" && v != b' ' && should_escape(v, Encoding::Host) {
                        return Err(UriError::Escape(s[i..i + 3].to_vec()));
                    }
                }
                i += 3;
            }
            c => {
                if c < 0x80 && should_escape(c, mode) {
                    return Err(UriError::InvalidHost(vec![c]));
                }
                i += 1;
            }
        }
    }
    if n == 0 {
        return Ok(s.to_vec());
    }
    let mut t = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'%' {
            t.push((unhex(s[i + 1]) << 4) | unhex(s[i + 2]));
            i += 3;
        } else {
            t.push(s[i]);
            i += 1;
        }
    }
    Ok(t)
}

fn should_escape(c: u8, _mode: Encoding) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    if matches!(
        c,
        b'!' | b'$'
            | b'&'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'+'
            | b','
            | b';'
            | b'='
            | b':'
            | b'['
            | b']'
            | b'<'
            | b'>'
            | b'"'
    ) {
        return false;
    }
    !matches!(c, b'-' | b'_' | b'.' | b'~')
}

fn ishex(c: u8) -> bool {
    hex2int(c) < 16
}

fn unhex(c: u8) -> u8 {
    hex2int(c) & 15
}

fn valid_optional_port(port: &[u8]) -> bool {
    if port.is_empty() {
        return true;
    }
    if port[0] != b':' {
        return false;
    }
    port[1..].iter().all(|b| b.is_ascii_digit())
}

fn add_leading_slash(dst: &mut Vec<u8>, src: &[u8]) {
    if src.first() != Some(&b'/') {
        dst.push(b'/');
    }
}

pub fn normalize_path(src: &[u8]) -> Vec<u8> {
    let mut dst = Vec::with_capacity(src.len() + 1);
    add_leading_slash(&mut dst, src);
    decode_arg_append_no_plus(&mut dst, src);

    let mut b = dst;
    while let Some(n) = index_of(&b, b"//") {
        b.remove(n);
    }

    while let Some(n) = index_of(&b, b"/./") {
        b.drain(n..n + 2);
    }

    while let Some(n) = index_of(&b, b"/../") {
        let nn = last_index_byte(&b[..n], b'/').unwrap_or(0);
        let end = n + 3;
        b.drain(nn..end);
    }

    if let Some(n) = last_index_of(&b, b"/..")
        && n + 3 == b.len()
    {
        match last_index_byte(&b[..n], b'/') {
            None => return b"/".to_vec(),
            Some(nn) => b.truncate(nn + 1),
        }
    }

    b
}
