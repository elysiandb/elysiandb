use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use elysiandb::boot;

use super::cli::BIN;
use super::tempdir::TempDir;

fn free_port() -> u16 {
    let ln = TcpListener::bind("127.0.0.1:0").expect("free port");
    ln.local_addr().expect("local address").port()
}

fn server_config(http: u16, tcp: u16, http_options: &str, level: &str) -> String {
    format!(
        "store:\n  folder: data\n  shards: 4\nserver:\n  http: {{ enabled: true, host: 127.0.0.1, port: {http}{http_options} }}\n  tcp: {{ enabled: true, host: 127.0.0.1, port: {tcp} }}\nlog:\n  level: {level}\n  format: text\nsecurity:\n  authentication:\n    enabled: false\nadminui:\n  enabled: true\n"
    )
}

struct Server {
    child: Child,
    out: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    http: u16,
    tcp: u16,
}

impl Server {
    fn start(dir: &Path) -> Server {
        Server::start_with(dir, "", "info")
    }

    fn start_with(dir: &Path, http_options: &str, level: &str) -> Server {
        let port = free_port();
        let http = loop {
            let p = free_port();
            if p != port {
                break p;
            }
        };
        std::fs::write(
            dir.join("s.yaml"),
            server_config(http, port, http_options, level),
        )
        .expect("config");
        let mut child = Command::new(BIN)
            .args(["-config", "s.yaml"])
            .current_dir(dir)
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir)
            .env("TZ", "UTC")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .expect("start the server");
        let out = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let mut stdout = child.stdout.take().expect("stdout");
        let sink = out.clone();
        let reader = std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match stdout.read(&mut buf) {
                    Ok(0) | Err(_) => return,
                    Ok(n) => sink
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .extend_from_slice(&buf[..n]),
                }
            }
        });
        let server = Server {
            child,
            out,
            reader: Some(reader),
            http,
            tcp: port,
        };
        server.wait_for("msg=\"tcp listening\"");
        server.wait_for("msg=\"admin ui available\"");
        std::thread::sleep(Duration::from_millis(200));
        server
    }

    fn text(&self) -> String {
        String::from_utf8_lossy(&self.out.lock().unwrap_or_else(|e| e.into_inner())).into_owned()
    }

    fn wait_for(&self, needle: &str) {
        let deadline = Instant::now() + Duration::from_secs(20);
        while !self.text().contains(needle) {
            assert!(Instant::now() < deadline, "no {needle} in {}", self.text());
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    fn signal(&self, sig: libc::c_int) {
        unsafe {
            libc::kill(self.child.id() as libc::pid_t, sig);
        }
    }

    fn wait(mut self) -> (std::process::ExitStatus, String, String) {
        let status = self.child.wait().expect("wait for the server");
        if let Some(r) = self.reader.take() {
            let _ = r.join();
        }
        let mut err = String::new();
        if let Some(mut e) = self.child.stderr.take() {
            let _ = e.read_to_string(&mut err);
        }
        (status, self.text(), err)
    }

    fn wait_within(mut self, limit: Duration) -> (std::process::ExitStatus, String, String) {
        let deadline = Instant::now() + limit;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                return self.wait();
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        let _ = self.child.kill();
        let (_, out, _) = self.wait();
        panic!("the server still runs {limit:?} after the signal\n{out}");
    }
}

fn positions(text: &str, needles: &[&str]) -> Vec<usize> {
    needles
        .iter()
        .map(|n| text.find(n).unwrap_or_else(|| panic!("no {n} in {text}")))
        .collect()
}

fn stops_cleanly(sig: libc::c_int, name: &str) {
    let dir = TempDir::new();
    let server = Server::start(dir.path());
    server.signal(sig);
    let (status, out, err) = server.wait();

    assert_eq!(status.code(), Some(0), "{status:?}\n{out}\n{err}");
    assert_eq!(err, "");
    let signal = format!("msg=\"shutdown requested\" component=boot signal={name}\n");
    let at = positions(
        &out,
        &[
            "msg=\"tcp listening\" component=tcp addr=127.0.0.1:",
            &signal,
            "msg=\"requests drained\" component=boot duration_ms=",
            "msg=\"data flushed on shutdown\" component=boot duration_ms=",
            "msg=\"shutdown complete\" component=boot uptime=",
        ],
    );
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{out}");
    let last = out.lines().last().unwrap_or_default();
    assert!(last.ends_with('s'), "{last}");
}

#[test]
fn sigterm_stops_the_server_cleanly() {
    stops_cleanly(libc::SIGTERM, "terminated");
}

#[test]
fn sigint_stops_the_server_cleanly() {
    stops_cleanly(libc::SIGINT, "interrupt");
}

fn drain_ms(out: &str) -> f64 {
    let line = out
        .lines()
        .find(|l| l.contains("msg=\"requests drained\""))
        .unwrap_or_else(|| panic!("no requests drained in {out}"));
    let at = line.find("duration_ms=").expect("duration_ms") + "duration_ms=".len();
    let digits: String = line[at..]
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    digits.parse().expect("drain duration")
}

#[test]
fn sigterm_closes_the_idle_tcp_connections_without_waiting_for_the_timeout() {
    let dir = TempDir::new();
    let server = Server::start(dir.path());
    let tcp = SocketAddr::from(([127, 0, 0, 1], server.tcp));

    let silent = TcpStream::connect(tcp).expect("connect");
    let mut answered = TcpStream::connect(tcp).expect("connect");
    answered.write_all(b"PING\n").expect("ping");
    let mut pong = [0u8; 5];
    answered.read_exact(&mut pong).expect("pong");
    assert_eq!(&pong, b"PONG\n");
    let mut partial = TcpStream::connect(tcp).expect("connect");
    partial.write_all(b"PING").expect("partial line");
    std::thread::sleep(Duration::from_millis(200));

    server.signal(libc::SIGTERM);
    let (status, out, err) = server.wait();

    assert_eq!(status.code(), Some(0), "{status:?}\n{out}\n{err}");
    assert_eq!(err, "");
    assert!(!out.contains("msg=\"connection error\""), "{out}");
    assert!(drain_ms(&out) < 4000.0, "{out}");
    for (name, mut conn) in [
        ("silent", silent),
        ("answered", answered),
        ("partial", partial),
    ] {
        let _ = conn.set_read_timeout(Some(Duration::from_secs(5)));
        let mut rest = Vec::new();
        match conn.read_to_end(&mut rest) {
            Ok(_) => assert!(rest.is_empty(), "{name}: {rest:?}"),
            Err(e) => panic!("{name}: {e}"),
        }
    }
}

#[test]
fn sigterm_stops_within_the_drain_timeout_with_a_tcp_client_that_does_not_read() {
    let dir = TempDir::new();
    let server = Server::start_with(dir.path(), "", "debug");
    let mut conn =
        TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], server.tcp))).expect("connect");
    let mut set = b"SET big ".to_vec();
    set.extend(std::iter::repeat_n(b'x', 100_000));
    set.push(b'\n');
    conn.write_all(&set).expect("set");
    let mut ok = [0u8; 3];
    conn.read_exact(&mut ok).expect("answer");
    assert_eq!(&ok, b"OK\n");
    conn.write_all(&b"GET big\n".repeat(400)).expect("gets");
    conn.set_nonblocking(true).expect("nonblocking");
    let mut unread = vec![0u8; 100_000];
    let deadline = Instant::now() + Duration::from_secs(20);
    while !matches!(conn.peek(&mut unread), Ok(n) if n == unread.len()) {
        assert!(Instant::now() < deadline, "no answer to the GET lines");
        std::thread::sleep(Duration::from_millis(5));
    }
    std::thread::sleep(Duration::from_millis(200));

    server.signal(libc::SIGTERM);
    let (status, out, err) = server.wait_within(Duration::from_secs(20));

    assert_eq!(status.code(), Some(0), "{status:?}\n{out}\n{err}");
    assert_eq!(err, "");
    assert!(!out.contains("msg=\"connection error\""), "{out}");
    let drained = drain_ms(&out);
    assert!((4000.0..10000.0).contains(&drained), "{out}");
    let at = positions(
        &out,
        &[
            "msg=\"shutdown requested\" component=boot signal=terminated\n",
            "msg=\"connection ended\" component=tcp op=flush remote_addr=127.0.0.1 err=\"Broken pipe (os error 32)\"\n",
            "msg=\"connection closed\" component=tcp remote_addr=127.0.0.1 duration_ms=",
            "msg=\"requests drained\" component=boot duration_ms=",
            "msg=\"data flushed on shutdown\" component=boot duration_ms=",
            "msg=\"shutdown complete\" component=boot uptime=",
        ],
    );
    assert!(at.windows(2).all(|w| w[0] < w[1]), "{out}");
    drop(conn);
}

