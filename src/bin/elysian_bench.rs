use std::io::{self, ErrorKind, Read, Write};
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, SyncSender};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use elysiandb::boot;
use elysiandb::cmd;
use elysiandb::gocompat::flag::FlagSet;
use elysiandb::gocompat::fmt::{Arg, sprintf};
use elysiandb::gocompat::rand::{self, Rand};
use elysiandb::gocompat::strings;
use elysiandb::gocompat::time::Duration;
use elysiandb::sync::{self, Mutex, Ticker};

const USAGE: &str = "Usage: elysian_bench [options]

Options:
  -addr <host:port>     TCP address (host:port) (default \"127.0.0.1:8088\")
  -auth <credentials>   Credentials sent with AUTH on each connection: '<token>' or '<username> <password>'
  -duration <duration>  Test duration (e.g., 20s, 1m) (default 20s)
  -keys <n>             Number of distinct keys (default 20000)
  -live                 Show a live summary every second (default true)
  -pair                 Chain SET then GET on the same key (1:1). Ignores -setratio
  -payload <n>          SET value size (bytes) (default 16)
  -rps <n>              Global req/s limit (0 = unlimited)
  -setratio <ratio>     SET fraction (0..1) (default 0.1)
  -vus <n>              Number of concurrent connections (VUs) (default 4)
  -warmup               Pre-fill the DB with SETs (default true)
";

const BUFFER_SIZE: usize = 128 << 10;
const SOCKET_BUFFER: libc::c_int = 256 << 10;
const KEEP_ALIVE_PERIOD: libc::c_int = 120;
const RULE: &str = "  ─────────────────────────────────────────────────────────────────\n";

#[cfg(target_os = "macos")]
const TCP_KEEPIDLE_OPTION: libc::c_int = libc::TCP_KEEPALIVE;
#[cfg(not(target_os = "macos"))]
const TCP_KEEPIDLE_OPTION: libc::c_int = libc::TCP_KEEPIDLE;

static RNG: Mutex<Option<Rand>> = Mutex::new(None);

#[derive(Clone, Copy, PartialEq, Eq)]
enum Op {
    Get,
    Set,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ErrKind {
    None,
    Connect,
    Write,
    Read,
    Eof,
    Proto,
    Miss,
}

#[derive(Clone, Copy)]
struct Outcome {
    latency: i64,
    bytes_in: i64,
    bytes_out: i64,
    err_kind: ErrKind,
}

impl Outcome {
    fn failed(err_kind: ErrKind, bytes_out: i64) -> Outcome {
        Outcome {
            latency: 0,
            bytes_in: 0,
            bytes_out,
            err_kind,
        }
    }
}

#[derive(Clone)]
struct Options {
    pair: bool,
    addr: String,
    auth: Vec<u8>,
    keys: i64,
    set_ratio: f64,
}

struct Rolling {
    buf: Vec<i64>,
}

impl Rolling {
    fn push(&mut self, d: i64) {
        self.buf.push(d);
        if self.buf.len() > 20000 {
            let keep = self.buf.len() - 10000;
            self.buf.drain(..keep);
        }
    }

    fn snapshot(&self) -> Vec<i64> {
        self.buf.clone()
    }
}

fn printf(format: &str, args: &[Arg]) {
    cmd::print(sprintf(format, args));
}

fn println(text: &str) {
    cmd::println(text);
}

fn intn(n: i64) -> i64 {
    let mut rng = RNG.lock();
    match rng.as_mut() {
        Some(r) => r.intn(n),
        None => panic!("the random source is not seeded"),
    }
}

fn float64() -> f64 {
    let mut rng = RNG.lock();
    match rng.as_mut() {
        Some(r) => r.float64(),
        None => panic!("the random source is not seeded"),
    }
}

fn since(t: Instant) -> Duration {
    Duration(i64::try_from(t.elapsed().as_nanos()).unwrap_or(i64::MAX))
}

fn readable_bps(bytes: i64, secs: f64) -> String {
    if secs <= 0.0 {
        return "0 B/s".to_string();
    }
    let bps = bytes as f64 / secs;
    let k = 1024.0;
    let text = if bps >= k * k * k {
        sprintf("%.2f GB/s", &[Arg::Float(bps / (k * k * k))])
    } else if bps >= k * k {
        sprintf("%.2f MB/s", &[Arg::Float(bps / (k * k))])
    } else if bps >= k {
        sprintf("%.2f KB/s", &[Arg::Float(bps / k)])
    } else {
        sprintf("%.0f B/s", &[Arg::Float(bps)])
    };

    String::from_utf8_lossy(&text).into_owned()
}

fn percentile(ds: &mut [i64], p: f64) -> i64 {
    if ds.is_empty() {
        return 0;
    }
    ds.sort_unstable();
    let idx = ((p * ds.len() as f64).ceil() as i64 - 1).clamp(0, ds.len() as i64 - 1);

    ds[idx as usize]
}

fn ascii_bar(p: f64, width: i64) -> String {
    let p = p.clamp(0.0, 1.0);
    let fill = ((p * width as f64).round() as i64).min(width);

    "█".repeat(fill as usize) + &"░".repeat((width - fill) as usize)
}

fn gen_value(n: i64) -> Vec<u8> {
    if n <= 0 {
        return b"x".to_vec();
    }
    (0..n).map(|_| b'A' + intn(16) as u8).collect()
}

fn pick_key(keys: i64) -> String {
    format!("key_{}", intn(keys))
}

fn dur_str(d: i64) -> String {
    if d == 0 {
        return "-".to_string();
    }
    let ms = Duration(d).microseconds() as f64 / 1000.0;

    String::from_utf8_lossy(&sprintf("%.2fms", &[Arg::Float(ms)])).into_owned()
}

fn ms(d: i64) -> Arg<'static> {
    Arg::Float(Duration(d).microseconds() as f64 / 1000.0)
}

fn setsockopt(fd: libc::c_int, level: libc::c_int, name: libc::c_int, value: libc::c_int) {
    unsafe {
        libc::setsockopt(
            fd,
            level,
            name,
            (&raw const value).cast::<libc::c_void>(),
            std::mem::size_of::<libc::c_int>() as libc::socklen_t,
        );
    }
}

fn dial(opts: &Options) -> io::Result<TcpStream> {
    let (ip, port) = boot::resolve_tcp_addr(&opts.addr).map_err(io::Error::other)?;
    let ip = ip.unwrap_or(IpAddr::V4(Ipv4Addr::UNSPECIFIED));
    let conn = TcpStream::connect(SocketAddr::new(ip, port))?;
    let _ = conn.set_nodelay(true);
    let fd = conn.as_raw_fd();
    setsockopt(fd, libc::SOL_SOCKET, libc::SO_KEEPALIVE, 1);
    setsockopt(
        fd,
        libc::IPPROTO_TCP,
        libc::TCP_KEEPINTVL,
        KEEP_ALIVE_PERIOD,
    );
    setsockopt(
        fd,
        libc::IPPROTO_TCP,
        TCP_KEEPIDLE_OPTION,
        KEEP_ALIVE_PERIOD,
    );
    setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, SOCKET_BUFFER);
    setsockopt(fd, libc::SOL_SOCKET, libc::SO_SNDBUF, SOCKET_BUFFER);
    if !opts.auth.is_empty() {
        authenticate(&conn, &opts.auth)?;
    }

    Ok(conn)
}

