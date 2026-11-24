use gocompat::json::quote_text;
use std::fmt;

const UPPERHEX: &[u8; 16] = b"0123456789ABCDEF";

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Escape(Vec<u8>),
    InvalidHost(Vec<u8>),
    ControlCharacter,
    MissingScheme,
    ColonInFirstSegment,
    InvalidPort(Vec<u8>),
    InvalidUserinfo,
    MissingBracket,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub op: &'static str,
    pub url: Vec<u8>,
    pub kind: ErrorKind,
}

impl fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ErrorKind::Escape(s) => write!(f, "invalid escape {}", quote_text(s)),
            ErrorKind::InvalidHost(s) => {
                write!(f, "invalid character {} in host name", quote_text(s))
            }
            ErrorKind::ControlCharacter => f.write_str("invalid control character"),
            ErrorKind::MissingScheme => f.write_str("missing protocol scheme"),
            ErrorKind::ColonInFirstSegment => {
                f.write_str("first path segment cannot contain a colon")
            }
            ErrorKind::InvalidPort(p) => write!(f, "invalid port {} after host", quote_text(p)),
            ErrorKind::InvalidUserinfo => f.write_str("invalid user information"),
            ErrorKind::MissingBracket => f.write_str("missing ']' in host"),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid URL {}: {}", quote_text(&self.url), self.kind)
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Path,
    Host,
    Zone,
    UserPassword,
    QueryComponent,
    Fragment,
}

fn ishex(c: u8) -> bool {
    c.is_ascii_digit() || (b'a'..=b'f').contains(&c) || (b'A'..=b'F').contains(&c)
}

fn unhex(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => 0,
    }
}

fn should_escape(c: u8, mode: Encoding) -> bool {
    if c.is_ascii_alphanumeric() {
        return false;
    }
    if (mode == Encoding::Host || mode == Encoding::Zone)
        && matches!(
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
        )
    {
        return false;
    }
    match c {
        b'-' | b'_' | b'.' | b'~' => return false,
        b'$' | b'&' | b'+' | b',' | b'/' | b':' | b';' | b'=' | b'?' | b'@' => match mode {
            Encoding::Path => return c == b'?',
            Encoding::UserPassword => return c == b'@' || c == b'/' || c == b'?' || c == b':',
            Encoding::QueryComponent => return true,
            Encoding::Fragment => return false,
            _ => {}
        },
        _ => {}
    }
    if mode == Encoding::Fragment && matches!(c, b'!' | b'(' | b')' | b'*') {
        return false;
    }
    true
}

fn unescape(s: &[u8], mode: Encoding) -> Result<Vec<u8>, ErrorKind> {
    let mut n = 0;
    let mut has_plus = false;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                n += 1;
                if i + 2 >= s.len() || !ishex(s[i + 1]) || !ishex(s[i + 2]) {
                    let mut rest = &s[i..];
                    if rest.len() > 3 {
                        rest = &rest[..3];
                    }
                    return Err(ErrorKind::Escape(rest.to_vec()));
                }
                if mode == Encoding::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                    return Err(ErrorKind::Escape(s[i..i + 3].to_vec()));
                }
                if mode == Encoding::Zone {
                    let v = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                    if &s[i..i + 3] != b"%25" && v != b' ' && should_escape(v, Encoding::Host) {
                        return Err(ErrorKind::Escape(s[i..i + 3].to_vec()));
                    }
                }
                i += 3;
            }
            b'+' => {
                has_plus = mode == Encoding::QueryComponent;
                i += 1;
            }
            _ => {
                if (mode == Encoding::Host || mode == Encoding::Zone)
                    && s[i] < 0x80
                    && should_escape(s[i], mode)
                {
                    return Err(ErrorKind::InvalidHost(s[i..i + 1].to_vec()));
                }
                i += 1;
            }
        }
    }
    if n == 0 && !has_plus {
        return Ok(s.to_vec());
    }
    let mut t = Vec::with_capacity(s.len().saturating_sub(2 * n));
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                t.push((unhex(s[i + 1]) << 4) | unhex(s[i + 2]));
                i += 2;
            }
            b'+' => {
                if mode == Encoding::QueryComponent {
                    t.push(b' ');
                } else {
                    t.push(b'+');
                }
            }
            c => t.push(c),
        }
        i += 1;
    }
    Ok(t)
}

