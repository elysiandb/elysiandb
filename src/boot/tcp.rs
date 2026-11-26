use std::cell::Cell;
use std::collections::HashMap;
use std::fmt;
use std::io::{self, ErrorKind, Write};
use std::net::{
    IpAddr, Ipv4Addr, Ipv6Addr, Shutdown, SocketAddr, TcpListener, TcpStream, ToSocketAddrs,
};
use std::os::fd::{AsRawFd, FromRawFd, RawFd};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::{Duration as StdDuration, Instant};

use crate::globals;
use crate::gocompat::json::quote_text;
use crate::gocompat::time::Duration;
use crate::httpd::{
    Host, Parked, Poller, Pool, TCP_KEEPIDLE_OPTION, Wait, WakePipe, somaxconn, wait_readable,
};
use crate::kv;
use crate::log::{self, Level, Logger, Throttle};
use crate::metrics;
use crate::security;
use crate::sync::{Mutex, WaitGroup, spawn};
use crate::transport::tcp::tcp_routing::{self, Conn, Session};

use super::init::config;
use super::server::STOPPING;

const ACCEPT_BACKOFF_MIN: Duration = Duration(5 * Duration::MILLISECOND.0);
const ACCEPT_BACKOFF_MAX: Duration = Duration::SECOND;
const UNAUTHENTICATED_READ_TIMEOUT: StdDuration = StdDuration::from_secs(30);
const BUFFER_SIZE: usize = 128 << 10;
const SOCKET_BUFFER: libc::c_int = 256 << 10;
const KEEP_ALIVE_IDLE: libc::c_int = 120;
const KEEP_ALIVE_INTERVAL: libc::c_int = 15;
const KEEP_ALIVE_COUNT: libc::c_int = 9;
const WORKER_STACK: usize = 512 << 10;

pub const LINE_TOO_LONG: &str = "line longer than 131072 bytes";
pub const CLOSED_BY_CLIENT: &str = "connection closed by the client";
pub const READ_TIMED_OUT: &str = "read timed out";
pub const CONNECTION_CLOSED: &str = "connection closed";

const TCP_LOG: Logger = log::for_component("tcp");
static ACCEPT_THROTTLE: Throttle = Throttle::new(StdDuration::from_secs(60));
static SLOW_THROTTLE: Throttle = Throttle::new(StdDuration::from_secs(60));

struct Listening {
    wake: WakePipe,
    closed: AtomicBool,
    accepting: AtomicBool,
    local: SocketAddr,
    poller: Arc<Poller<Connection>>,
}

static LISTENER: Mutex<Option<Arc<Listening>>> = Mutex::new(None);
static CONNS: WaitGroup = WaitGroup::new();
static OPEN: Mutex<Option<HashMap<u64, Arc<TcpStream>>>> = Mutex::new(None);
static NEXT_CONN: AtomicU64 = AtomicU64::new(0);
static WORKERS: OnceLock<Pool> = OnceLock::new();

fn workers() -> &'static Pool {
    WORKERS.get_or_init(|| Pool::new("tcp-worker", WORKER_STACK))
}

#[derive(Debug)]
pub enum AddrError {
    Invalid(String),
    Unresolvable(String),
    NotIpv4(String),
    Listen(String, io::Error),
}

impl fmt::Display for AddrError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AddrError::Invalid(addr) => {
                write!(f, "{} is not a valid address", quote_text(addr.as_bytes()))
            }
            AddrError::Unresolvable(host) => {
                write!(f, "cannot resolve {}", quote_text(host.as_bytes()))
            }
            AddrError::NotIpv4(addr) => write!(f, "cannot listen on {addr}: not an IPv4 address"),
            AddrError::Listen(addr, err) if err.kind() == ErrorKind::AddrInUse => {
                write!(f, "cannot listen on {addr}: address already in use")
            }
            AddrError::Listen(addr, err) => write!(f, "cannot listen on {addr}: {err}"),
        }
    }
}