fn authenticate(mut conn: &TcpStream, credentials: &[u8]) -> io::Result<()> {
    let mut line = b"AUTH ".to_vec();
    line.extend_from_slice(credentials);
    line.push(b'\n');
    conn.write_all(&line)?;

    let mut reply = Vec::new();
    let mut b = [0u8; 1];
    loop {
        match conn.read(&mut b) {
            Ok(0) => return Err(io::Error::from(ErrorKind::UnexpectedEof)),
            Ok(_) if b[0] == b'\n' => break,
            Ok(_) => reply.push(b[0]),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => return Err(e),
        }
    }
    if strings::trim_space(&reply) != b"OK" {
        return Err(io::Error::other("AUTH failed"));
    }

    Ok(())
}

struct LineReader {
    conn: TcpStream,
    buf: Vec<u8>,
    r: usize,
    w: usize,
}

enum ReadFailure {
    Eof,
    Other,
}

impl LineReader {
    fn new(conn: TcpStream) -> LineReader {
        LineReader {
            conn,
            buf: vec![0; BUFFER_SIZE],
            r: 0,
            w: 0,
        }
    }

    fn read_slice(&mut self) -> Result<usize, ReadFailure> {
        let mut s = 0;
        loop {
            if let Some(i) = self.buf[self.r + s..self.w]
                .iter()
                .position(|&c| c == b'\n')
            {
                let n = s + i + 1;
                self.r += n;
                return Ok(n);
            }
            if self.w - self.r >= self.buf.len() {
                self.r = self.w;
                return Err(ReadFailure::Other);
            }
            s = self.w - self.r;
            if self.r > 0 {
                self.buf.copy_within(self.r..self.w, 0);
                self.w -= self.r;
                self.r = 0;
            }
            loop {
                match self.conn.read(&mut self.buf[self.w..]) {
                    Ok(0) => return Err(ReadFailure::Eof),
                    Ok(n) => {
                        self.w += n;
                        break;
                    }
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => return Err(ReadFailure::Other),
                }
            }
        }
    }
}

