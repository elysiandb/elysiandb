use std::cell::Cell;
use std::fmt;
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::os::fd::AsRawFd;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI32, Ordering};
use std::time::{Duration, Instant};

use super::bufio::BufReader;
use super::conn::{ConnHandle, ConnInner, ConnWriter, HijackConn, SockReader};
use super::ctx::{RequestCtx, next_conn_id};
use super::date::server_date;
use super::header::RequestHeader;
use super::listener::{Wait, WakePipe, configure_accepted, wait_readable};
use super::poller::Poller;
use super::request::{DEFAULT_MAX_REQUEST_BODY_SIZE, HeadError, RequestError, quiet_io};
use super::workers::pool;
use crate::kv;
use crate::log::{Logger, for_component};
use crate::sync::Mutex;

pub type Handler = Arc<dyn Fn(&mut RequestCtx) + Send + Sync>;
pub type HeaderReceived = fn(&mut RequestHeader) -> RequestConfig;

pub const DEFAULT_CONCURRENCY: usize = 256 * 1024;
pub const MSG_TOO_MANY_CONNECTIONS: &str = "too many connections";

const NEW_CONN_CLOSABLE_AFTER: Duration = Duration::from_secs(5);

fn http_log() -> Logger {
    for_component("http")
}

pub fn handler(f: impl Fn(&mut RequestCtx) + Send + Sync + 'static) -> Handler {
    Arc::new(f)
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RequestConfig {
    pub read_timeout: Option<Duration>,
    pub write_timeout: Option<Duration>,
    pub max_request_body_size: Option<usize>,
}

#[derive(Clone)]
pub struct ServerConfig {
    pub handler: Handler,
    pub header_received: Option<HeaderReceived>,
    pub name: String,
    pub concurrency: usize,
    pub read_buffer_size: usize,
    pub write_buffer_size: usize,
    pub read_timeout: Duration,
    pub write_timeout: Duration,
    pub idle_timeout: Duration,
    pub max_request_body_size: usize,
}

impl ServerConfig {
    pub fn new(handler: Handler) -> ServerConfig {
        ServerConfig {
            handler,
            header_received: None,
            name: String::new(),
            concurrency: DEFAULT_CONCURRENCY,
            read_buffer_size: 4096,
            write_buffer_size: 4096,
            read_timeout: Duration::ZERO,
            write_timeout: Duration::ZERO,
            idle_timeout: Duration::ZERO,
            max_request_body_size: DEFAULT_MAX_REQUEST_BODY_SIZE,
        }
    }
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("name", &self.name)
            .field("concurrency", &self.concurrency)
            .field("read_buffer_size", &self.read_buffer_size)
            .field("read_timeout", &self.read_timeout)
            .field("write_timeout", &self.write_timeout)
            .field("idle_timeout", &self.idle_timeout)
            .field("max_request_body_size", &self.max_request_body_size)
            .finish()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShutdownError {
    DeadlineExceeded,
}

impl fmt::Display for ShutdownError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ShutdownError::DeadlineExceeded => f.write_str("drain timeout expired"),
        }
    }
}

impl std::error::Error for ShutdownError {}

pub(crate) struct ConnState {
    pub(crate) inner: Arc<ConnInner>,
    leftover: Vec<u8>,
    request_num: u64,
    conn_id: u64,
    conn_time: Instant,
    previous_write_timeout: Duration,
}

pub(crate) struct ServerInner {
    cfg: ServerConfig,
    open: AtomicI32,
    stopping: AtomicBool,
    accept_running: AtomicBool,
    wake: Mutex<Option<Arc<WakePipe>>>,
    poller: Mutex<Option<Arc<Poller>>>,
    last_overflow_log: Mutex<Option<Instant>>,
}

#[derive(Clone)]
pub struct Server {
    inner: Arc<ServerInner>,
}

impl fmt::Debug for Server {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Server")
            .field("cfg", &self.inner.cfg)
            .finish()
    }
}

