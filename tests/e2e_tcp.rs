use std::io::{BufRead, BufReader, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use elysiandb::boot;
use elysiandb::configuration::{Config, SecurityConfig};
use elysiandb::globals;
use elysiandb::log::{self, AttrValue, Entry, Level, Options, Query};
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;
use elysiandb::transport::tcp::tcp_routing;

fn wait_tcp_up(timeout: Duration) -> Result<SocketAddr, String> {
    let deadline = Instant::now() + timeout;
    loop {
        let failure = match boot::tcp_listener_addr() {
            Some(addr) => match TcpStream::connect_timeout(&addr, Duration::from_millis(200)) {
                Ok(_) => return Ok(addr),
                Err(e) => e.to_string(),
            },
            None => "the listener has no address yet".to_string(),
        };
        if Instant::now() > deadline {
            return Err(failure);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
}

static TCP_STORE_FOLDER: OnceLock<PathBuf> = OnceLock::new();
static TCP_SERVER_ADDR: OnceLock<SocketAddr> = OnceLock::new();

fn tcp_store_folder() -> &'static PathBuf {
    TCP_STORE_FOLDER.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("elysiandb-tcp-e2e");
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap_or_else(|e| panic!("{e}"));
        dir
    })
}

fn tcp_config(security: &SecurityConfig, port: u16) -> Config {
    let mut cfg = Config::default();
    cfg.store.folder = tcp_store_folder().to_string_lossy().into_owned();
    cfg.store.shards = 8;
    cfg.server.tcp.enabled = true;
    cfg.server.tcp.host = "127.0.0.1".to_string();
    cfg.server.tcp.port = i64::from(port);
    cfg.security = security.clone();
    cfg
}

fn start_tcp_server(security: SecurityConfig) -> SocketAddr {
    let addr = *TCP_SERVER_ADDR.get_or_init(|| {
        globals::set_config(Some(Arc::new(tcp_config(&security, 0))));
        if let Err(e) = storage::open(storage::MODE_SERVER) {
            panic!("cannot open the store: {e}");
        }

        std::thread::spawn(boot::init_tcp);
        match wait_tcp_up(Duration::from_secs(2)) {
            Ok(addr) => addr,
            Err(e) => panic!("TCP server did not start: {e}"),
        }
    });

    globals::set_config(Some(Arc::new(tcp_config(&security, addr.port()))));

    addr
}

fn token_mode() -> SecurityConfig {
    let mut security = SecurityConfig::default();
    security.authentication.enabled = true;
    security.authentication.mode = "token".to_string();
    security.authentication.token = "tcp-token".to_string();
    security
}

fn chop(mut line: Vec<u8>) -> String {
    line.pop();
    String::from_utf8_lossy(&line).into_owned()
}