struct LineWriter {
    conn: TcpStream,
    buf: Vec<u8>,
    failed: bool,
}

impl LineWriter {
    fn new(conn: TcpStream) -> LineWriter {
        LineWriter {
            conn,
            buf: Vec::with_capacity(BUFFER_SIZE),
            failed: false,
        }
    }

    fn flush(&mut self) -> bool {
        if self.failed {
            return false;
        }
        if self.buf.is_empty() {
            return true;
        }
        if self.conn.write_all(&self.buf).is_err() {
            self.failed = true;
            return false;
        }
        self.buf.clear();

        true
    }

    fn write_string(&mut self, mut s: &[u8]) -> (i64, bool) {
        let mut nn = 0i64;
        while s.len() > BUFFER_SIZE - self.buf.len() && !self.failed {
            let n = BUFFER_SIZE - self.buf.len();
            self.buf.extend_from_slice(&s[..n]);
            self.flush();
            nn += n as i64;
            s = &s[n..];
        }
        if self.failed {
            return (nn, false);
        }
        self.buf.extend_from_slice(s);

        (nn + s.len() as i64, true)
    }

    fn send(&mut self, line: &[u8]) -> (i64, bool) {
        let (nw, ok) = self.write_string(line);
        (nw, ok && self.flush())
    }
}

fn exchange(
    reader: &mut LineReader,
    writer: &mut LineWriter,
    line: &[u8],
    op: Op,
    results: &SyncSender<Outcome>,
) -> Option<bool> {
    let t0 = Instant::now();
    let (nw, ok) = writer.send(line);
    if !ok {
        let _ = results.send(Outcome::failed(ErrKind::Write, nw));
        return None;
    }
    let read = reader.read_slice();
    let latency = since(t0).0;

    let mut res = Outcome {
        latency,
        bytes_in: 0,
        bytes_out: nw,
        err_kind: ErrKind::None,
    };
    let n = match read {
        Ok(n) => n,
        Err(failure) => {
            res.err_kind = match failure {
                ReadFailure::Eof => ErrKind::Eof,
                ReadFailure::Other => ErrKind::Read,
            };
            let _ = results.send(res);
            return Some(false);
        }
    };
    res.bytes_in = n as i64;
    let answer = strings::trim_space(&reader.buf[reader.r - n..reader.r]);
    match op {
        Op::Get => {
            if strings::to_lower(answer).starts_with(b"key not found") {
                res.err_kind = ErrKind::Miss;
            }
        }
        Op::Set => {
            if !strings::to_upper(answer).starts_with(b"OK") {
                res.err_kind = ErrKind::Proto;
            }
        }
    }
    let _ = results.send(res);

    Some(true)
}

fn set_line(key: &str, value: &[u8]) -> Vec<u8> {
    let mut line = b"SET ".to_vec();
    line.extend_from_slice(key.as_bytes());
    line.push(b' ');
    line.extend_from_slice(value);
    line.push(b'\n');
    line
}

fn get_line(key: &str) -> Vec<u8> {
    format!("GET {key}\n").into_bytes()
}

fn passed(deadline: Option<Instant>) -> bool {
    deadline.is_some_and(|d| Instant::now() >= d)
}