impl std::error::Error for AddrError {}

pub fn split_host_port(hostport: &str) -> Option<(&str, &str)> {
    let b = hostport.as_bytes();
    let i = b.iter().rposition(|&c| c == b':')?;
    let (host, j, k) = if b.first() == Some(&b'[') {
        let end = b.iter().position(|&c| c == b']')?;
        if end + 1 != i {
            return None;
        }
        (&hostport[1..end], 1, end + 1)
    } else {
        let host = &hostport[..i];
        if host.contains(':') {
            return None;
        }
        (host, 0, 0)
    };
    if hostport[j..].contains('[') || hostport[k..].contains(']') {
        return None;
    }

    Some((host, &hostport[i + 1..]))
}

fn parse_port(service: &str) -> Option<u16> {
    let (neg, digits) = match service.as_bytes().first() {
        Some(b'+') => (false, &service[1..]),
        Some(b'-') => (true, &service[1..]),
        _ => (false, service),
    };
    if service.is_empty() {
        return Some(0);
    }
    if digits.is_empty() || !digits.bytes().all(|c| c.is_ascii_digit()) {
        return None;
    }
    if neg {
        return digits.bytes().all(|c| c == b'0').then_some(0);
    }
    digits.parse::<u16>().ok()
}

pub fn resolve_tcp_addr(addr: &str) -> Result<(Option<IpAddr>, u16), AddrError> {
    let invalid = || AddrError::Invalid(addr.to_string());
    let (host, port) = split_host_port(addr).ok_or_else(invalid)?;
    let port = parse_port(port).ok_or_else(invalid)?;

    if host.is_empty() {
        return Ok((None, port));
    }

    if let Ok(ip) = host.parse::<IpAddr>() {
        return Ok((Some(ip), port));
    }

    let addrs: Vec<SocketAddr> = (host, port)
        .to_socket_addrs()
        .map_err(|_| AddrError::Unresolvable(host.to_string()))?
        .collect();
    let pick = addrs
        .iter()
        .find(|a| a.is_ipv4())
        .or_else(|| addrs.first())
        .ok_or_else(|| AddrError::Unresolvable(host.to_string()))?;

    Ok((Some(pick.ip()), port))
}

pub(crate) fn go_addr(ip: Option<IpAddr>, port: u16) -> String {
    match ip {
        None => format!(":{port}"),
        Some(IpAddr::V4(v4)) => format!("{v4}:{port}"),
        Some(IpAddr::V6(v6)) => format!("[{v6}]:{port}"),
    }
}

fn setsockopt(
    fd: RawFd,
    level: libc::c_int,
    name: libc::c_int,
    value: libc::c_int,
) -> io::Result<()> {
    let rc = unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (&raw const value).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        )
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

fn socket(family: libc::c_int) -> io::Result<RawFd> {
    let fd = unsafe { libc::socket(family, libc::SOCK_STREAM, 0) };
    if fd < 0 {
        return Err(io::Error::last_os_error());
    }
    unsafe {
        libc::fcntl(fd, libc::F_SETFD, libc::FD_CLOEXEC);
        let flags = libc::fcntl(fd, libc::F_GETFL);
        libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
    }
    Ok(fd)
}

