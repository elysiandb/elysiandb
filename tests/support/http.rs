#![allow(dead_code)]

use std::collections::BTreeMap;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Mutex;
use std::time::Duration;

pub struct Response {
    pub status: u16,
    pub reason: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Response {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn content_type(&self) -> &str {
        self.header("Content-Type").unwrap_or("")
    }

    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.body).into_owned()
    }
}

pub struct Client {
    addr: SocketAddr,
    timeout: Duration,
    max_idle: usize,
    jar: bool,
    idle: Mutex<Vec<BufReader<TcpStream>>>,
    cookies: Mutex<BTreeMap<String, String>>,
}

enum Framing {
    Empty,
    Length(usize),
    Chunked,
    UntilClose,
}

struct Head {
    status: u16,
    reason: String,
    headers: Vec<(String, String)>,
}

fn invalid(what: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, what.to_string())
}

fn read_line(conn: &mut BufReader<TcpStream>) -> io::Result<String> {
    let mut line = Vec::new();
    conn.read_until(b'\n', &mut line)?;
    if line.last() != Some(&b'\n') {
        return Err(io::Error::new(
            io::ErrorKind::UnexpectedEof,
            "the connection was closed before the end of a line",
        ));
    }
    line.pop();
    if line.last() == Some(&b'\r') {
        line.pop();
    }
    Ok(String::from_utf8_lossy(&line).into_owned())
}

fn read_head(conn: &mut BufReader<TcpStream>) -> io::Result<Head> {
    let status_line = read_line(conn)?;
    let mut parts = status_line.splitn(3, ' ');
    let version = parts.next().unwrap_or("");
    if !version.starts_with("HTTP/1.") {
        return Err(invalid("malformed status line"));
    }
    let status = parts
        .next()
        .and_then(|s| s.parse::<u16>().ok())
        .ok_or_else(|| invalid("malformed status code"))?;
    let reason = parts.next().unwrap_or("").to_string();
    let mut headers = Vec::new();
    loop {
        let line = read_line(conn)?;
        if line.is_empty() {
            break;
        }
        let (name, value) = line
            .split_once(':')
            .ok_or_else(|| invalid("malformed header"))?;
        headers.push((name.trim().to_string(), value.trim().to_string()));
    }
    Ok(Head {
        status,
        reason,
        headers,
    })
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.as_str())
}

fn framing(method: &str, head: &Head) -> io::Result<Framing> {
    if method == "HEAD" || head.status / 100 == 1 || head.status == 204 || head.status == 304 {
        return Ok(Framing::Empty);
    }
    if header(&head.headers, "Transfer-Encoding").is_some_and(|v| {
        v.split(',')
            .any(|p| p.trim().eq_ignore_ascii_case("chunked"))
    }) {
        return Ok(Framing::Chunked);
    }
    if let Some(len) = header(&head.headers, "Content-Length") {
        let n = len
            .parse::<usize>()
            .map_err(|_| invalid("malformed content length"))?;
        return Ok(Framing::Length(n));
    }
    Ok(Framing::UntilClose)
}

fn read_limited(
    conn: &mut BufReader<TcpStream>,
    out: &mut Vec<u8>,
    want: usize,
    limit: usize,
) -> io::Result<bool> {
    let mut left = want;
    let mut buf = [0u8; 16 << 10];
    while left > 0 {
        if out.len() >= limit {
            return Ok(false);
        }
        let room = left.min(buf.len()).min(limit - out.len());
        let n = conn.read(&mut buf[..room])?;
        if n == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the connection was closed before the end of the body",
            ));
        }
        out.extend_from_slice(&buf[..n]);
        left -= n;
    }
    Ok(true)
}

fn read_body(
    conn: &mut BufReader<TcpStream>,
    framing: &Framing,
    limit: usize,
) -> io::Result<(Vec<u8>, bool)> {
    let mut out = Vec::new();
    match framing {
        Framing::Empty => Ok((out, true)),
        Framing::Length(n) => {
            let whole = read_limited(conn, &mut out, *n, limit)?;
            Ok((out, whole))
        }
        Framing::Chunked => loop {
            let line = read_line(conn)?;
            let size = line.split(';').next().unwrap_or("").trim();
            let n = usize::from_str_radix(size, 16).map_err(|_| invalid("malformed chunk size"))?;
            if n == 0 {
                loop {
                    if read_line(conn)?.is_empty() {
                        break;
                    }
                }
                return Ok((out, true));
            }
            if !read_limited(conn, &mut out, n, limit)? {
                return Ok((out, false));
            }
            if !read_line(conn)?.is_empty() {
                return Err(invalid("malformed chunk end"));
            }
        },
        Framing::UntilClose => {
            let mut buf = [0u8; 16 << 10];
            loop {
                if out.len() >= limit {
                    return Ok((out, false));
                }
                let room = buf.len().min(limit - out.len());
                let n = conn.read(&mut buf[..room])?;
                if n == 0 {
                    return Ok((out, false));
                }
                out.extend_from_slice(&buf[..n]);
            }
        }
    }
}