fn run_vu(
    opts: &Options,
    value: &[u8],
    deadline: Option<Instant>,
    tick: Option<&Mutex<Receiver<()>>>,
    results: &SyncSender<Outcome>,
) {
    let conn = match dial(opts) {
        Ok(c) => c,
        Err(_) => {
            let _ = results.send(Outcome::failed(ErrKind::Connect, 0));
            return;
        }
    };
    let Ok(write_half) = conn.try_clone() else {
        let _ = results.send(Outcome::failed(ErrKind::Connect, 0));
        return;
    };
    let mut reader = LineReader::new(conn);
    let mut writer = LineWriter::new(write_half);

    loop {
        if passed(deadline) {
            return;
        }
        if let Some(tick) = tick {
            let _ = tick.lock().recv();
        }

        if opts.pair {
            let key = pick_key(opts.keys);
            match exchange(
                &mut reader,
                &mut writer,
                &set_line(&key, value),
                Op::Set,
                results,
            ) {
                None => continue,
                Some(false) => return,
                Some(true) => {}
            }
            match exchange(&mut reader, &mut writer, &get_line(&key), Op::Get, results) {
                None => continue,
                Some(false) => return,
                Some(true) => {}
            }
            continue;
        }

        let op = if float64() < opts.set_ratio {
            Op::Set
        } else {
            Op::Get
        };
        let key = pick_key(opts.keys);
        let line = match op {
            Op::Set => set_line(&key, value),
            Op::Get => get_line(&key),
        };
        if exchange(&mut reader, &mut writer, &line, op, results) == Some(false) {
            return;
        }
    }
}

fn warm_up(opts: &Options, vus: i64, value: &Arc<Vec<u8>>) {
    println("  ▶ Warmup: SET keys…");
    let start = Instant::now();
    let wp = (vus.wrapping_mul(2) as f64).min(64.0) as i64;
    let next = Arc::new(AtomicI64::new(0));
    let set_errs = Arc::new(AtomicI64::new(0));
    let mut workers = Vec::new();
    for i in 0..wp {
        let opts = opts.clone();
        let value = value.clone();
        let next = next.clone();
        let set_errs = set_errs.clone();
        workers.push(sync::spawn(&format!("warmup-{i}"), move || {
            let _fatal = sync::FatalOnPanic;
            let Ok(conn) = dial(&opts) else {
                set_errs.fetch_add(1, Ordering::SeqCst);
                return;
            };
            let Ok(write_half) = conn.try_clone() else {
                set_errs.fetch_add(1, Ordering::SeqCst);
                return;
            };
            let mut reader = LineReader::new(conn);
            let mut writer = LineWriter::new(write_half);
            loop {
                let k = next.fetch_add(1, Ordering::SeqCst);
                if k >= opts.keys {
                    return;
                }
                let mut line = format!("SET key_{k} ").into_bytes();
                line.extend_from_slice(&value);
                line.push(b'\n');
                if !writer.write_string(&line).1 || !writer.flush() {
                    set_errs.fetch_add(1, Ordering::SeqCst);
                    return;
                }
                if reader.read_slice().is_err() {
                    set_errs.fetch_add(1, Ordering::SeqCst);
                    return;
                }
            }
        }));
    }
    for w in workers {
        let _ = w.join();
    }
    let el = since(start).to_string();
    let errs = set_errs.load(Ordering::SeqCst);
    if errs > 0 {
        printf(
            "  ⚠ Warmup completed with %d errors in %s\n",
            &[Arg::Int(errs), Arg::Str(el.as_bytes())],
        );
    } else {
        printf("  ✓ Warmup completed in %s\n", &[Arg::Str(el.as_bytes())]);
    }
    println("");
}

fn start_ticker(rps: i64) -> Option<Arc<Mutex<Receiver<()>>>> {
    if rps <= 0 {
        return None;
    }
    let period = Duration::SECOND.0 / rps;
    if period <= 0 {
        return None;
    }
    let (tx, rx) = mpsc::sync_channel::<()>(1);
    sync::spawn("rps-ticker", move || {
        let mut ticker = Ticker::new(std::time::Duration::from_nanos(period as u64));
        loop {
            let wait = ticker.deadline().saturating_duration_since(Instant::now());
            std::thread::sleep(wait);
            if ticker.take(Instant::now()).is_some()
                && let Err(mpsc::TrySendError::Disconnected(())) = tx.try_send(())
            {
                return;
            }
        }
    });

    Some(Arc::new(Mutex::new(rx)))
}

fn main() {
    sync::install_base_panic_hook();
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    sync::run_fatal(run);
}