#[test]
fn tcp_ping_set_mget_get_wildcard_save_reset() {
    let tcp_addr = start_tcp_server(SecurityConfig::default());

    let c = TcpStream::connect_timeout(&tcp_addr, Duration::from_secs(2))
        .unwrap_or_else(|e| panic!("dial: {e}"));
    let mut r = BufReader::new(c.try_clone().unwrap_or_else(|e| panic!("dial: {e}")));

    let write = |s: &str| {
        let _ = c.set_write_timeout(Some(Duration::from_secs(1)));
        if let Err(e) = (&c).write_all(format!("{s}\n").as_bytes()) {
            panic!("write {s:?}: {e}");
        }
    };
    let mut read_line = || {
        let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
        let mut l = Vec::new();
        match r.read_until(b'\n', &mut l) {
            Ok(_) if l.last() == Some(&b'\n') => chop(l),
            Ok(_) => panic!("read: the connection was closed"),
            Err(e) => panic!("read: {e}"),
        }
    };
    macro_rules! read_n {
        ($n:expr) => {{
            let mut out = Vec::with_capacity($n);
            for _ in 0..$n {
                out.push(read_line());
            }
            out
        }};
    }
    let contains = |list: &[String], s: &str| list.iter().any(|x| x == s);

    write("PING");
    let got = read_line();
    if got != "PONG" {
        panic!("want PONG, got {got:?}");
    }

    for set in [
        "SET foo hello",
        "SET bar bat",
        "SET user:1 alice",
        "SET user:2 bob",
    ] {
        write(set);
        let got = read_line();
        if got != "OK" {
            panic!("want OK, got {got:?}");
        }
    }

    write("MGET foo bar baz");
    let got: Vec<String> = read_n!(3);
    let exp = ["hello", "bat", "baz=not found"];
    for (i, want) in exp.iter().enumerate() {
        if got[i] != *want {
            panic!("MGET[{i}]: want {want:?}, got {:?} (all={got:?})", got[i]);
        }
    }

    write("GET foo");
    let got = read_line();
    if got != "foo=hello" {
        panic!("want {:?}, got {got:?}", "foo=hello");
    }

    write("GET user:*");
    let lines: Vec<String> = read_n!(2);
    if !contains(&lines, "user:1=alice") || !contains(&lines, "user:2=bob") {
        panic!("GET user:* expected both user:1=alice and user:2=bob, got {lines:?}");
    }

    write("MGET foo user:* zoo");
    let mixed: Vec<String> = read_n!(4);
    if mixed[0] != "hello" {
        panic!(
            "MGET mixed[0]: want {:?}, got {:?} (all={mixed:?})",
            "hello", mixed[0]
        );
    }
    if !contains(&mixed[1..3], "user:1=alice") || !contains(&mixed[1..3], "user:2=bob") {
        panic!("MGET mixed user block mismatch, got {:?}", &mixed[1..3]);
    }
    if mixed[3] != "zoo=not found" {
        panic!(
            "MGET mixed last line: want {:?}, got {:?} (all={mixed:?})",
            "zoo=not found", mixed[3]
        );
    }

    write("DEL user:*");
    let got = read_line();
    if got != "Deleted 2" {
        panic!("after DEL user:* want {:?}, got {got:?}", "Deleted 2");
    }

    write("GET user:*");
    let got = read_line();
    if got != "user:*=not found" {
        panic!(
            "GET user:* after delete: want {:?}, got {got:?}",
            "user:*=not found"
        );
    }

    write("MGET user:* foo");
    let after_del: Vec<String> = read_n!(2);
    if after_del[0] != "user:*=not found" {
        panic!(
            "MGET user:* foo [0]: want {:?}, got {:?} (all={after_del:?})",
            "user:*=not found", after_del[0]
        );
    }
    if after_del[1] != "hello" {
        panic!(
            "MGET user:* foo [1]: want {:?}, got {:?} (all={after_del:?})",
            "hello", after_del[1]
        );
    }

    write("SAVE");
    let got = read_line();
    if got != "OK" {
        panic!("want OK, got {got:?}");
    }

    write("GET foo");
    let got = read_line();
    if got != "foo=hello" {
        panic!("want {:?}, got {got:?}", "foo=hello");
    }

    write("RESET");
    let got = read_line();
    if got != "OK" {
        panic!("want OK, got {got:?}");
    }

    write("GET foo");
    let got = read_line();
    if got != "foo=not found" {
        panic!("want {:?}, got {got:?}", "foo=not found");
    }

    write("SET to_delete hello");
    let got = read_line();
    if got != "OK" {
        panic!("want OK, got {got:?}");
    }

    write("DEL to_delete");
    let got = read_line();
    if got != "Deleted 1" {
        panic!("after DEL to_delete want {:?}, got {got:?}", "Deleted 1");
    }

    write("GET to_delete");
    let got = read_line();
    if got != "to_delete=not found" {
        panic!("want {:?}, got {got:?}", "to_delete=not found");
    }
}

fn dial_tcp(addr: SocketAddr) -> impl FnMut(&str) -> String {
    let c = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .unwrap_or_else(|e| panic!("dial: {e}"));
    let mut r = BufReader::new(c.try_clone().unwrap_or_else(|e| panic!("dial: {e}")));

    move |line: &str| {
        let _ = c.set_read_timeout(Some(Duration::from_secs(2)));
        let _ = c.set_write_timeout(Some(Duration::from_secs(2)));
        if let Err(e) = (&c).write_all(format!("{line}\r\n").as_bytes()) {
            panic!("write {line:?}: {e}");
        }

        let mut reply = Vec::new();
        match r.read_until(b'\n', &mut reply) {
            Ok(_) if reply.last() == Some(&b'\n') => chop(reply),
            Ok(_) => panic!("read after {line:?}: the connection was closed"),
            Err(e) => panic!("read after {line:?}: {e}"),
        }
    }
}

#[test]
fn tcp_token_mode_requires_auth() {
    let addr = start_tcp_server(token_mode());

    let mut send = dial_tcp(addr);

    let got = send("PING");
    if got != "PONG" {
        panic!("PING before AUTH: want PONG, got {got:?}");
    }

    for cmd in [
        "SET secret hello",
        "GET secret",
        "MGET secret",
        "DEL secret",
        "SAVE",
        "RESET",
    ] {
        let got = send(cmd);
        if got != "ERR auth required" {
            panic!(
                "{cmd} before AUTH: want {:?}, got {got:?}",
                "ERR auth required"
            );
        }
    }

    if storage::get_by_key(b"secret").is_ok() {
        panic!("an unauthenticated SET must not write the key");
    }

    let got = send("AUTH wrong-token");
    if got != "ERR invalid credentials" {
        panic!(
            "AUTH with a wrong token: want {:?}, got {got:?}",
            "ERR invalid credentials"
        );
    }

    let got = send("GET secret");
    if got != "ERR auth required" {
        panic!(
            "GET after a failed AUTH: want {:?}, got {got:?}",
            "ERR auth required"
        );
    }

    let got = send("AUTH tcp-token");
    if got != "OK" {
        panic!("AUTH with the token: want OK, got {got:?}");
    }

    let got = send("SET secret hello");
    if got != "OK" {
        panic!("SET after AUTH: want OK, got {got:?}");
    }

    let got = send("GET secret");
    if got != "secret=hello" {
        panic!("GET after AUTH: want {:?}, got {got:?}", "secret=hello");
    }

    let mut other = dial_tcp(addr);
    let got = other("GET secret");
    if got != "ERR auth required" {
        panic!("authentication must be per connection, got {got:?}");
    }
}