impl Client {
    pub fn new(addr: SocketAddr, timeout: Duration) -> Client {
        Client {
            addr,
            timeout,
            max_idle: 64,
            jar: true,
            idle: Mutex::new(Vec::new()),
            cookies: Mutex::new(BTreeMap::new()),
        }
    }

    pub fn without_cookies(addr: SocketAddr, timeout: Duration) -> Client {
        Client {
            jar: false,
            ..Client::new(addr, timeout)
        }
    }

    pub fn close_idle_connections(&self) {
        self.idle.lock().unwrap_or_else(|e| e.into_inner()).clear();
    }

    fn connect(&self) -> io::Result<BufReader<TcpStream>> {
        let conn = TcpStream::connect_timeout(&self.addr, self.timeout)?;
        conn.set_read_timeout(Some(self.timeout))?;
        conn.set_write_timeout(Some(self.timeout))?;
        conn.set_nodelay(true)?;
        Ok(BufReader::with_capacity(64 << 10, conn))
    }

    fn head(&self, method: &str, path: &str, headers: &[(&str, &str)], body: &[u8]) -> Vec<u8> {
        let mut head = format!("{method} {path} HTTP/1.1\r\nHost: {}\r\n", self.addr);
        for (name, value) in headers {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
        if !body.is_empty() || matches!(method, "POST" | "PUT" | "PATCH") {
            head.push_str(&format!("Content-Length: {}\r\n", body.len()));
        }
        let cookies = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        if !cookies.is_empty() {
            let line: Vec<String> = cookies.iter().map(|(n, v)| format!("{n}={v}")).collect();
            head.push_str(&format!("Cookie: {}\r\n", line.join("; ")));
        }
        head.push_str("\r\n");
        head.into_bytes()
    }

    fn remember_cookies(&self, headers: &[(String, String)]) {
        if !self.jar {
            return;
        }
        let mut cookies = self.cookies.lock().unwrap_or_else(|e| e.into_inner());
        for (name, value) in headers {
            if !name.eq_ignore_ascii_case("Set-Cookie") {
                continue;
            }
            let pair = value.split(';').next().unwrap_or("");
            if let Some((n, v)) = pair.split_once('=') {
                cookies.insert(n.trim().to_string(), v.trim().to_string());
            }
        }
    }

    fn exchange(
        &self,
        conn: &mut BufReader<TcpStream>,
        method: &str,
        head: &[u8],
        body: &[u8],
        limit: usize,
    ) -> Result<(Response, bool), (io::Error, bool)> {
        let sent = conn
            .get_mut()
            .write_all(head)
            .and_then(|()| conn.get_mut().write_all(body));
        let answer = loop {
            match read_head(conn) {
                Ok(h) if h.status / 100 == 1 => continue,
                Ok(h) => break h,
                Err(e) => {
                    let untouched = conn.buffer().is_empty()
                        && matches!(
                            e.kind(),
                            io::ErrorKind::UnexpectedEof
                                | io::ErrorKind::ConnectionReset
                                | io::ErrorKind::ConnectionAborted
                                | io::ErrorKind::BrokenPipe
                        );
                    return Err((sent.err().unwrap_or(e), untouched));
                }
            }
        };
        let framing = framing(method, &answer).map_err(|e| (e, false))?;
        let (data, whole) = read_body(conn, &framing, limit).map_err(|e| (e, false))?;
        let close =
            header(&answer.headers, "Connection").is_some_and(|v| v.eq_ignore_ascii_case("close"));
        let reusable = sent.is_ok() && whole && !close && !matches!(framing, Framing::UntilClose);
        self.remember_cookies(&answer.headers);
        Ok((
            Response {
                status: answer.status,
                reason: answer.reason,
                headers: answer.headers,
                body: data,
            },
            reusable,
        ))
    }

    pub fn request(
        &self,
        method: &str,
        path: &str,
        headers: &[(&str, &str)],
        body: &[u8],
        limit: usize,
    ) -> io::Result<Response> {
        let head = self.head(method, path, headers, body);
        loop {
            let pooled = self.idle.lock().unwrap_or_else(|e| e.into_inner()).pop();
            let reused = pooled.is_some();
            let mut conn = match pooled {
                Some(c) => c,
                None => self.connect()?,
            };
            match self.exchange(&mut conn, method, &head, body, limit) {
                Ok((resp, reusable)) => {
                    if reusable {
                        let mut idle = self.idle.lock().unwrap_or_else(|e| e.into_inner());
                        if idle.len() < self.max_idle {
                            idle.push(conn);
                        }
                    }
                    return Ok(resp);
                }
                Err((_, true)) if reused => continue,
                Err((e, _)) => return Err(e),
            }
        }
    }

    pub fn send_reading(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: &[u8],
        limit: usize,
    ) -> io::Result<Response> {
        match content_type {
            Some(ct) => self.request(method, path, &[("Content-Type", ct)], body, limit),
            None => self.request(method, path, &[], body, limit),
        }
    }

    pub fn send(
        &self,
        method: &str,
        path: &str,
        content_type: Option<&str>,
        body: &[u8],
    ) -> io::Result<Response> {
        self.send_reading(method, path, content_type, body, usize::MAX)
    }

    pub fn get(&self, path: &str) -> io::Result<Response> {
        self.send("GET", path, None, b"")
    }
}
