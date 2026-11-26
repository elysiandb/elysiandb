use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::cli::BIN;
use super::tempdir::TempDir;

fn free_port() -> u16 {
    let ln = TcpListener::bind("127.0.0.1:0").expect("free port");
    ln.local_addr().expect("local address").port()
}

fn tcp_only_config(port: u16) -> String {
    format!(
        "store:\n  folder: data\n  shards: 4\nserver:\n  http: {{ enabled: false, host: 127.0.0.1, port: 1 }}\n  tcp: {{ enabled: true, host: 127.0.0.1, port: {port} }}\nlog:\n  level: info\n  format: text\nsecurity:\n  authentication:\n    enabled: false\n"
    )
}

struct Server {
    child: Child,
    out: std::sync::Arc<std::sync::Mutex<Vec<u8>>>,
    reader: Option<std::thread::JoinHandle<()>>,
    port: u16,
}

impl Server {
    fn start(dir: &Path) -> Server {
        let port = free_port();
        std::fs::write(dir.join("s.yaml"), tcp_only_config(port)).expect("config");
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
            port,
        };
        server.wait_for("msg=\"tcp listening\"");
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
    server.wait_for("msg=\"ready to serve\"");
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

#[test]
fn a_second_signal_during_the_shutdown_ends_the_process() {
    use std::os::unix::process::ExitStatusExt;

    let dir = TempDir::new();
    let server = Server::start(dir.path());
    server.wait_for("msg=\"ready to serve\"");

    let mut conn =
        TcpStream::connect(SocketAddr::from(([127, 0, 0, 1], server.port))).expect("connect");
    let mut set = b"SET big ".to_vec();
    set.extend(std::iter::repeat_n(b'x', 100_000));
    set.push(b'\n');
    conn.write_all(&set).expect("set");
    let mut ok = [0u8; 3];
    conn.read_exact(&mut ok).expect("answer");
    assert_eq!(&ok, b"OK\n");
    for _ in 0..400 {
        conn.write_all(b"GET big\n").expect("get");
    }
    std::thread::sleep(Duration::from_millis(500));

    server.signal(libc::SIGTERM);
    server.wait_for("msg=\"shutdown requested\"");
    std::thread::sleep(Duration::from_millis(300));
    server.signal(libc::SIGTERM);
    let (status, out, _) = server.wait();

    assert_eq!(status.signal(), Some(libc::SIGTERM), "{status:?}\n{out}");
    assert!(!out.contains("msg=\"shutdown complete\""), "{out}");
    drop(conn);
}