#[test]
fn a_second_signal_during_the_shutdown_ends_the_process() {
    use std::os::unix::process::ExitStatusExt;

    let dir = TempDir::new();
    let server = Server::start_with(dir.path(), ", readTimeoutSeconds: 60", "info");

    let mut conn =
        TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], server.http))).expect("connect");
    conn.write_all(
        b"POST /kv/big HTTP/1.1\r\nHost: 127.0.0.1\r\nContent-Length: 100\r\n\r\nonly a part",
    )
    .expect("partial request");
    std::thread::sleep(Duration::from_millis(500));

    server.signal(libc::SIGTERM);
    server.wait_for("msg=\"shutdown requested\"");
    std::thread::sleep(Duration::from_millis(300));
    let draining = server.text();
    server.signal(libc::SIGTERM);
    let (status, out, _) = server.wait();

    assert!(!draining.contains("msg=\"requests drained\""), "{draining}");
    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}\n{out}");
    assert!(!out.contains("msg=\"shutdown complete\""), "{out}");
    let mut rest = Vec::new();
    let _ = conn.read_to_end(&mut rest);
    assert!(rest.is_empty(), "{}", String::from_utf8_lossy(&rest));
}

fn refused<T>(r: Result<T, boot::AddrError>) -> String {
    match r {
        Ok(_) => panic!("the address should be refused"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn listen_addresses_are_reported_without_go_wording() {
    assert_eq!(
        refused(boot::listen_tcp4("::1:8089")),
        "\"::1:8089\" is not a valid address"
    );
    assert_eq!(
        refused(boot::listen_tcp4("127.0.0.1")),
        "\"127.0.0.1\" is not a valid address"
    );
    assert_eq!(
        refused(boot::listen_tcp4("127.0.0.1:70000")),
        "\"127.0.0.1:70000\" is not a valid address"
    );
    assert_eq!(
        refused(boot::listen_tcp4("[::1]:0")),
        "cannot listen on [::1]:0: not an IPv4 address"
    );

    let busy = TcpListener::bind("127.0.0.1:0").expect("busy port");
    let port = busy.local_addr().expect("local address").port();
    assert_eq!(
        refused(boot::listen_tcp4(&format!("127.0.0.1:{port}"))),
        format!("cannot listen on 127.0.0.1:{port}: address already in use")
    );

    let tcp =
        |addr: &str| boot::resolve_tcp_addr(addr).and_then(|(ip, port)| boot::listen_tcp(ip, port));
    assert_eq!(
        refused(tcp("::1:8088")),
        "\"::1:8088\" is not a valid address"
    );
    assert_eq!(
        refused(tcp(&format!("127.0.0.1:{port}"))),
        format!("cannot listen on 127.0.0.1:{port}: address already in use")
    );
    drop(busy);

    let ln = tcp("[::1]:0").expect("an IPv6 literal is accepted by the TCP listener");
    assert!(ln.local_addr().expect("local address").is_ipv6());
}