fn bind_listen(fd: RawFd, sa: &SocketAddr) -> io::Result<()> {
    setsockopt(fd, libc::SOL_SOCKET, libc::SO_REUSEADDR, 1)?;
    let rc = match sa {
        SocketAddr::V4(v4) => {
            let mut sin: libc::sockaddr_in = unsafe { std::mem::zeroed() };
            sin.sin_family = libc::AF_INET as libc::sa_family_t;
            sin.sin_port = v4.port().to_be();
            sin.sin_addr = libc::in_addr {
                s_addr: u32::from_ne_bytes(v4.ip().octets()),
            };
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
            {
                sin.sin_len = std::mem::size_of::<libc::sockaddr_in>() as u8;
            }
            unsafe {
                libc::bind(
                    fd,
                    (&raw const sin).cast::<libc::sockaddr>(),
                    std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t,
                )
            }
        }
        SocketAddr::V6(v6) => {
            setsockopt(fd, libc::IPPROTO_IPV6, libc::IPV6_V6ONLY, 0)?;
            let mut sin6: libc::sockaddr_in6 = unsafe { std::mem::zeroed() };
            sin6.sin6_family = libc::AF_INET6 as libc::sa_family_t;
            sin6.sin6_port = v6.port().to_be();
            sin6.sin6_addr = libc::in6_addr {
                s6_addr: v6.ip().octets(),
            };
            sin6.sin6_scope_id = v6.scope_id();
            #[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
            {
                sin6.sin6_len = std::mem::size_of::<libc::sockaddr_in6>() as u8;
            }
            unsafe {
                libc::bind(
                    fd,
                    (&raw const sin6).cast::<libc::sockaddr>(),
                    std::mem::size_of::<libc::sockaddr_in6>() as libc::socklen_t,
                )
            }
        }
    };
    if rc < 0 {
        return Err(io::Error::last_os_error());
    }
    if unsafe { libc::listen(fd, somaxconn()) } < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

pub fn listen_tcp(ip: Option<IpAddr>, port: u16) -> Result<TcpListener, AddrError> {
    let shown = go_addr(ip, port);
    let fail = |err: io::Error| AddrError::Listen(shown.clone(), err);
    let wildcard = ip.is_none_or(|ip| ip.is_unspecified());

    let target = match ip {
        _ if wildcard => SocketAddr::from((Ipv6Addr::UNSPECIFIED, port)),
        Some(ip) => SocketAddr::from((ip, port)),
        None => SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
    };

    let family = if target.is_ipv4() {
        libc::AF_INET
    } else {
        libc::AF_INET6
    };
    let (fd, target) = match socket(family) {
        Ok(fd) => (fd, target),
        Err(e) if wildcard && e.raw_os_error() == Some(libc::EAFNOSUPPORT) => (
            socket(libc::AF_INET).map_err(fail)?,
            SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)),
        ),
        Err(e) => return Err(fail(e)),
    };
    let listener = unsafe { TcpListener::from_raw_fd(fd) };
    bind_listen(fd, &target).map_err(fail)?;

    Ok(listener)
}

pub fn init_tcp() {
    security::register();
    let cfg = config();
    let addr = format!("{}:{}", cfg.server.tcp.host, cfg.server.tcp.port);

    let (ip, port) = match resolve_tcp_addr(&addr) {
        Ok(resolved) => resolved,
        Err(err) => log::fatal(
            "cannot resolve the TCP address",
            kv!["component" => "tcp", "addr" => addr, "err" => log::display(&err)],
        ),
    };

    let listener = match listen_tcp(ip, port) {
        Ok(l) => l,
        Err(err) => log::fatal(
            "cannot start the TCP server",
            kv!["component" => "tcp", "addr" => addr, "err" => log::display(&err)],
        ),
    };

    let wake = match WakePipe::new() {
        Ok(w) => w,
        Err(err) => log::fatal(
            "cannot start the TCP server",
            kv!["component" => "tcp", "addr" => addr, "err" => log::display(&err)],
        ),
    };
    let poller = match Poller::new() {
        Ok(p) => Arc::new(p),
        Err(err) => log::fatal(
            "cannot start the TCP server",
            kv!["component" => "tcp", "addr" => addr, "err" => log::display(&err)],
        ),
    };
    let local = listener
        .local_addr()
        .unwrap_or_else(|_| SocketAddr::from((Ipv4Addr::UNSPECIFIED, port)));
    let listening = Arc::new(Listening {
        wake,
        closed: AtomicBool::new(false),
        accepting: AtomicBool::new(true),
        local,
        poller: poller.clone(),
    });
    *LISTENER.lock() = Some(listening.clone());

    {
        let host = listening.clone();
        drop(spawn("tcp-poller", move || poller.run(&host)));
    }

    TCP_LOG.info("tcp listening", kv!["addr" => addr]);

    accept_loop(&listener, &listening);
    drop(listener);
    listening.accepting.store(false, Ordering::SeqCst);
    listening.poller.wake();
}