struct OpenGuard<'a> {
    server: &'a ServerInner,
    counted: Cell<bool>,
}

impl Drop for OpenGuard<'_> {
    fn drop(&mut self) {
        if self.counted.get() && std::thread::panicking() {
            self.server.open.fetch_sub(1, Ordering::SeqCst);
        }
    }
}

enum Outcome {
    Idle(ConnState),
    Close(ConnState),
    Detached,
}

impl Server {
    pub fn new(cfg: ServerConfig) -> Server {
        Server {
            inner: Arc::new(ServerInner {
                cfg,
                open: AtomicI32::new(0),
                stopping: AtomicBool::new(false),
                accept_running: AtomicBool::new(false),
                wake: Mutex::new(None),
                poller: Mutex::new(None),
                last_overflow_log: Mutex::new(None),
            }),
        }
    }

    pub fn config(&self) -> &ServerConfig {
        &self.inner.cfg
    }

    pub fn open_connections(&self) -> i32 {
        self.inner.open.load(Ordering::SeqCst)
    }

    pub fn serve(&self, listener: TcpListener) -> io::Result<()> {
        self.inner.serve(listener)
    }

    pub fn serve_connection(&self, stream: TcpStream, peer_addr: SocketAddr) -> io::Result<()> {
        self.inner.serve_connection(stream, peer_addr)
    }

    pub fn shutdown(&self) -> Result<(), ShutdownError> {
        self.inner.shutdown(None)
    }

    pub fn shutdown_with_timeout(&self, timeout: Duration) -> Result<(), ShutdownError> {
        self.inner.shutdown(Some(timeout))
    }
}

impl ServerInner {
    pub(crate) fn is_stopping(&self) -> bool {
        self.stopping.load(Ordering::SeqCst)
    }

    pub(crate) fn accept_done(&self) -> bool {
        !self.accept_running.load(Ordering::SeqCst)
    }

    fn concurrency(&self) -> usize {
        if self.cfg.concurrency == 0 {
            DEFAULT_CONCURRENCY
        } else {
            self.cfg.concurrency
        }
    }

    fn server_name(&self) -> &str {
        &self.cfg.name
    }

    fn serve(self: &Arc<Self>, listener: TcpListener) -> io::Result<()> {
        listener.set_nonblocking(true)?;
        let wake = Arc::new(WakePipe::new()?);
        let poller = Arc::new(Poller::new()?);
        *self.wake.lock() = Some(wake.clone());
        *self.poller.lock() = Some(poller.clone());
        self.accept_running.store(true, Ordering::SeqCst);
        if self.is_stopping() {
            self.accept_running.store(false, Ordering::SeqCst);
            return Ok(());
        }
        {
            let server = self.clone();
            let p = poller.clone();
            crate::sync::spawn("http-poller", move || p.run(&server));
        }
        let res = self.accept_loop(&listener, &wake);
        drop(listener);
        self.accept_running.store(false, Ordering::SeqCst);
        poller.wake();
        res
    }

    fn serve_connection(
        self: &Arc<Self>,
        stream: TcpStream,
        peer_addr: SocketAddr,
    ) -> io::Result<()> {
        if self.is_stopping() {
            return Err(io::Error::from(io::ErrorKind::NotConnected));
        }
        let mut current = self.poller.lock();
        if current.is_none() {
            let poller = Arc::new(Poller::new()?);
            *current = Some(poller.clone());
            let server = self.clone();
            crate::sync::spawn("http-poller", move || poller.run(&server));
        }
        drop(current);
        self.accepted(stream, peer_addr);
        Ok(())
    }

