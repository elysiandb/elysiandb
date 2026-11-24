use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr, TcpStream};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use elysiandb::httpd::{RequestCtx, Server, ServerConfig, bind_tcp4, handler};
use elysiandb::log;
use elysiandb::sync::{Mutex, MutexGuard};

static SERIAL: Mutex<()> = Mutex::new(());

pub fn serial() -> MutexGuard<'static, ()> {
    SERIAL.lock()
}

#[derive(Clone, Default)]
pub struct LogBuffer(Arc<Mutex<Vec<u8>>>);

impl Write for LogBuffer {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0.lock().extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

impl LogBuffer {
    pub fn text(&self) -> String {
        String::from_utf8_lossy(&self.0.lock()).into_owned()
    }

    pub fn clear(&self) {
        self.0.lock().clear();
    }

    pub fn wait_for(&self, needle: &str) -> String {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let text = self.text();
            if text.contains(needle) || Instant::now() >= deadline {
                return text;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

pub struct LogCapture {
    pub buf: LogBuffer,
}

impl LogCapture {
    pub fn start() -> LogCapture {
        let buf = LogBuffer::default();
        let _ = log::configure(&log::Options::default());
        log::set_output(Some(Box::new(buf.clone())), false);
        LogCapture { buf }
    }
}

impl Drop for LogCapture {
    fn drop(&mut self) {
        log::set_output(None, false);
    }
}

pub struct TestServer {
    pub server: Server,
    pub addr: SocketAddr,
    thread: Option<JoinHandle<io::Result<()>>>,
}

impl TestServer {
    pub fn start(cfg: ServerConfig) -> TestServer {
        let ln = match bind_tcp4("127.0.0.1:0") {
            Ok(ln) => ln,
            Err(e) => panic!("bind: {e}"),
        };
        let addr = match ln.local_addr() {
            Ok(a) => a,
            Err(e) => panic!("local addr: {e}"),
        };
        let server = Server::new(cfg);
        let s = server.clone();
        let thread = std::thread::spawn(move || s.serve(ln));
        TestServer {
            server,
            addr,
            thread: Some(thread),
        }
    }

    pub fn connect(&self) -> TcpStream {
        match TcpStream::connect(self.addr) {
            Ok(c) => c,
            Err(e) => panic!("connect: {e}"),
        }
    }

    pub fn stop(&mut self) {
        let _ = self.server.shutdown_with_timeout(Duration::from_secs(5));
        if let Some(t) = self.thread.take() {
            let _ = t.join();
        }
    }
}

impl Drop for TestServer {
    fn drop(&mut self) {
        self.stop();
    }
}

pub fn elysian_config(h: impl Fn(&mut RequestCtx) + Send + Sync + 'static) -> ServerConfig {
    let mut cfg = ServerConfig::new(handler(h));
    cfg.name = "ElysianDB".to_string();
    cfg.concurrency = 100_000;
    cfg.read_buffer_size = 64 << 10;
    cfg.write_buffer_size = 64 << 10;
    cfg.read_timeout = Duration::from_secs(3);
    cfg.write_timeout = Duration::from_secs(3);
    cfg.idle_timeout = Duration::from_secs(30);
    cfg
}

pub enum Until {
    Close(Duration),
    Quiet(Duration, Duration),
}

pub fn read_response(conn: &mut TcpStream, until: Until) -> (Vec<u8>, bool) {
    let mut out = Vec::new();
    let (max, quiet) = match until {
        Until::Close(max) => (max, None),
        Until::Quiet(q, max) => (max, Some(q)),
    };
    let deadline = Instant::now() + max;
    let mut buf = vec![0u8; 64 << 10];
    loop {
        let now = Instant::now();
        if now >= deadline {
            return (out, false);
        }
        let mut wait = deadline - now;
        if let Some(q) = quiet {
            wait = wait.min(q);
        }
        let _ = conn.set_read_timeout(Some(wait.max(Duration::from_millis(1))));
        match conn.read(&mut buf) {
            Ok(0) => return (out, true),
            Ok(n) => out.extend_from_slice(&buf[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) =>
            {
                if quiet.is_some() {
                    return (out, false);
                }
            }
            Err(e) if e.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return (out, true),
        }
    }
}

pub fn roundtrip(ts: &TestServer, request: &[u8]) -> (Vec<u8>, bool) {
    let mut c = ts.connect();
    let _ = c.write_all(request);
    read_response(&mut c, Until::Close(Duration::from_secs(10)))
}

pub fn roundtrip_half_close(ts: &TestServer, request: &[u8]) -> (Vec<u8>, bool) {
    let mut c = ts.connect();
    let _ = c.write_all(request);
    let _ = c.shutdown(Shutdown::Write);
    read_response(&mut c, Until::Close(Duration::from_secs(10)))
}

pub fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

pub fn mask_date(b: &[u8]) -> String {
    let s = text(b);
    let mut out = String::new();
    for line in s.split_inclusive("\r\n") {
        if line.starts_with("Date: ") {
            out.push_str("Date: <DATE>\r\n");
        } else {
            out.push_str(line);
        }
    }
    out
}

pub struct Parsed {
    pub status: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

impl Parsed {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }

    pub fn names(&self) -> Vec<&str> {
        self.headers.iter().map(|(k, _)| k.as_str()).collect()
    }
}

pub fn parse_responses(raw: &[u8]) -> Vec<Parsed> {
    let mut out = Vec::new();
    let mut rest = raw;
    while !rest.is_empty() {
        let Some(end) = rest.windows(4).position(|w| w == b"\r\n\r\n") else {
            break;
        };
        let head = text(&rest[..end]);
        rest = &rest[end + 4..];
        let mut lines = head.split("\r\n");
        let status = lines.next().unwrap_or("").to_string();
        let mut headers = Vec::new();
        for l in lines {
            if let Some((k, v)) = l.split_once(':') {
                headers.push((k.to_string(), v.trim().to_string()));
            }
        }
        let code: u16 = status
            .split(' ')
            .nth(1)
            .and_then(|c| c.parse().ok())
            .unwrap_or(0);
        let chunked = headers.iter().any(|(k, v)| {
            k.eq_ignore_ascii_case("Transfer-Encoding") && v.eq_ignore_ascii_case("chunked")
        });
        let length: Option<usize> = headers
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case("Content-Length"))
            .and_then(|(_, v)| v.parse().ok());
        let mut body = Vec::new();
        let bodiless = code == 100 || code == 204 || code == 304;
        if bodiless {
            body.clear();
        } else if chunked {
            let (decoded, consumed) = decode_chunked(rest);
            body = decoded;
            rest = &rest[consumed..];
        } else if let Some(n) = length {
            let n = n.min(rest.len());
            body = rest[..n].to_vec();
            rest = &rest[n..];
        } else {
            body = rest.to_vec();
            rest = &[];
        }
        out.push(Parsed {
            status,
            headers,
            body,
        });
    }
    out
}

pub fn decode_chunked(b: &[u8]) -> (Vec<u8>, usize) {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let Some(eol) = b[pos..].windows(2).position(|w| w == b"\r\n") else {
            return (out, b.len());
        };
        let size_text = text(&b[pos..pos + eol]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .unwrap_or(0);
        pos += eol + 2;
        if size == 0 {
            if b[pos..].starts_with(b"\r\n") {
                pos += 2;
            }
            return (out, pos);
        }
        let end = (pos + size).min(b.len());
        out.extend_from_slice(&b[pos..end]);
        pos = (end + 2).min(b.len());
    }
}