pub fn tcp_listener_addr() -> Option<SocketAddr> {
    LISTENER.lock().as_ref().map(|l| l.local)
}

fn accept_loop(listener: &TcpListener, listening: &Listening) {
    let fd = listener.as_raw_fd();
    let mut delay = Duration(0);
    loop {
        if listening.closed.load(Ordering::SeqCst) {
            return;
        }

        match wait_readable(fd, &listening.wake, None) {
            Ok(Wait::Woken) => return,
            Ok(Wait::Ready) => {}
            Err(_) => continue,
        }

        let (stream, peer) = match listener.accept() {
            Ok(accepted) => accepted,
            Err(e)
                if matches!(
                    e.kind(),
                    ErrorKind::WouldBlock | ErrorKind::Interrupted | ErrorKind::ConnectionAborted
                ) =>
            {
                continue;
            }
            Err(e) => {
                if listening.closed.load(Ordering::SeqCst) {
                    return;
                }

                delay = next_accept_delay(delay);
                let (suppressed, ok) = ACCEPT_THROTTLE.allow("accept");
                if ok {
                    TCP_LOG.error(
                        "accept failed",
                        &log::with_suppressed(kv!["err" => e, "retry_in" => delay], suppressed),
                    );
                }
                std::thread::sleep(StdDuration::from_nanos(delay.0 as u64));
                continue;
            }
        };

        delay = Duration(0);
        configure_accepted(&stream);
        listening.open(stream, peer);
    }
}

fn configure_accepted(stream: &TcpStream) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_nodelay(true);
    let fd = stream.as_raw_fd();
    let _ = setsockopt(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1);
    let _ = setsockopt(fd, libc::IPPROTO_TCP, TCP_KEEPIDLE_OPTION, KEEP_ALIVE_IDLE);
    let _ = setsockopt(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_KEEPINTVL,
        KEEP_ALIVE_INTERVAL,
    );
    let _ = setsockopt(fd, libc::IPPROTO_TCP, libc::TCP_KEEPCNT, KEEP_ALIVE_COUNT);
    let _ = setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, SOCKET_BUFFER);
    let _ = setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDBUF, SOCKET_BUFFER);
}

fn next_accept_delay(delay: Duration) -> Duration {
    if delay.0 == 0 {
        return ACCEPT_BACKOFF_MIN;
    }

    let delay = Duration(delay.0.saturating_mul(2));
    if delay.0 > ACCEPT_BACKOFF_MAX.0 {
        return ACCEPT_BACKOFF_MAX;
    }

    delay
}

pub fn stop_tcp(timeout: StdDuration) {
    let listening = LISTENER.lock().clone();
    if let Some(l) = &listening {
        l.closed.store(true, Ordering::SeqCst);
        l.wake.wake();
    }

    STOPPING.store(true, Ordering::SeqCst);
    if let Some(l) = &listening {
        l.poller.wake();
    }
    for_each_open(|s| {
        let _ = s.shutdown(Shutdown::Read);
    });

    if !CONNS.wait_timeout(timeout) {
        for_each_open(|s| {
            let _ = s.shutdown(Shutdown::Both);
        });
        CONNS.wait();
    }
}

fn for_each_open(f: impl Fn(&TcpStream)) {
    if let Some(open) = OPEN.lock().as_ref() {
        for s in open.values() {
            f(s);
        }
    }
}

fn register(stream: &Arc<TcpStream>) -> u64 {
    let id = NEXT_CONN.fetch_add(1, Ordering::SeqCst);
    OPEN.lock()
        .get_or_insert_with(HashMap::new)
        .insert(id, stream.clone());
    id
}