    fn accept_loop(self: &Arc<Self>, listener: &TcpListener, wake: &WakePipe) -> io::Result<()> {
        let fd = listener.as_raw_fd();
        loop {
            if self.is_stopping() {
                return Ok(());
            }
            match wait_readable(fd, wake, None)? {
                Wait::Woken => {
                    wake.drain();
                    if self.is_stopping() {
                        return Ok(());
                    }
                    continue;
                }
                Wait::Ready => {}
            }
            loop {
                match listener.accept() {
                    Ok((stream, remote)) => self.accepted(stream, remote),
                    Err(e) => match e.kind() {
                        io::ErrorKind::WouldBlock
                        | io::ErrorKind::Interrupted
                        | io::ErrorKind::ConnectionAborted => {
                            break;
                        }
                        io::ErrorKind::TimedOut => {
                            http_log().warn("http accept error", kv!["error" => e.to_string()]);
                            std::thread::sleep(Duration::from_secs(1));
                            break;
                        }
                        _ => {
                            if self.is_stopping() {
                                return Ok(());
                            }
                            http_log().warn("http accept error", kv!["error" => e.to_string()]);
                            return Err(e);
                        }
                    },
                }
            }
        }
    }

    fn accepted(self: &Arc<Self>, stream: std::net::TcpStream, remote: SocketAddr) {
        configure_accepted(&stream);
        let local = stream.local_addr().unwrap_or(remote);
        let open = self.open.fetch_add(1, Ordering::SeqCst) + 1;
        if usize::try_from(open).unwrap_or(0) > self.concurrency() {
            self.open.fetch_sub(1, Ordering::SeqCst);
            self.write_fast_error(&stream, 503, MSG_TOO_MANY_CONNECTIONS);
            drop(stream);
            let mut last = self.last_overflow_log.lock();
            if last.is_none_or(|t| t.elapsed() > Duration::from_secs(60)) {
                http_log().warn(
                    "http connection limit reached",
                    kv!["limit" => self.concurrency()],
                );
                *last = Some(Instant::now());
            }
            return;
        }
        let now = Instant::now();
        let st = ConnState {
            inner: Arc::new(ConnInner::new(stream, remote, local)),
            leftover: Vec::new(),
            request_num: 0,
            conn_id: next_conn_id(),
            conn_time: now,
            previous_write_timeout: Duration::ZERO,
        };
        self.park(st, None, now + NEW_CONN_CLOSABLE_AFTER);
    }

