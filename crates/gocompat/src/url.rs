use std::fmt;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Url {
    pub scheme: Vec<u8>,
    pub opaque: Vec<u8>,
    pub has_user: bool,
    pub host: Vec<u8>,
    pub path: Vec<u8>,
    pub raw_query: Vec<u8>,
    pub force_query: bool,
    pub omit_host: bool,
    pub fragment: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    ControlCharacter,
    MissingScheme,
    ColonInFirstSegment,
    InvalidUserinfo,
    MissingBracket,
    InvalidPort(Vec<u8>),
    Escape(Vec<u8>),
    InvalidHost(Vec<u8>),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ControlCharacter => f.write_str("the URL contains a control character"),
            Error::MissingScheme => f.write_str("the URL has no scheme before ':'"),
            Error::ColonInFirstSegment => {
                f.write_str("the first path segment of a relative URL contains ':'")
            }
            Error::InvalidUserinfo => f.write_str("the URL has invalid user information"),
            Error::MissingBracket => f.write_str("the URL host has no closing ']'"),
            Error::InvalidPort(p) => write!(f, "invalid port {:?}", String::from_utf8_lossy(p)),
            Error::Escape(s) => write!(f, "invalid escape {:?}", String::from_utf8_lossy(s)),
            Error::InvalidHost(s) => write!(
                f,
                "invalid character {:?} in host",
                String::from_utf8_lossy(s)
            ),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Encoding {
    Path,
    Host,
    Zone,
    UserPassword,
    Fragment,
}

fn cut(s: &[u8], sep: u8) -> (&[u8], &[u8], bool) {
    match s.iter().position(|&b| b == sep) {
        Some(i) => (&s[..i], &s[i + 1..], true),
        None => (s, &[], false),
    }
}

fn index(s: &[u8], sep: &[u8]) -> Option<usize> {
    if sep.is_empty() {
        return Some(0);
    }
    s.windows(sep.len()).position(|w| w == sep)
}

fn last_index(s: &[u8], sep: u8) -> Option<usize> {
    s.iter().rposition(|&b| b == sep)
}

pub fn parse(raw_url: &[u8]) -> Result<Url, Error> {
    let (u, frag, _) = cut(raw_url, b'#');
    let mut url = parse_url(u, false)?;
    if frag.is_empty() {
        return Ok(url);
    }
    url.fragment = unescape(frag, Encoding::Fragment)?;
    Ok(url)
}

pub fn path_unescape(s: &[u8]) -> Result<Vec<u8>, Error> {
    unescape(s, Encoding::Path)
}

fn string_contains_ctl_byte(s: &[u8]) -> bool {
    s.iter().any(|&b| b < b' ' || b == 0x7f)
}

fn get_scheme(raw_url: &[u8]) -> Result<(&[u8], &[u8]), Error> {
    for (i, &c) in raw_url.iter().enumerate() {
        if c.is_ascii_alphabetic() {
            continue;
        }
        if c.is_ascii_digit() || c == b'+' || c == b'-' || c == b'.' {
            if i == 0 {
                return Ok((&[], raw_url));
            }
            continue;
        }
        if c == b':' {
            if i == 0 {
                return Err(Error::MissingScheme);
            }
            return Ok((&raw_url[..i], &raw_url[i + 1..]));
        }
        return Ok((&[], raw_url));
    }
    Ok((&[], raw_url))
}

fn parse_url(raw_url: &[u8], via_request: bool) -> Result<Url, Error> {
    if string_contains_ctl_byte(raw_url) {
        return Err(Error::ControlCharacter);
    }

    let mut url = Url::default();

    if raw_url == b"*" {
        url.path = b"*".to_vec();
        return Ok(url);
    }

    let (scheme, mut rest) = get_scheme(raw_url)?;
    url.scheme = scheme.to_ascii_lowercase();

    if rest.ends_with(b"?") && rest.iter().filter(|&&b| b == b'?').count() == 1 {
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
            return Err(Error::ColonInFirstSegment);
        }
    }

    if (!url.scheme.is_empty() || !via_request && !rest.starts_with(b"///"))
        && rest.starts_with(b"//")
    {
        let mut authority = &rest[2..];
        rest = &[];
        if let Some(i) = index(authority, b"/") {
            rest = &authority[i..];
            authority = &authority[..i];
        }
        let (has_user, host) = parse_authority(authority)?;
        url.has_user = has_user;
        url.host = host;
    } else if !url.scheme.is_empty() && rest.starts_with(b"/") {
        url.omit_host = true;
    }

    url.path = unescape(rest, Encoding::Path)?;
    Ok(url)
}