fn escape(s: &[u8], mode: Encoding) -> Vec<u8> {
    let mut space_count = 0;
    let mut hex_count = 0;
    for &c in s {
        if should_escape(c, mode) {
            if c == b' ' && mode == Encoding::QueryComponent {
                space_count += 1;
            } else {
                hex_count += 1;
            }
        }
    }
    if space_count == 0 && hex_count == 0 {
        return s.to_vec();
    }
    if hex_count == 0 {
        return s
            .iter()
            .map(|&c| if c == b' ' { b'+' } else { c })
            .collect();
    }
    let mut t = Vec::with_capacity(s.len() + 2 * hex_count);
    for &c in s {
        if c == b' ' && mode == Encoding::QueryComponent {
            t.push(b'+');
        } else if should_escape(c, mode) {
            t.push(b'%');
            t.push(UPPERHEX[usize::from(c >> 4)]);
            t.push(UPPERHEX[usize::from(c & 15)]);
        } else {
            t.push(c);
        }
    }
    t
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Userinfo {
    username: Vec<u8>,
    password: Vec<u8>,
    password_set: bool,
}

impl Userinfo {
    pub fn username(&self) -> &[u8] {
        &self.username
    }

    pub fn password(&self) -> (&[u8], bool) {
        (&self.password, self.password_set)
    }

    pub fn string(&self) -> Vec<u8> {
        let mut s = escape(&self.username, Encoding::UserPassword);
        if self.password_set {
            s.push(b':');
            s.extend_from_slice(&escape(&self.password, Encoding::UserPassword));
        }
        s
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct Url {
    pub scheme: Vec<u8>,
    pub opaque: Vec<u8>,
    pub user: Option<Userinfo>,
    pub host: Vec<u8>,
    pub path: Vec<u8>,
    pub raw_path: Vec<u8>,
    pub omit_host: bool,
    pub force_query: bool,
    pub raw_query: Vec<u8>,
    pub fragment: Vec<u8>,
    pub raw_fragment: Vec<u8>,
}

fn get_scheme(raw_url: &[u8]) -> Result<(&[u8], &[u8]), ErrorKind> {
    for (i, &c) in raw_url.iter().enumerate() {
        if c.is_ascii_alphabetic() {
            continue;
        }
        if c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.' {
            if i == 0 {
                return Ok((&raw_url[..0], raw_url));
            }
            continue;
        }
        if c == b':' {
            if i == 0 {
                return Err(ErrorKind::MissingScheme);
            }
            return Ok((&raw_url[..i], &raw_url[i + 1..]));
        }
        return Ok((&raw_url[..0], raw_url));
    }
    Ok((&raw_url[..0], raw_url))
}

fn cut(s: &[u8], sep: u8) -> (&[u8], &[u8], bool) {
    match s.iter().position(|&c| c == sep) {
        Some(i) => (&s[..i], &s[i + 1..], true),
        None => (s, &s[..0], false),
    }
}

fn last_index(s: &[u8], c: u8) -> Option<usize> {
    s.iter().rposition(|&b| b == c)
}

fn find(s: &[u8], pat: &[u8]) -> Option<usize> {
    if pat.len() > s.len() {
        return None;
    }
    (0..=s.len() - pat.len()).find(|&i| &s[i..i + pat.len()] == pat)
}

fn string_contains_ctl_byte(s: &[u8]) -> bool {
    s.iter().any(|&b| b < b' ' || b == 0x7f)
}

pub fn parse(raw_url: &[u8]) -> Result<Url, Error> {
    let (u, frag, _) = cut(raw_url, b'#');
    let mut url = match parse_inner(u, false) {
        Ok(url) => url,
        Err(kind) => {
            return Err(Error {
                op: "parse",
                url: u.to_vec(),
                kind,
            });
        }
    };
    if frag.is_empty() {
        return Ok(url);
    }
    if let Err(kind) = url.set_fragment(frag) {
        return Err(Error {
            op: "parse",
            url: raw_url.to_vec(),
            kind,
        });
    }
    Ok(url)
}

fn parse_inner(raw_url: &[u8], via_request: bool) -> Result<Url, ErrorKind> {
    if string_contains_ctl_byte(raw_url) {
        return Err(ErrorKind::ControlCharacter);
    }
    let mut url = Url::default();
    if raw_url == b"*" {
        url.path = b"*".to_vec();
        return Ok(url);
    }
    let (scheme, mut rest) = get_scheme(raw_url)?;
    url.scheme = scheme.to_ascii_lowercase();
    if rest.ends_with(b"?") && rest.iter().filter(|&&c| c == b'?').count() == 1 {
        url.force_query = true;
        rest = &rest[..rest.len() - 1];
    } else {
        let (r, q, _) = cut(rest, b'?');
        rest = r;
        url.raw_query = q.to_vec();
    }
    if !rest.starts_with(b"/") {
        if !url.scheme.is_empty() {
            url.opaque = rest.to_vec();
            return Ok(url);
        }
        let (segment, _, _) = cut(rest, b'/');
        if segment.contains(&b':') {
            return Err(ErrorKind::ColonInFirstSegment);
        }
    }
    let mut rest = rest.to_vec();
    if (!url.scheme.is_empty() || !via_request && !rest.starts_with(b"///"))
        && rest.starts_with(b"//")
    {
        let mut authority = rest[2..].to_vec();
        rest = Vec::new();
        if let Some(i) = authority.iter().position(|&c| c == b'/') {
            rest = authority[i..].to_vec();
            authority.truncate(i);
        }
        let (user, host) = parse_authority(&authority)?;
        url.user = user;
        url.host = host;
    } else if !url.scheme.is_empty() && rest.starts_with(b"/") {
        url.omit_host = true;
    }
    url.set_path(&rest)?;
    Ok(url)
}

fn parse_authority(authority: &[u8]) -> Result<(Option<Userinfo>, Vec<u8>), ErrorKind> {
    let i = last_index(authority, b'@');
    let host = match i {
        None => parse_host(authority)?,
        Some(i) => parse_host(&authority[i + 1..])?,
    };
    let Some(i) = i else {
        return Ok((None, host));
    };
    let userinfo = &authority[..i];
    if !valid_userinfo(userinfo) {
        return Err(ErrorKind::InvalidUserinfo);
    }
    let user = if !userinfo.contains(&b':') {
        Userinfo {
            username: unescape(userinfo, Encoding::UserPassword)?,
            password: Vec::new(),
            password_set: false,
        }
    } else {
        let (username, password, _) = cut(userinfo, b':');
        Userinfo {
            username: unescape(username, Encoding::UserPassword)?,
            password: unescape(password, Encoding::UserPassword)?,
            password_set: true,
        }
    };
    Ok((Some(user), host))
}

fn parse_host(host: &[u8]) -> Result<Vec<u8>, ErrorKind> {
    if host.starts_with(b"[") {
        let Some(i) = last_index(host, b']') else {
            return Err(ErrorKind::MissingBracket);
        };
        let colon_port = &host[i + 1..];
        if !valid_optional_port(colon_port) {
            return Err(ErrorKind::InvalidPort(colon_port.to_vec()));
        }
        if let Some(zone) = find(&host[..i], b"%25") {
            let mut out = unescape(&host[..zone], Encoding::Host)?;
            out.extend_from_slice(&unescape(&host[zone..i], Encoding::Zone)?);
            out.extend_from_slice(&unescape(&host[i..], Encoding::Host)?);
            return Ok(out);
        }
    } else if let Some(i) = last_index(host, b':') {
        let colon_port = &host[i..];
        if !valid_optional_port(colon_port) {
            return Err(ErrorKind::InvalidPort(colon_port.to_vec()));
        }
    }
    unescape(host, Encoding::Host)
}

fn valid_encoded(s: &[u8], mode: Encoding) -> bool {
    for &c in s {
        match c {
            b'!' | b'$' | b'&' | b'\'' | b'(' | b')' | b'*' | b'+' | b',' | b';' | b'=' | b':'
            | b'@' | b'[' | b']' | b'%' => {}
            _ => {
                if should_escape(c, mode) {
                    return false;
                }
            }
        }
    }
    true
}

fn valid_optional_port(port: &[u8]) -> bool {
    if port.is_empty() {
        return true;
    }
    if port[0] != b':' {
        return false;
    }
    for r in String::from_utf8_lossy(&port[1..]).chars() {
        if !r.is_ascii_digit() {
            return false;
        }
    }
    true
}

fn valid_userinfo(s: &[u8]) -> bool {
    for r in String::from_utf8_lossy(s).chars() {
        if r.is_ascii_alphanumeric() {
            continue;
        }
        match r {
            '-' | '.' | '_' | ':' | '~' | '!' | '$' | '&' | '\'' | '(' | ')' | '*' | '+' | ','
            | ';' | '=' | '%' | '@' => continue,
            _ => return false,
        }
    }
    true
}

pub fn resolve_path(base: &[u8], r#ref: &[u8]) -> Vec<u8> {
    let full: Vec<u8> = if r#ref.is_empty() {
        base.to_vec()
    } else if r#ref[0] != b'/' {
        let i = last_index(base, b'/').map(|i| i + 1).unwrap_or(0);
        let mut f = base[..i].to_vec();
        f.extend_from_slice(r#ref);
        f
    } else {
        r#ref.to_vec()
    };
    if full.is_empty() {
        return Vec::new();
    }
    let mut elem: &[u8] = b"";
    let mut dst: Vec<u8> = vec![b'/'];
    let mut first = true;
    let mut remaining: &[u8] = &full;
    let mut found = true;
    while found {
        let (e, r, f) = cut(remaining, b'/');
        elem = e;
        remaining = r;
        found = f;
        if elem == b"." {
            first = false;
            continue;
        }
        if elem == b".." {
            let s = dst[1..].to_vec();
            let index = last_index(&s, b'/');
            dst.clear();
            dst.push(b'/');
            match index {
                None => first = true,
                Some(index) => dst.extend_from_slice(&s[..index]),
            }
        } else {
            if !first {
                dst.push(b'/');
            }
            dst.extend_from_slice(elem);
            first = false;
        }
    }
    if elem == b"." || elem == b".." {
        dst.push(b'/');
    }
    if dst.len() > 1 && dst[1] == b'/' {
        dst.remove(0);
    }
    dst
}

impl Url {
    pub fn set_path(&mut self, p: &[u8]) -> Result<(), ErrorKind> {
        let path = unescape(p, Encoding::Path)?;
        let escp = escape(&path, Encoding::Path);
        self.raw_path = if p == escp.as_slice() {
            Vec::new()
        } else {
            p.to_vec()
        };
        self.path = path;
        Ok(())
    }

    fn set_fragment(&mut self, f: &[u8]) -> Result<(), ErrorKind> {
        let frag = unescape(f, Encoding::Fragment)?;
        let escf = escape(&frag, Encoding::Fragment);
        self.raw_fragment = if f == escf.as_slice() {
            Vec::new()
        } else {
            f.to_vec()
        };
        self.fragment = frag;
        Ok(())
    }

    pub fn escaped_path(&self) -> Vec<u8> {
        if !self.raw_path.is_empty()
            && valid_encoded(&self.raw_path, Encoding::Path)
            && let Ok(p) = unescape(&self.raw_path, Encoding::Path)
            && p == self.path
        {
            return self.raw_path.clone();
        }
        if self.path == b"*" {
            return b"*".to_vec();
        }
        escape(&self.path, Encoding::Path)
    }

    pub fn escaped_fragment(&self) -> Vec<u8> {
        if !self.raw_fragment.is_empty()
            && valid_encoded(&self.raw_fragment, Encoding::Fragment)
            && let Ok(f) = unescape(&self.raw_fragment, Encoding::Fragment)
            && f == self.fragment
        {
            return self.raw_fragment.clone();
        }
        escape(&self.fragment, Encoding::Fragment)
    }

    pub fn is_abs(&self) -> bool {
        !self.scheme.is_empty()
    }

    pub fn string_bytes(&self) -> Vec<u8> {
        let mut buf: Vec<u8> = Vec::new();
        if !self.scheme.is_empty() {
            buf.extend_from_slice(&self.scheme);
            buf.push(b':');
        }
        if !self.opaque.is_empty() {
            buf.extend_from_slice(&self.opaque);
        } else {
            if !self.scheme.is_empty() || !self.host.is_empty() || self.user.is_some() {
                let omit = self.omit_host && self.host.is_empty() && self.user.is_none();
                if !omit {
                    if !self.host.is_empty() || !self.path.is_empty() || self.user.is_some() {
                        buf.extend_from_slice(b"//");
                    }
                    if let Some(ui) = &self.user {
                        buf.extend_from_slice(&ui.string());
                        buf.push(b'@');
                    }
                    if !self.host.is_empty() {
                        buf.extend_from_slice(&escape(&self.host, Encoding::Host));
                    }
                }
            }
            let path = self.escaped_path();
            if !path.is_empty() && path[0] != b'/' && !self.host.is_empty() {
                buf.push(b'/');
            }
            if buf.is_empty() {
                let (segment, _, _) = cut(&path, b'/');
                if segment.contains(&b':') {
                    buf.extend_from_slice(b"./");
                }
            }
            buf.extend_from_slice(&path);
        }
        if self.force_query || !self.raw_query.is_empty() {
            buf.push(b'?');
            buf.extend_from_slice(&self.raw_query);
        }
        if !self.fragment.is_empty() {
            buf.push(b'#');
            buf.extend_from_slice(&self.escaped_fragment());
        }
        buf
    }

    pub fn string(&self) -> String {
        String::from_utf8_lossy(&self.string_bytes()).into_owned()
    }

    pub fn resolve_reference(&self, r#ref: &Url) -> Url {
        let mut url = r#ref.clone();
        if r#ref.scheme.is_empty() {
            url.scheme = self.scheme.clone();
        }
        if !r#ref.scheme.is_empty() || !r#ref.host.is_empty() || r#ref.user.is_some() {
            let _ = url.set_path(&resolve_path(&r#ref.escaped_path(), b""));
            return url;
        }
        if !r#ref.opaque.is_empty() {
            url.user = None;
            url.host = Vec::new();
            url.path = Vec::new();
            return url;
        }
        if r#ref.path.is_empty() && !r#ref.force_query && r#ref.raw_query.is_empty() {
            url.raw_query = self.raw_query.clone();
            if r#ref.fragment.is_empty() {
                url.fragment = self.fragment.clone();
                url.raw_fragment = self.raw_fragment.clone();
            }
        }
        if r#ref.path.is_empty() && !self.opaque.is_empty() {
            url.opaque = self.opaque.clone();
            url.user = None;
            url.host = Vec::new();
            url.path = Vec::new();
            return url;
        }
        url.host = self.host.clone();
        url.user = self.user.clone();
        let _ = url.set_path(&resolve_path(&self.escaped_path(), &r#ref.escaped_path()));
        url
    }
}
