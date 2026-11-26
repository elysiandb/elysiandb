use std::io::{self, BufRead, BufReader, ErrorKind, Write};
use std::net::TcpStream;

pub struct Response {
    pub status: u16,
    pub headers: Vec<(String, Vec<u8>)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&[u8]> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }

    pub fn header_values<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a [u8]> + 'a {
        self.headers
            .iter()
            .filter(move |(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_slice())
    }
}

pub enum Body<'a> {
    None,
    Sized(&'a [u8]),
    Chunked(&'a [u8]),
}

#[derive(Clone)]
pub struct Head {
    lines: Vec<(String, Vec<u8>)>,
}

impl Head {
    pub fn new() -> Head {
        Head { lines: Vec::new() }
    }

    pub fn set(&mut self, name: &str, value: impl AsRef<[u8]>) {
        let value = value.as_ref().to_vec();
        match self
            .lines
            .iter_mut()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
        {
            Some(line) => line.1 = value,
            None => self.lines.push((name.to_string(), value)),
        }
    }

    pub fn remove(&mut self, name: &str) -> Option<Vec<u8>> {
        let i = self
            .lines
            .iter()
            .position(|(n, _)| n.eq_ignore_ascii_case(name))?;
        Some(self.lines.remove(i).1)
    }

    pub fn lines(&self) -> &[(String, Vec<u8>)] {
        &self.lines
    }
}

fn line(out: &mut Vec<u8>, name: &str, value: &[u8]) {
    out.extend_from_slice(name.as_bytes());
    out.extend_from_slice(b": ");
    out.extend_from_slice(value);
    out.extend_from_slice(b"\r\n");
}

fn ignores_body(method: &str) -> bool {
    method == "GET" || method == "HEAD"
}

pub fn fasthttp_request(
    method: &str,
    host: &str,
    uri: &str,
    body: &[u8],
    head: &Head,
    cookies: &[(String, String)],
) -> Vec<u8> {
    let mut head = Head {
        lines: head.lines.clone(),
    };
    let user_agent = head.remove("User-Agent");
    let host = head
        .remove("Host")
        .unwrap_or_else(|| host.as_bytes().to_vec());
    let content_type = head.remove("Content-Type");
    head.remove("Content-Length");

    let mut out = Vec::new();
    out.extend_from_slice(method.as_bytes());
    out.push(b' ');
    out.extend_from_slice(uri.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\n");
    line(
        &mut out,
        "User-Agent",
        user_agent.as_deref().unwrap_or(b"fasthttp"),
    );
    line(&mut out, "Host", &host);
    match content_type {
        Some(ct) if !ct.is_empty() => line(&mut out, "Content-Type", &ct),
        _ if !ignores_body(method) => line(&mut out, "Content-Type", b"application/octet-stream"),
        _ => {}
    }
    let has_body = !body.is_empty() || !ignores_body(method);
    if has_body {
        line(
            &mut out,
            "Content-Length",
            body.len().to_string().as_bytes(),
        );
    }
    for (name, value) in head.lines() {
        line(&mut out, name, value);
    }
    if !cookies.is_empty() {
        let joined = cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ");
        line(&mut out, "Cookie", joined.as_bytes());
    }
    out.extend_from_slice(b"\r\n");
    if has_body {
        out.extend_from_slice(body);
    }
    out
}

pub fn go_request(method: &str, host: &str, uri: &str, body: &Body<'_>, head: &Head) -> Vec<u8> {
    let mut out = Vec::new();
    out.extend_from_slice(method.as_bytes());
    out.push(b' ');
    out.extend_from_slice(uri.as_bytes());
    out.extend_from_slice(b" HTTP/1.1\r\n");
    line(&mut out, "Host", host.as_bytes());
    line(&mut out, "User-Agent", b"Go-http-client/1.1");
    match body {
        Body::None if matches!(method, "POST" | "PUT" | "PATCH") => {
            line(&mut out, "Content-Length", b"0")
        }
        Body::None => {}
        Body::Sized(b) => line(&mut out, "Content-Length", b.len().to_string().as_bytes()),
        Body::Chunked(_) => line(&mut out, "Transfer-Encoding", b"chunked"),
    }
    let mut sorted: Vec<&(String, Vec<u8>)> = head.lines().iter().collect();
    sorted.sort_by(|a, b| a.0.cmp(&b.0));
    for (name, value) in sorted {
        line(&mut out, name, value);
    }
    line(&mut out, "Accept-Encoding", b"gzip");
    out.extend_from_slice(b"\r\n");
    match body {
        Body::None => {}
        Body::Sized(b) => out.extend_from_slice(b),
        Body::Chunked(b) => {
            for chunk in b.chunks(32 << 10) {
                out.extend_from_slice(format!("{:x}\r\n", chunk.len()).as_bytes());
                out.extend_from_slice(chunk);
                out.extend_from_slice(b"\r\n");
            }
            out.extend_from_slice(b"0\r\n\r\n");
        }
    }
    out
}

fn malformed(what: &str) -> io::Error {
    io::Error::new(
        ErrorKind::InvalidData,
        format!("malformed HTTP response: {what}"),
    )
}

fn read_line<R: BufRead>(r: &mut R) -> io::Result<Vec<u8>> {
    let mut buf = Vec::new();
    let n = r.read_until(b'\n', &mut buf)?;
    if n == 0 {
        return Err(io::Error::from(ErrorKind::UnexpectedEof));
    }
    if buf.last() != Some(&b'\n') {
        return Err(io::Error::from(ErrorKind::UnexpectedEof));
    }
    buf.pop();
    if buf.last() == Some(&b'\r') {
        buf.pop();
    }
    Ok(buf)
}

fn trim(b: &[u8]) -> &[u8] {
    let start = b
        .iter()
        .position(|c| !c.is_ascii_whitespace())
        .unwrap_or(b.len());
    let end = b
        .iter()
        .rposition(|c| !c.is_ascii_whitespace())
        .map_or(start, |i| i + 1);
    &b[start..end.max(start)]
}

fn parse_size(b: &[u8], radix: u32) -> io::Result<u64> {
    let s = std::str::from_utf8(trim(b)).map_err(|_| malformed("size"))?;
    let s = s.split(';').next().unwrap_or("").trim();
    u64::from_str_radix(s, radix).map_err(|_| malformed("size"))
}

pub fn read_response<R: BufRead>(r: &mut R, method: &str) -> io::Result<Response> {
    loop {
        let status_line = read_line(r)?;
        let text = String::from_utf8_lossy(&status_line).into_owned();
        let mut parts = text.splitn(3, ' ');
        let proto = parts.next().unwrap_or("");
        if !proto.starts_with("HTTP/") {
            return Err(malformed("status line"));
        }
        let status: u16 = parts
            .next()
            .and_then(|c| c.parse().ok())
            .ok_or_else(|| malformed("status code"))?;

        let mut headers = Vec::new();
        loop {
            let l = read_line(r)?;
            if l.is_empty() {
                break;
            }
            let Some(colon) = l.iter().position(|&c| c == b':') else {
                return Err(malformed("header line"));
            };
            let name = String::from_utf8_lossy(trim(&l[..colon])).into_owned();
            headers.push((name, trim(&l[colon + 1..]).to_vec()));
        }

        if (100..200).contains(&status) {
            continue;
        }

        let mut resp = Response {
            status,
            headers,
            body: Vec::new(),
        };
        if method == "HEAD" || status == 204 || status == 304 {
            return Ok(resp);
        }

        let chunked = resp
            .header("Transfer-Encoding")
            .is_some_and(|v| v.eq_ignore_ascii_case(b"chunked"));
        if chunked {
            loop {
                let size = parse_size(&read_line(r)?, 16)?;
                if size == 0 {
                    loop {
                        if read_line(r)?.is_empty() {
                            break;
                        }
                    }
                    return Ok(resp);
                }
                let start = resp.body.len();
                resp.body.resize(start + size as usize, 0);
                r.read_exact(&mut resp.body[start..])?;
                if !read_line(r)?.is_empty() {
                    return Err(malformed("chunk"));
                }
            }
        }

        match resp.header("Content-Length").map(<[u8]>::to_vec) {
            Some(cl) => {
                let n = parse_size(&cl, 10)? as usize;
                resp.body.resize(n, 0);
                r.read_exact(&mut resp.body)?;
            }
            None => {
                r.read_to_end(&mut resp.body)?;
            }
        }
        return Ok(resp);
    }
}

pub fn round_trip(mut stream: TcpStream, request: &[u8], method: &str) -> io::Result<Response> {
    stream.write_all(request)?;
    stream.flush()?;
    let mut reader = BufReader::with_capacity(64 << 10, stream);
    read_response(&mut reader, method)
}

#[derive(Default)]
pub struct Jar {
    cookies: Vec<(String, String)>,
}

impl Jar {
    pub fn store(&mut self, resp: &Response) {
        for raw in resp.header_values("Set-Cookie") {
            let text = String::from_utf8_lossy(raw).into_owned();
            let mut parts = text.split(';');
            let Some((name, value)) = parts.next().and_then(|p| p.split_once('=')) else {
                continue;
            };
            let name = name.trim().to_string();
            let value = value.trim().trim_matches('"').to_string();
            let mut expired = false;
            for attr in parts {
                let (k, v) = attr.split_once('=').unwrap_or((attr, ""));
                let (k, v) = (k.trim(), v.trim());
                if k.eq_ignore_ascii_case("max-age") && v.parse::<i64>().is_ok_and(|n| n <= 0) {
                    expired = true;
                }
            }
            self.cookies.retain(|(n, _)| *n != name);
            if !expired {
                self.cookies.push((name, value));
            }
        }
    }

    pub fn header(&self) -> String {
        self.cookies
            .iter()
            .map(|(n, v)| format!("{n}={v}"))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

pub struct Multipart {
    boundary: String,
    buf: Vec<u8>,
    parts: usize,
}

impl Multipart {
    pub fn new() -> Multipart {
        let mut b = [0u8; 30];
        let _ = elysiandb::crypto::rand::os_random(&mut b);
        let boundary = b.iter().map(|x| format!("{x:02x}")).collect();
        Multipart {
            boundary,
            buf: Vec::new(),
            parts: 0,
        }
    }

    fn open(&mut self, disposition: &str, content_type: Option<&str>) {
        if self.parts > 0 {
            self.buf.extend_from_slice(b"\r\n");
        }
        self.parts += 1;
        self.buf
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        self.buf
            .extend_from_slice(format!("Content-Disposition: {disposition}\r\n").as_bytes());
        if let Some(ct) = content_type {
            self.buf
                .extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
        }
        self.buf.extend_from_slice(b"\r\n");
    }

    pub fn file(&mut self, field: &str, file_name: &str, data: &[u8]) {
        self.open(
            &format!("form-data; name=\"{field}\"; filename=\"{file_name}\""),
            Some("application/octet-stream"),
        );
        self.buf.extend_from_slice(data);
    }

    pub fn field(&mut self, field: &str, value: &str) {
        self.open(&format!("form-data; name=\"{field}\""), None);
        self.buf.extend_from_slice(value.as_bytes());
    }

    pub fn content_type(&self) -> String {
        format!("multipart/form-data; boundary={}", self.boundary)
    }

    pub fn close(mut self) -> Vec<u8> {
        if self.parts > 0 {
            self.buf.extend_from_slice(b"\r\n");
        }
        self.buf
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        self.buf
    }
}