fn parse_authority(authority: &[u8]) -> Result<(bool, Vec<u8>), Error> {
    let at = last_index(authority, b'@');
    let host = match at {
        None => parse_host(authority)?,
        Some(i) => parse_host(&authority[i + 1..])?,
    };
    let Some(i) = at else {
        return Ok((false, host));
    };
    let userinfo = &authority[..i];
    if !valid_userinfo(userinfo) {
        return Err(Error::InvalidUserinfo);
    }
    if !userinfo.contains(&b':') {
        unescape(userinfo, Encoding::UserPassword)?;
    } else {
        let (username, password, _) = cut(userinfo, b':');
        unescape(username, Encoding::UserPassword)?;
        unescape(password, Encoding::UserPassword)?;
    }
    Ok((true, host))
}

fn parse_host(host: &[u8]) -> Result<Vec<u8>, Error> {
    if host.starts_with(b"[") {
        let Some(i) = last_index(host, b']') else {
            return Err(Error::MissingBracket);
        };
        let colon_port = &host[i + 1..];
        if !valid_optional_port(colon_port) {
            return Err(Error::InvalidPort(colon_port.to_vec()));
        }
        if let Some(zone) = index(&host[..i], b"%25") {
            let mut out = unescape(&host[..zone], Encoding::Host)?;
            out.extend(unescape(&host[zone..i], Encoding::Zone)?);
            out.extend(unescape(&host[i..], Encoding::Host)?);
            return Ok(out);
        }
    } else if let Some(i) = last_index(host, b':') {
        let colon_port = &host[i..];
        if !valid_optional_port(colon_port) {
            return Err(Error::InvalidPort(colon_port.to_vec()));
        }
    }

    unescape(host, Encoding::Host)
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

fn valid_userinfo(s: &[u8]) -> bool {
    let mut i = 0;
    while i < s.len() {
        let (r, width) = crate::utf8::decode_rune(&s[i..]);
        i += width;
        if (0x41..=0x5a).contains(&r) || (0x61..=0x7a).contains(&r) || (0x30..=0x39).contains(&r) {
            continue;
        }
        let allowed = u8::try_from(r).is_ok_and(|c| {
            matches!(
                c,
                b'-' | b'.'
                    | b'_'
                    | b':'
                    | b'~'
                    | b'!'
                    | b'$'
                    | b'&'
                    | b'\''
                    | b'('
                    | b')'
                    | b'*'
                    | b'+'
                    | b','
                    | b';'
                    | b'='
                    | b'%'
                    | b'@'
            )
        });
        if !allowed {
            return false;
        }
    }
    true
}

fn ishex(c: u8) -> bool {
    c.is_ascii_hexdigit()
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
            Encoding::Fragment => return false,
            Encoding::Host | Encoding::Zone => {}
        },
        _ => {}
    }

    if mode == Encoding::Fragment && matches!(c, b'!' | b'(' | b')' | b'*') {
        return false;
    }

    true
}

fn unescape(s: &[u8], mode: Encoding) -> Result<Vec<u8>, Error> {
    let mut n = 0;
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                n += 1;
                if i + 2 >= s.len() || !ishex(s[i + 1]) || !ishex(s[i + 2]) {
                    let tail = &s[i..];
                    let tail = if tail.len() > 3 { &tail[..3] } else { tail };
                    return Err(Error::Escape(tail.to_vec()));
                }
                if mode == Encoding::Host && unhex(s[i + 1]) < 8 && &s[i..i + 3] != b"%25" {
                    return Err(Error::Escape(s[i..i + 3].to_vec()));
                }
                if mode == Encoding::Zone {
                    let v = (unhex(s[i + 1]) << 4) | unhex(s[i + 2]);
                    if &s[i..i + 3] != b"%25" && v != b' ' && should_escape(v, Encoding::Host) {
                        return Err(Error::Escape(s[i..i + 3].to_vec()));
                    }
                }
                i += 3;
            }
            c => {
                if (mode == Encoding::Host || mode == Encoding::Zone)
                    && c < 0x80
                    && should_escape(c, mode)
                {
                    return Err(Error::InvalidHost(vec![c]));
                }
                i += 1;
            }
        }
    }

    if n == 0 {
        return Ok(s.to_vec());
    }

    let mut t = Vec::with_capacity(s.len() - 2 * n);
    let mut i = 0;
    while i < s.len() {
        match s[i] {
            b'%' => {
                t.push((unhex(s[i + 1]) << 4) | unhex(s[i + 2]));
                i += 2;
            }
            c => t.push(c),
        }
        i += 1;
    }
    Ok(t)
}