fn run() {
    let mut flags = FlagSet::new(USAGE);
    flags
        .bool("pair", false)
        .string("addr", b"127.0.0.1:8088")
        .int("vus", 4)
        .duration("duration", Duration::SECOND * 20)
        .int("rps", 0)
        .int("keys", 20000)
        .float64("setratio", 0.10)
        .int("payload", 16)
        .bool("warmup", true)
        .bool("live", true)
        .string("auth", b"");
    let args: Vec<Vec<u8>> = std::env::args_os().skip(1).map(|a| a.into_vec()).collect();
    if let Err(err) = flags.parse(args) {
        let mut out = Vec::new();
        let _ = flags.write_error(&err, &mut out);
        cmd::write_stderr(out);
        std::process::exit(err.exit_code());
    }

    let pair = flags.get_bool("pair");
    let addr = flags.get_string("addr").to_vec();
    let vus = flags.get_int("vus");
    let duration = flags.get_duration("duration");
    let rps = flags.get_int("rps");
    let keys = flags.get_int("keys");
    let set_ratio = flags.get_float64("setratio");
    let payload_len = flags.get_int("payload");
    let warmup = flags.get_bool("warmup");
    let show_live = flags.get_bool("live");
    let auth = flags.get_string("auth").to_vec();

    let seed = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as i64);
    *RNG.lock() = Some(Rand::new(rand::new_source(seed)));

    println("");
    println("  █ ElysianDB TCP Bench");
    println("  ─────────────────────");
    printf("  addr      : %s\n", &[Arg::Str(&addr)]);
    printf("  vus       : %d\n", &[Arg::Int(vus)]);
    printf(
        "  duration  : %s\n",
        &[Arg::Str(duration.to_string().as_bytes())],
    );
    if rps > 0 {
        printf("  rps limit : %d\n", &[Arg::Int(rps)]);
    } else {
        printf("  rps limit : unlimited\n", &[]);
    }
    printf("  keys      : %d\n", &[Arg::Int(keys)]);
    if pair {
        printf("  mode      : SET→GET paired (1:1)\n", &[]);
        printf("  set ratio : (ignored in -pair)\n", &[]);
    } else {
        printf("  set ratio : %.2f\n", &[Arg::Float(set_ratio)]);
    }
    printf("  payload   : %d bytes\n", &[Arg::Int(payload_len)]);
    printf("  warmup    : %v\n", &[Arg::Bool(warmup)]);
    if pair {
        printf("  mode      : SET→GET paired (1:1)\n", &[]);
    }
    println("");

    let opts = Options {
        pair,
        addr: String::from_utf8_lossy(&addr).into_owned(),
        auth,
        keys,
        set_ratio,
    };
    let value = Arc::new(gen_value(payload_len));

    if warmup && keys > 0 {
        warm_up(&opts, vus, &value);
    }

    let tick = start_ticker(rps);

    let created = Instant::now();
    let deadline = if duration.0 <= 0 {
        Some(created)
    } else {
        created.checked_add(std::time::Duration::from_nanos(duration.0 as u64))
    };

    let (tx, rx) = mpsc::sync_channel::<Outcome>(1024);
    for i in 0..vus {
        let opts = opts.clone();
        let value = value.clone();
        let tick = tick.clone();
        let tx = tx.clone();
        sync::spawn(&format!("vu-{i}"), move || {
            let _fatal = sync::FatalOnPanic;
            run_vu(&opts, &value, deadline, tick.as_deref(), &tx)
        });
    }
    drop(tx);

    let mut total_req: i64 = 0;
    let mut total_err: i64 = 0;
    let mut total_miss: i64 = 0;
    let mut bytes_in: i64 = 0;
    let mut bytes_out: i64 = 0;
    let mut all_latencies: Vec<i64> = Vec::new();
    let mut roll = Rolling { buf: Vec::new() };

    let start = Instant::now();
    let (mut last_total, mut last_bytes_in, mut last_bytes_out) = (0i64, 0i64, 0i64);
    let mut live_ticker = Ticker::new(std::time::Duration::from_secs(1));

    loop {
        let now = Instant::now();
        if passed(deadline) {
            break;
        }
        let wake = match deadline {
            Some(d) => d.min(live_ticker.deadline()),
            None => live_ticker.deadline(),
        };
        match rx.recv_timeout(wake.saturating_duration_since(now)) {
            Ok(r) => {
                total_req += 1;
                bytes_in += r.bytes_in;
                bytes_out += r.bytes_out;
                if r.err_kind != ErrKind::None && r.err_kind != ErrKind::Miss {
                    total_err += 1;
                }
                if r.err_kind == ErrKind::Miss {
                    total_miss += 1;
                }
                if r.latency > 0 {
                    roll.push(r.latency);
                    all_latencies.push(r.latency);
                }
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => break,
        }

        if live_ticker.take(Instant::now()).is_none() || passed(deadline) {
            continue;
        }
        if show_live {
            let delta = total_req - last_total;
            last_total = total_req;
            let dbi = bytes_in - last_bytes_in;
            let dbo = bytes_out - last_bytes_out;
            last_bytes_in = bytes_in;
            last_bytes_out = bytes_out;

            let mut ls = roll.snapshot();
            let (mut p50, mut p95) = (0, 0);
            if !ls.is_empty() {
                p50 = percentile(&mut ls, 0.50);
                p95 = percentile(&mut ls, 0.95);
            }
            let elapsed = since(start).truncate(Duration::SECOND).to_string();
            let (p50, p95) = (dur_str(p50), dur_str(p95));
            let (rin, rout) = (readable_bps(dbi, 1.0), readable_bps(dbo, 1.0));
            printf(
                "\r  %s  | VUs: %-3d | req/s: %-6d | p50: %-6s | p95: %-6s | errs: %-6d | miss: %-6d | in: %-10s | out: %-10s",
                &[
                    Arg::Str(elapsed.as_bytes()),
                    Arg::Int(vus),
                    Arg::Int(delta),
                    Arg::Str(p50.as_bytes()),
                    Arg::Str(p95.as_bytes()),
                    Arg::Int(total_err),
                    Arg::Int(total_miss),
                    Arg::Str(rin.as_bytes()),
                    Arg::Str(rout.as_bytes()),
                ],
            );
        }
    }

    let total_dur = since(start);
    if show_live {
        println("");
    }
    let mut lat = all_latencies;

    let (mut min, mut max, mut sum) = (0i64, 0i64, 0i64);
    if let Some(&first) = lat.first() {
        min = first;
        max = first;
        for &d in &lat {
            min = min.min(d);
            max = max.max(d);
            sum = sum.wrapping_add(d);
        }
    }
    let avg = if lat.is_empty() {
        0
    } else {
        sum / lat.len() as i64
    };
    let p50 = percentile(&mut lat, 0.50);
    let p90 = percentile(&mut lat, 0.90);
    let p95 = percentile(&mut lat, 0.95);
    let p99 = percentile(&mut lat, 0.99);

    let thru = total_req as f64 / total_dur.seconds();

    println("");
    cmd::print(RULE);
    println("  █ RESULTS");
    cmd::print(RULE);
    printf(
        "  time           : %s\n",
        &[Arg::Str(
            total_dur
                .truncate(Duration::MILLISECOND)
                .to_string()
                .as_bytes(),
        )],
    );
    printf("  vus            : %d\n", &[Arg::Int(vus)]);
    printf("  requests       : %d\n", &[Arg::Int(total_req)]);
    printf(
        "  successes      : %d\n",
        &[Arg::Int(total_req - total_err)],
    );
    printf("  errors         : %d\n", &[Arg::Int(total_err)]);
    printf("  misses (GET)   : %d\n", &[Arg::Int(total_miss)]);
    println("");
    printf("  throughput     : %.0f req/s\n", &[Arg::Float(thru)]);
    if pair {
        printf(
            "  throughput (pairs) : %.0f pairs/s\n",
            &[Arg::Float(thru / 2.0)],
        );
    }
    let secs = total_dur.seconds();
    let (rin, rout) = (readable_bps(bytes_in, secs), readable_bps(bytes_out, secs));
    printf("  transfer in    : %s\n", &[Arg::Str(rin.as_bytes())]);
    printf("  transfer out   : %s\n", &[Arg::Str(rout.as_bytes())]);
    println("");
    println("  Latency (ms)");
    printf("    min          : %6.2f\n", &[ms(min)]);
    printf("    p50          : %6.2f\n", &[ms(p50)]);
    printf("    avg          : %6.2f\n", &[ms(avg)]);
    printf("    p90          : %6.2f\n", &[ms(p90)]);
    printf("    p95          : %6.2f\n", &[ms(p95)]);
    printf("    p99          : %6.2f\n", &[ms(p99)]);
    printf("    max          : %6.2f\n", &[ms(max)]);
    println("");
    let success_rate = if total_req > 0 {
        (total_req - total_err) as f64 / total_req as f64
    } else {
        0.0
    };
    let bar = ascii_bar(success_rate, 40);
    printf(
        "  success        : [%s] %5.1f%%\n",
        &[Arg::Str(bar.as_bytes()), Arg::Float(success_rate * 100.0)],
    );
    cmd::print(RULE);
}