fn unregister(id: u64) {
    if let Some(open) = OPEN.lock().as_mut() {
        open.remove(&id);
    }
}

impl Listening {
    fn open(&self, stream: TcpStream, peer: SocketAddr) {
        CONNS.add(1);
        let mut conn = Connection::open(stream, Some(peer));
        if conn.begin_line() {
            self.park(conn);
        } else {
            conn.finish();
        }
    }

    fn park(&self, conn: Connection) {
        let deadline = conn.deadline;
        if let Err(conn) = self.poller.register(conn, deadline, Instant::now()) {
            conn.finish();
        }
    }

    fn serve(&self, mut conn: Connection) {
        match conn.advance() {
            Next::Wait => self.park(conn),
            Next::Close => conn.finish(),
        }
    }
}

impl Host<Connection> for Listening {
    fn stopping(&self) -> bool {
        self.closed.load(Ordering::SeqCst) && STOPPING.load(Ordering::SeqCst)
    }

    fn accept_finished(&self) -> bool {
        !self.accepting.load(Ordering::SeqCst)
    }

    fn expired(&self, conn: Connection) {
        if !STOPPING.load(Ordering::SeqCst) {
            read_error(&conn.addr, ReadError::Timeout);
        }
        conn.finish();
    }

    fn drained(&self, conn: Connection) {
        conn.finish();
    }

    fn ready(self: &Arc<Self>, conn: Connection) {
        let turn = Turn {
            conn: Some(conn),
            listening: self.clone(),
        };
        workers().submit(Box::new(move || turn.run()));
    }
}

struct Turn {
    conn: Option<Connection>,
    listening: Arc<Listening>,
}

impl Turn {
    fn run(mut self) {
        if let Some(conn) = self.conn.take() {
            self.listening.serve(conn);
        }
    }
}

impl Drop for Turn {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            conn.finish();
        }
    }
}

struct TcpConn {
    stream: Arc<TcpStream>,
    remote: Option<SocketAddr>,
    closed: Cell<bool>,
}

impl Conn for TcpConn {
    fn close(&self) {
        self.closed.set(true);
        let _ = self.stream.shutdown(Shutdown::Both);
    }

    fn remote_addr(&self) -> Option<SocketAddr> {
        self.remote
    }
}

enum Next {
    Wait,
    Close,
}

struct Connection {
    conn: TcpConn,
    id: u64,
    addr: String,
    start: Instant,
    debug: bool,
    commands: i64,
    session: Session,
    reader: LineReader,
    writer: LineWriter,
    reading: bool,
    deadline: Option<Instant>,
}

impl Parked for Connection {
    fn fd(&self) -> RawFd {
        self.conn.stream.as_raw_fd()
    }
}

impl Connection {
    fn open(stream: TcpStream, remote: Option<SocketAddr>) -> Connection {
        let start = Instant::now();
        let addr = connection_addr(remote);
        let debug = log::enabled(Level::DEBUG);

        metrics::tcp_connection_opened();
        if debug {
            TCP_LOG.debug("connection opened", kv!["remote_addr" => addr]);
        }

        let stream = Arc::new(stream);
        let id = register(&stream);
        Connection {
            conn: TcpConn {
                stream,
                remote,
                closed: Cell::new(false),
            },
            id,
            addr,
            start,
            debug,
            commands: 0,
            session: tcp_routing::new_session(),
            reader: LineReader::new(),
            writer: LineWriter::new(),
            reading: false,
            deadline: None,
        }
    }

    fn begin_line(&mut self) -> bool {
        if STOPPING.load(Ordering::SeqCst) {
            return false;
        }

        self.deadline = if self.session.is_authenticated() {
            None
        } else {
            Some(Instant::now() + UNAUTHENTICATED_READ_TIMEOUT)
        };
        self.reading = true;
        true
    }

