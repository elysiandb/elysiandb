use std::io::{self, Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use elysiandb::boot;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::log::{self, Options};
use elysiandb::metrics::counters::TCP_OPEN;

use crate::tempdir::TempDir;

const WANTED_CONNECTIONS: usize = 5000;
const MIN_CONNECTIONS: usize = 1000;
const SPARE_DESCRIPTORS: libc::rlim_t = 512;
const EXTRA_THREADS: usize = 64;
const CONNECT_BATCH: usize = 64;
const SLOW_READERS: usize = 128;
const GETS_PER_READER: usize = 16;
const LARGE_VALUE: usize = 100_000;

struct QuietLog;

impl Drop for QuietLog {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
    }
}

fn quiet_log() -> QuietLog {
    if let Err(err) = log::configure(&Options {
        level: "info".to_string(),
        ..Options::default()
    }) {
        panic!("{err}");
    }
    log::set_output(Some(Box::new(io::sink())), false);
    QuietLog
}

#[cfg(target_os = "linux")]
fn threads() -> usize {
    let status = std::fs::read_to_string("/proc/self/status").expect("/proc/self/status");
    status
        .lines()
        .find_map(|line| line.strip_prefix("Threads:"))
        .and_then(|n| n.trim().parse().ok())
        .expect("thread count")
}

#[cfg(target_os = "macos")]
fn threads() -> usize {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    assert_eq!(n, size, "proc_pidinfo");
    usize::try_from(unsafe { info.assume_init() }.pti_threadnum).expect("thread count")
}

fn connection_budget() -> usize {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    assert_eq!(
        unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut lim) },
        0,
        "getrlimit"
    );
    let wanted = (WANTED_CONNECTIONS * 2) as libc::rlim_t + SPARE_DESCRIPTORS;
    if lim.rlim_cur < wanted {
        let raised = libc::rlimit {
            rlim_cur: wanted.min(lim.rlim_max),
            rlim_max: lim.rlim_max,
        };
        unsafe {
            libc::setrlimit(libc::RLIMIT_NOFILE, &raw const raised);
            libc::getrlimit(libc::RLIMIT_NOFILE, &raw mut lim);
        }
    }
    let usable = lim.rlim_cur.saturating_sub(SPARE_DESCRIPTORS) / 2;
    usize::try_from(usable).map_or(WANTED_CONNECTIONS, |n| n.min(WANTED_CONNECTIONS))
}