#[test]
fn tcp_user_mode_requires_admin_credentials() {
    let mut user_mode = SecurityConfig::default();
    user_mode.authentication.enabled = true;
    user_mode.authentication.mode = "user".to_string();
    let addr = start_tcp_server(user_mode);

    if let Err(e) = security::create_basic_user(&BasicUser {
        username: b"ops".to_vec(),
        password: b"ops pass".to_vec(),
        role: Role::admin(),
    }) {
        panic!("create admin: {e}");
    }
    if let Err(e) = security::create_basic_user(&BasicUser {
        username: b"alice".to_vec(),
        password: b"alice-pass".to_vec(),
        role: Role::user(),
    }) {
        panic!("create user: {e}");
    }

    let mut send = dial_tcp(addr);

    let got = send("GET foo");
    if got != "ERR auth required" {
        panic!(
            "GET before AUTH: want {:?}, got {got:?}",
            "ERR auth required"
        );
    }

    let got = send("AUTH alice alice-pass");
    if got != "ERR invalid credentials" {
        panic!(
            "AUTH as a regular user: want {:?}, got {got:?}",
            "ERR invalid credentials"
        );
    }

    let got = send("AUTH ops wrong");
    if got != "ERR invalid credentials" {
        panic!(
            "AUTH with a wrong password: want {:?}, got {got:?}",
            "ERR invalid credentials"
        );
    }

    let got = send("AUTH ops ops pass");
    if got != "OK" {
        panic!("AUTH as an admin: want OK, got {got:?}");
    }

    let got = send("SET foo bar");
    if got != "OK" {
        panic!("SET after AUTH: want OK, got {got:?}");
    }
}

#[test]
fn tcp_no_auth_when_authentication_disabled() {
    let addr = start_tcp_server(SecurityConfig::default());

    let mut send = dial_tcp(addr);

    let got = send("SET foo bar");
    if got != "OK" {
        panic!("SET without authentication: want OK, got {got:?}");
    }

    let got = send("AUTH anything");
    if got != "OK" {
        panic!("AUTH without authentication: want OK, got {got:?}");
    }
}

struct RestoreLog;

impl Drop for RestoreLog {
    fn drop(&mut self) {
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
    }
}

#[test]
fn tcp_logs_security_audit_and_debug_events() {
    let addr = start_tcp_server(token_mode());

    if let Err(e) = log::configure(&Options {
        level: "debug".to_string(),
        buffer_lines: 1000,
        ..Options::default()
    }) {
        panic!("{e}");
    }
    log::set_output(Some(Box::new(std::io::sink())), false);
    tcp_routing::reset_throttles();
    let _restore = RestoreLog;

    let mut send = dial_tcp(addr);
    for (line, want) in [
        ("AUTH wrong-token", "ERR invalid credentials"),
        ("AUTH tcp-token", "OK"),
        ("FOO\x1b[31m bar", "ERR"),
        ("RESET", "OK"),
    ] {
        let got = send(line);
        if got != want {
            panic!("unexpected answer {got:?}");
        }
    }

    let find = |q: Query| -> Vec<Arc<Entry>> {
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            let got = log::logs(&q).entries;
            if !got.is_empty() || Instant::now() > deadline {
                return got;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    let text = |s: &str| Some(AttrValue::Str(s.as_bytes().to_vec()));

    let got = find(Query {
        min_level: Level::WARN,
        text: b"tcp auth failed".to_vec(),
        ..Query::default()
    });
    if got.len() != 1 || got[0].component != b"security" {
        panic!("expected a tcp auth failed warning, got {got:?}");
    }

    let got = find(Query {
        component: b"audit".to_vec(),
        text: b"store reset".to_vec(),
        ..Query::default()
    });
    if got.len() != 1
        || got[0].attrs.get("actor").cloned() != text("token")
        || got[0].attrs.get("source").cloned() != text("tcp")
    {
        panic!("expected an audit line for RESET, got {got:?}");
    }

    let unknown = find(Query {
        min_level: Level::DEBUG,
        text: b"unknown command".to_vec(),
        ..Query::default()
    });
    if unknown.len() != 1
        || unknown[0].level != "DEBUG"
        || unknown[0].attrs.get("command").cloned() != text("FOO\x1b[31m")
    {
        panic!("expected one debug line for the unknown command, got {unknown:?}");
    }

    let got = find(Query {
        min_level: Level::DEBUG,
        text: b"connection opened".to_vec(),
        ..Query::default()
    });
    if got.is_empty() {
        panic!("expected a connection opened debug line");
    }
}