    fn advance(&mut self) -> Next {
        loop {
            if !self.reading && !self.begin_line() {
                return Next::Close;
            }

            match self.reader.read_slice(&self.conn.stream, self.deadline) {
                Ok(Some((lo, hi))) => {
                    self.reading = false;
                    if !self.answer(lo, hi) {
                        return Next::Close;
                    }
                }
                Ok(None) => return Next::Wait,
                Err(err) => {
                    if !STOPPING.load(Ordering::SeqCst) {
                        read_error(&self.addr, err);
                    }
                    return Next::Close;
                }
            }
        }
    }

    fn answer(&mut self, lo: usize, hi: usize) -> bool {
        let line = &self.reader.buf[lo..hi];

        let command_start = Instant::now();
        let resp = tcp_routing::route_line(line, &self.conn, &mut self.session);
        observe_command(
            tcp_routing::command_name(line),
            command_start.elapsed(),
            command_start,
        );
        self.commands += 1;

        let stream = &*self.conn.stream;
        if !resp.is_empty()
            && let Err(e) = self.writer.write(stream, &resp)
        {
            write_error("write", &self.addr, &self.conn, &e);
            return false;
        }
        if let Err(e) = self.writer.write(stream, b"\n") {
            write_error("write", &self.addr, &self.conn, &e);
            return false;
        }
        if let Err(e) = self.writer.flush(stream) {
            write_error("flush", &self.addr, &self.conn, &e);
            return false;
        }
        true
    }

    fn finish(self) {
        let Connection {
            conn,
            id,
            addr,
            start,
            debug,
            commands,
            ..
        } = self;

        unregister(id);
        drop(conn);
        metrics::tcp_connection_closed();
        if debug {
            TCP_LOG.debug(
                "connection closed",
                kv![
                    "remote_addr" => addr,
                    "duration_ms" => log::since(start),
                    "commands" => commands,
                ],
            );
        }
        CONNS.done();
    }
}

enum ReadError {
    Eof,
    Timeout,
    BufferFull,
    Io(io::Error),
}

struct LineReader {
    buf: Vec<u8>,
    start: usize,
    end: usize,
}

impl LineReader {
    fn new() -> LineReader {
        LineReader {
            buf: Vec::new(),
            start: 0,
            end: 0,
        }
    }

    fn read_slice(
        &mut self,
        stream: &TcpStream,
        deadline: Option<Instant>,
    ) -> Result<Option<(usize, usize)>, ReadError> {
        loop {
            if let Some(i) = self.buf[self.start..self.end]
                .iter()
                .position(|&c| c == b'\n')
            {
                let line = (self.start, self.start + i + 1);
                self.start += i + 1;
                return Ok(Some(line));
            }

            if self.end - self.start == BUFFER_SIZE {
                self.start = self.end;
                return Err(ReadError::BufferFull);
            }

            if self.start > 0 {
                self.buf.copy_within(self.start..self.end, 0);
                self.end -= self.start;
                self.start = 0;
            }

            if let Some(d) = deadline
                && Instant::now() >= d
            {
                return Err(ReadError::Timeout);
            }

            if self.buf.is_empty() {
                self.buf = vec![0u8; BUFFER_SIZE];
            }

            match receive(stream, &mut self.buf[self.end..]) {
                Ok(0) => return Err(ReadError::Eof),
                Ok(n) => self.end += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) if e.kind() == ErrorKind::WouldBlock => return Ok(None),
                Err(e) if e.kind() == ErrorKind::TimedOut && deadline.is_some() => {
                    return Err(ReadError::Timeout);
                }
                Err(e) => return Err(ReadError::Io(e)),
            }
        }
    }
}

fn receive(stream: &TcpStream, buf: &mut [u8]) -> io::Result<usize> {
    let n = unsafe {
        libc::recv(
            stream.as_raw_fd(),
            buf.as_mut_ptr().cast::<libc::c_void>(),
            buf.len(),
            libc::MSG_DONTWAIT,
        )
    };
    if n < 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(n as usize)
}