fn start_tcp(folder: &std::path::Path, token: Option<&str>) -> SocketAddr {
    let mut cfg = Config::default();
    cfg.store.folder = folder.join("data").to_string_lossy().into_owned();
    cfg.store.shards = 4;
    cfg.server.tcp.enabled = true;
    cfg.server.tcp.host = "127.0.0.1".to_string();
    cfg.server.tcp.port = 0;
    if let Some(token) = token {
        cfg.security.authentication.enabled = true;
        cfg.security.authentication.mode = "token".to_string();
        cfg.security.authentication.token = token.to_string();
    }
    globals::set_config(Some(Arc::new(cfg)));
    if token.is_none() {
        boot::init_db();
    }

    let before = boot::tcp_listener_addr();
    std::thread::spawn(boot::init_tcp);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(addr) = boot::tcp_listener_addr()
            && Some(addr) != before
        {
            return addr;
        }
        assert!(Instant::now() < deadline, "the TCP listener did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn ping(conn: &mut TcpStream) {
    conn.write_all(b"PING\n").expect("ping");
    let mut answer = [0u8; 5];
    conn.read_exact(&mut answer).expect("pong");
    assert_eq!(&answer, b"PONG\n");
}

fn wait_open(want: i64) {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let open = TCP_OPEN.load(Ordering::SeqCst);
        if open == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{open} TCP connections are open, want {want}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn test_idle_tcp_connections_keep_the_thread_count_bounded() {
    let _log = quiet_log();
    let n = connection_budget();
    assert!(
        n >= MIN_CONNECTIONS,
        "the file limit leaves room for {n} connections"
    );
    let dir = TempDir::new();
    let addr = start_tcp(dir.path(), Some("idle-token"));
    let open_before = TCP_OPEN.load(Ordering::SeqCst);
    let threads_before = threads();

    let mut conns = Vec::with_capacity(n);
    while conns.len() < n {
        for _ in 0..CONNECT_BATCH.min(n - conns.len()) {
            match TcpStream::connect(addr) {
                Ok(conn) => conns.push(conn),
                Err(err) => panic!("connection {} of {n}: {err}", conns.len()),
            }
        }
        wait_open(open_before + conns.len() as i64);
    }

    let idle = threads();
    assert!(
        idle <= threads_before + EXTRA_THREADS,
        "{n} idle connections run on {idle} threads, {threads_before} before"
    );

    for conn in &mut conns {
        ping(conn);
    }
    let mut fresh = TcpStream::connect(addr).expect("connect");
    ping(&mut fresh);

    let busy = threads();
    assert!(
        busy <= threads_before + EXTRA_THREADS,
        "{n} connections run on {busy} threads after a command each, {threads_before} before"
    );

    drop(conns);
    drop(fresh);
    wait_open(open_before);
}

fn wait_unread(conn: &TcpStream, want: usize) {
    conn.set_nonblocking(true).expect("nonblocking");
    let mut buf = vec![0u8; want];
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        match conn.peek(&mut buf) {
            Ok(n) if n >= want => break,
            Ok(0) => panic!("the server closed a connection that does not read"),
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {}
            Err(e) => panic!("peek: {e}"),
        }
        assert!(
            Instant::now() < deadline,
            "the server did not answer a connection that does not read"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    conn.set_nonblocking(false).expect("blocking");
}

fn wait_threads(limit: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        let n = threads();
        if n <= limit {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what} run on {n} threads, want at most {limit}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

#[test]
fn test_tcp_clients_that_do_not_read_do_not_hold_threads() {
    let _log = quiet_log();
    let dir = TempDir::new();
    let addr = start_tcp(dir.path(), None);
    let open_before = TCP_OPEN.load(Ordering::SeqCst);

    let mut normal = TcpStream::connect(addr).expect("connect");
    let mut set = b"SET big ".to_vec();
    set.extend(std::iter::repeat_n(b'x', LARGE_VALUE));
    set.push(b'\n');
    normal.write_all(&set).expect("set");
    let mut ok = [0u8; 3];
    normal.read_exact(&mut ok).expect("set answer");
    assert_eq!(&ok, b"OK\n");
    let threads_before = threads();

    let gets = b"GET big\n".repeat(GETS_PER_READER);
    let mut readers = Vec::with_capacity(SLOW_READERS);
    for _ in 0..SLOW_READERS {
        let conn = TcpStream::connect(addr).expect("connect");
        (&conn).write_all(&gets).expect("gets");
        wait_unread(&conn, LARGE_VALUE);
        readers.push(conn);
    }
    wait_open(open_before + SLOW_READERS as i64 + 1);
    wait_threads(
        threads_before + EXTRA_THREADS,
        &format!("{SLOW_READERS} clients that do not read their answers"),
    );

    let start = Instant::now();
    ping(&mut normal);
    let mut fresh = TcpStream::connect(addr).expect("connect");
    ping(&mut fresh);
    normal.write_all(b"GET big\n").expect("get");
    let mut value = vec![0u8; "big=".len() + LARGE_VALUE + 1];
    normal.read_exact(&mut value).expect("get answer");
    assert!(value.starts_with(b"big=x"));
    assert_eq!(value.last(), Some(&b'\n'));
    assert!(
        start.elapsed() < Duration::from_secs(2),
        "a client that reads waited {:?} behind {SLOW_READERS} clients that do not",
        start.elapsed()
    );

    drop(readers);
    drop(fresh);
    drop(normal);
    wait_open(open_before);
}