    fn write_fast_error(&self, mut stream: &std::net::TcpStream, status: u16, msg: &str) {
        let mut out = Vec::new();
        super::status::format_status_line(&mut out, b"HTTP/1.1", status, b"");
        out.extend_from_slice(b"Connection: close\r\n");
        if !self.server_name().is_empty() {
            out.extend_from_slice(b"Server: ");
            out.extend_from_slice(self.server_name().as_bytes());
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(b"Date: ");
        out.extend_from_slice(&server_date());
        out.extend_from_slice(b"\r\n");
        out.extend_from_slice(b"Content-Type: text/plain\r\n");
        out.extend_from_slice(format!("Content-Length: {}\r\n\r\n", msg.len()).as_bytes());
        out.extend_from_slice(msg.as_bytes());
        let _ = stream.write_all(&out);
    }

    fn park(self: &Arc<Self>, st: ConnState, deadline: Option<Instant>, closable_at: Instant) {
        let poller = self.poller.lock().clone();
        let Some(poller) = poller else {
            self.close_conn(st);
            return;
        };
        if let Err(st) = poller.register(st, deadline, closable_at) {
            self.close_conn(st);
        }
    }

    pub(crate) fn close_conn(&self, st: ConnState) {
        st.inner.close();
        drop(st);
        self.open.fetch_sub(1, Ordering::SeqCst);
    }

    pub(crate) fn dispatch(self: &Arc<Self>, st: ConnState) {
        let server = self.clone();
        pool().submit(Box::new(move || server.run_conn(st)));
    }

    fn run_conn(self: &Arc<Self>, st: ConnState) {
        let guard = OpenGuard {
            server: self,
            counted: Cell::new(true),
        };
        let outcome = self.serve_conn(st, &guard);
        guard.counted.set(false);
        match outcome {
            Outcome::Idle(st) => {
                if self.is_stopping() {
                    self.close_conn(st);
                    return;
                }
                let idle = self.cfg.idle_timeout;
                let idle = if idle.is_zero() {
                    self.cfg.read_timeout
                } else {
                    idle
                };
                let now = Instant::now();
                let deadline = if idle.is_zero() {
                    None
                } else {
                    Some(now + idle)
                };
                self.park(st, deadline, now);
            }
            Outcome::Close(st) => self.close_conn(st),
            Outcome::Detached => {}
        }
    }

    fn log_conn_error(&self, st: &ConnState, reason: &str) {
        http_log().warn(
            "http connection error",
            kv!["remote_addr" => st.inner.remote().ip().to_string(), "error" => reason],
        );
    }

    fn log_io_error(&self, st: &ConnState, e: &io::Error) {
        if !quiet_io(e) {
            self.log_conn_error(st, &e.to_string());
        }
    }

    fn write_error_response(&self, st: &ConnState, ctx: &mut RequestCtx, status: u16, body: &str) {
        ctx.error(body, status);
        if !self.server_name().is_empty() {
            ctx.response.header.set_server(self.server_name());
        }
        ctx.set_connection_close();
        let mut w = ConnWriter::new(st.inner.clone(), self.cfg.write_buffer_size);
        let _ = ctx.response.write(&mut w);
        ctx.response.reset();
        let _ = w.flush();
    }

    fn serve_conn(self: &Arc<Self>, mut st: ConnState, guard: &OpenGuard<'_>) -> Outcome {
        let name = self.server_name().to_string();
        let mut ctx = RequestCtx::new();
        ctx.attach(
            ConnHandle {
                inner: st.inner.clone(),
            },
            st.inner.remote(),
            st.inner.local(),
            st.conn_id,
            st.conn_time,
        );
        let read_buffer_size = self.cfg.read_buffer_size.max(16);
        loop {
            st.request_num += 1;
            let first_request = st.request_num == 1;

            if st.leftover.is_empty() {
                let mut b = [0u8; 1];
                st.inner.set_read_deadline(None);
                match st.inner.read_with_deadline(&mut b) {
                    Ok(1) => st.leftover.push(b[0]),
                    _ => return Outcome::Close(st),
                }
            }

            if !self.cfg.read_timeout.is_zero() {
                st.inner
                    .set_read_deadline(Some(Instant::now() + self.cfg.read_timeout));
            } else if !self.cfg.idle_timeout.is_zero() && !first_request {
                st.inner.set_read_deadline(None);
            }

            let leftover = std::mem::take(&mut st.leftover);
            let mut br = BufReader::with_buffer(
                read_buffer_size,
                SockReader {
                    inner: st.inner.clone(),
                },
                &leftover,
            );

            let mut max_body = self.cfg.max_request_body_size;
            if max_body == 0 {
                max_body = DEFAULT_MAX_REQUEST_BODY_SIZE;
            }
            let mut write_timeout = self.cfg.write_timeout;

            let mut result: Result<(), RequestError> = match ctx.request.read_header(&mut br, true)
            {
                Ok(()) => Ok(()),
                Err(e) => Err(HeadError::into_request_error(e, first_request)),
            };
            if result.is_ok() {
                if let Some(hook) = self.cfg.header_received {
                    let conf = hook(&mut ctx.request.header);
                    if let Some(rt) = conf.read_timeout.filter(|d| !d.is_zero()) {
                        st.inner.set_read_deadline(Some(Instant::now() + rt));
                    }
                    max_body = match conf.max_request_body_size.filter(|&m| m > 0) {
                        Some(m) => m,
                        None => {
                            if self.cfg.max_request_body_size > 0 {
                                self.cfg.max_request_body_size
                            } else {
                                DEFAULT_MAX_REQUEST_BODY_SIZE
                            }
                        }
                    };
                    write_timeout = conf
                        .write_timeout
                        .filter(|d| !d.is_zero())
                        .unwrap_or(self.cfg.write_timeout);
                }
                result = ctx.request.read_limit_body(&mut br, max_body, true);
            }

            if let Err(e) = result {
                return self.fail(st, &mut ctx, e);
            }

            if ctx.request.may_continue() {
                let mut w = ConnWriter::new(st.inner.clone(), self.cfg.write_buffer_size);
                let continued = w
                    .write_all(b"HTTP/1.1 100 Continue\r\n\r\n")
                    .and_then(|()| w.flush());
                if let Err(e) = continued {
                    self.log_io_error(&st, &e);
                    return Outcome::Close(st);
                }
                if let Err(e) = ctx.request.continue_read_body(&mut br, max_body, true) {
                    return self.fail(st, &mut ctx, e);
                }
            }

            let mut connection_close = ctx.request.header.connection_close();

            if !name.is_empty() {
                ctx.response.header.set_server(&name);
            }
            ctx.start(st.request_num);

            (self.cfg.handler)(&mut ctx);

            if ctx.is_head() {
                ctx.response.skip_body = true;
            }

            let (hijack_handler, hijack_no_response) = ctx.take_hijack();

            if !write_timeout.is_zero() {
                st.inner
                    .set_write_deadline(Some(Instant::now() + write_timeout));
                st.previous_write_timeout = write_timeout;
            } else if !st.previous_write_timeout.is_zero() {
                st.inner.set_write_deadline(None);
                st.previous_write_timeout = Duration::ZERO;
            }

            connection_close = connection_close || ctx.response.connection_close();
            if connection_close {
                ctx.response.header.set_connection_close();
            } else if !ctx.request.header.is_http11() {
                ctx.response
                    .header
                    .set_non_special(b"Connection", b"keep-alive");
            }

            if !name.is_empty() && ctx.response.header.server().is_empty() {
                ctx.response.header.set_server(&name);
            }

            if !hijack_no_response {
                let mut w = ConnWriter::new(st.inner.clone(), self.cfg.write_buffer_size);
                let written = ctx.response.write(&mut w).and_then(|()| w.flush());
                if let Err(e) = written {
                    self.log_io_error(&st, &e);
                    return Outcome::Close(st);
                }
                if connection_close {
                    return Outcome::Close(st);
                }
            }

            st.leftover = br.take_buffered();

            if let Some(h) = hijack_handler {
                let buffered = std::mem::take(&mut st.leftover);
                st.inner.set_read_deadline(None);
                st.inner.set_write_deadline(None);
                self.open.fetch_sub(1, Ordering::SeqCst);
                guard.counted.set(false);
                let mut hc = HijackConn::new(st.inner.clone(), buffered);
                h(&mut ctx, &mut hc);
                st.inner.close();
                return Outcome::Detached;
            }

            ctx.reset_for_next();

            if self.is_stopping() {
                return Outcome::Close(st);
            }

            if st.leftover.is_empty() {
                return Outcome::Idle(st);
            }
        }
    }

    fn fail(&self, st: ConnState, ctx: &mut RequestCtx, e: RequestError) -> Outcome {
        match e {
            RequestError::Silent => {}
            RequestError::Respond { status, body, log } => {
                self.write_error_response(&st, ctx, status, body);
                if let Some(reason) = log {
                    self.log_conn_error(&st, &reason);
                }
            }
        }
        Outcome::Close(st)
    }

    fn shutdown(self: &Arc<Self>, timeout: Option<Duration>) -> Result<(), ShutdownError> {
        self.stopping.store(true, Ordering::SeqCst);
        if let Some(w) = self.wake.lock().clone() {
            w.wake();
        }
        if let Some(p) = self.poller.lock().clone() {
            p.wake();
        }
        let deadline = timeout.map(|t| Instant::now() + t);
        loop {
            if self.open.load(Ordering::SeqCst) <= 0 && self.accept_done() {
                return Ok(());
            }
            let now = Instant::now();
            if let Some(d) = deadline
                && now >= d
            {
                return Err(ShutdownError::DeadlineExceeded);
            }
            let tick = Duration::from_millis(100);
            let wait = deadline.map_or(tick, |d| tick.min(d - now));
            std::thread::sleep(wait);
        }
    }
}