struct LineWriter {
    buf: Vec<u8>,
}

impl LineWriter {
    fn new() -> LineWriter {
        LineWriter { buf: Vec::new() }
    }

    fn write(&mut self, mut stream: &TcpStream, mut p: &[u8]) -> io::Result<()> {
        while p.len() > BUFFER_SIZE - self.buf.len() {
            if self.buf.is_empty() {
                let n = stream.write(p)?;
                p = &p[n..];
            } else {
                let n = (BUFFER_SIZE - self.buf.len()).min(p.len());
                self.buf.extend_from_slice(&p[..n]);
                p = &p[n..];
                self.flush(stream)?;
            }
        }
        self.buf.extend_from_slice(p);
        Ok(())
    }

    fn flush(&mut self, mut stream: &TcpStream) -> io::Result<()> {
        let mut written = 0;
        while written < self.buf.len() {
            match stream.write(&self.buf[written..]) {
                Ok(0) => {
                    self.buf.drain(..written);
                    return Err(io::Error::from(ErrorKind::WriteZero));
                }
                Ok(n) => written += n,
                Err(e) if e.kind() == ErrorKind::Interrupted => {}
                Err(e) => {
                    self.buf.drain(..written);
                    return Err(e);
                }
            }
        }
        self.buf.clear();
        Ok(())
    }
}

fn connection_addr(remote: Option<SocketAddr>) -> String {
    tcp_routing::source_name(tcp_routing::login_source(remote))
}

fn observe_command(command: &str, d: StdDuration, start: Instant) {
    metrics::observe_tcp(command, d);

    let Some(cfg) = globals::config() else {
        return;
    };
    let slow = cfg.log.slow_request_ms;
    if slow <= 0 || (d.as_nanos() as i128) < i128::from(slow) * 1_000_000 {
        return;
    }

    metrics::slow_request();
    let (suppressed, ok) = SLOW_THROTTLE.allow(command);
    if ok {
        TCP_LOG.warn(
            "slow command",
            &log::with_suppressed(
                kv!["command" => command, "duration_ms" => log::since(start)],
                suppressed,
            ),
        );
    }
}

fn read_error(addr: &str, err: ReadError) {
    match err {
        ReadError::Eof => connection_ended("read", addr, CLOSED_BY_CLIENT),
        ReadError::Timeout => connection_ended("read", addr, READ_TIMED_OUT),
        ReadError::BufferFull => connection_failed("read", addr, LINE_TOO_LONG),
        ReadError::Io(e) => io_error("read", addr, &e),
    }
}

fn write_error(op: &str, addr: &str, conn: &TcpConn, e: &io::Error) {
    if conn.closed.get() {
        connection_ended(op, addr, CONNECTION_CLOSED);
        return;
    }

    io_error(op, addr, e);
}

fn io_error(op: &str, addr: &str, e: &io::Error) {
    let text = e.to_string();
    let expected = matches!(
        e.kind(),
        ErrorKind::ConnectionReset
            | ErrorKind::BrokenPipe
            | ErrorKind::UnexpectedEof
            | ErrorKind::TimedOut
            | ErrorKind::WouldBlock
    ) || e.raw_os_error() == Some(libc::EPIPE)
        || e.raw_os_error() == Some(libc::ECONNRESET);

    if expected {
        connection_ended(op, addr, &text);
        return;
    }

    connection_failed(op, addr, &text);
}

fn connection_ended(op: &str, addr: &str, err: &str) {
    if log::enabled(Level::DEBUG) {
        TCP_LOG.debug(
            "connection ended",
            kv!["op" => op, "remote_addr" => addr, "err" => err],
        );
    }
}

fn connection_failed(op: &str, addr: &str, err: &str) {
    TCP_LOG.warn(
        "connection error",
        kv!["op" => op, "remote_addr" => addr, "err" => err],
    );
}
