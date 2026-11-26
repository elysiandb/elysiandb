mod common;

#[path = "internal/tempdir.rs"]
mod tempdir;

use std::ffi::OsString;
use std::io;
use std::net::{Ipv4Addr, SocketAddr, TcpListener};
use std::os::unix::ffi::OsStringExt;
use std::process::{Command, Output};
use std::sync::Arc;
use std::time::{Duration, Instant};

use elysiandb::boot;
use elysiandb::configuration::Config;
use elysiandb::globals;
use elysiandb::gocompat::flag::ParseError;
use elysiandb::gocompat::json::{Value, quote_text};
use elysiandb::log::{self, Options};
use elysiandb::sync::{Mutex, MutexGuard};

use tempdir::TempDir;

const BIN: &str = env!("CARGO_BIN_EXE_elysian_bench");

const FLAG_KINDS: [(&str, &str); 11] = [
    ("pair", "bool"),
    ("addr", "string"),
    ("vus", "int"),
    ("duration", "duration"),
    ("rps", "int"),
    ("keys", "int"),
    ("setratio", "float64"),
    ("payload", "int"),
    ("warmup", "bool"),
    ("live", "bool"),
    ("auth", "string"),
];

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

static SERIAL: Mutex<()> = Mutex::new(());

struct Server {
    addr: SocketAddr,
    _dir: TempDir,
    _serial: MutexGuard<'static, ()>,
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = elysiandb::storage::close();
        log::set_output(None, false);
        let _ = log::configure(&Options::default());
    }
}

fn start_tcp(token: Option<&str>) -> Server {
    let serial = SERIAL.lock();
    if let Err(err) = log::configure(&Options {
        level: "error".to_string(),
        ..Options::default()
    }) {
        panic!("{err}");
    }
    log::set_output(Some(Box::new(io::sink())), false);

    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().join("data").to_string_lossy().into_owned();
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
    boot::init_db();

    let before = boot::tcp_listener_addr();
    std::thread::spawn(boot::init_tcp);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(addr) = boot::tcp_listener_addr()
            && Some(addr) != before
        {
            assert_ne!(addr.port(), 8088);
            return Server {
                addr,
                _dir: dir,
                _serial: serial,
            };
        }
        assert!(Instant::now() < deadline, "the TCP listener did not start");
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn closed_port() -> SocketAddr {
    let ln = TcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("bind");
    ln.local_addr().expect("addr")
}

fn bench(args: &[&str]) -> Output {
    Command::new(BIN)
        .args(args)
        .env_remove("RUST_BACKTRACE")
        .output()
        .expect("run elysian_bench")
}

fn run_against(addr: SocketAddr, args: &[&str]) -> (String, String) {
    let addr = addr.to_string();
    let mut all = vec!["-addr", addr.as_str()];
    all.extend_from_slice(args);
    let out = bench(&all);
    let stdout = String::from_utf8_lossy(&out.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&out.stderr).into_owned();
    assert_eq!(out.status.code(), Some(0), "{stdout}{stderr}");
    (stdout, stderr)
}

fn line<'a>(stdout: &'a str, label: &str) -> &'a str {
    stdout
        .lines()
        .find_map(|l| l.strip_prefix(label))
        .unwrap_or_else(|| panic!("no {label:?} line in {stdout}"))
}

fn count(stdout: &str, label: &str) -> i64 {
    line(stdout, label)
        .trim()
        .parse()
        .unwrap_or_else(|e| panic!("{label}: {e} in {stdout}"))
}

#[test]
fn help_prints_the_usage_and_exits_with_zero() {
    for flag in ["-h", "-help", "--help"] {
        let out = bench(&[flag]);
        assert_eq!(out.status.code(), Some(0), "{flag}");
        assert!(out.stdout.is_empty(), "{flag}");
        assert_eq!(String::from_utf8_lossy(&out.stderr), USAGE, "{flag}");
    }
}

#[test]
fn flag_errors_print_the_message_then_the_usage_and_exit_with_two() {
    for (args, message) in [
        (&["-zzz"][..], "unknown flag: -zzz"),
        (&["-vus"][..], "missing value for flag: -vus"),
        (
            &["-vus", "abc"][..],
            "invalid value \"abc\" for -vus: \"abc\" is not a valid integer",
        ),
        (
            &["-setratio", "x"][..],
            "invalid value \"x\" for -setratio: \"x\" is not a valid number",
        ),
        (
            &["-duration", "10"][..],
            "invalid value \"10\" for -duration: \"10\" is not a valid duration",
        ),
        (
            &["-live=maybe"][..],
            "invalid value \"maybe\" for -live: \"maybe\" is not a valid boolean",
        ),
    ] {
        let out = bench(args);
        assert_eq!(out.status.code(), Some(2), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert_eq!(
            String::from_utf8_lossy(&out.stderr),
            format!("{message}\n{USAGE}"),
            "{args:?}"
        );
    }
}

fn expected_reason(kind: &str, class: &str, value: &[u8]) -> String {
    let q = quote_text(value);
    match (kind, class) {
        (_, "range") => format!("{q} is out of range"),
        ("int", "syntax") => format!("{q} is not a valid integer"),
        ("float64", "syntax") => format!("{q} is not a valid number"),
        ("bool", "syntax") => format!("{q} is not a valid boolean"),
        ("duration", "syntax") => format!("{q} is not a valid duration"),
        _ => panic!("unexpected reason {kind} {class}"),
    }
}

#[test]
fn the_binary_answers_the_flag_errors_of_the_bench_corpus() {
    let rel = "gocompat/flag/cases.jsonl";
    let mut replayed = 0;
    for row in &common::read_jsonl(rel) {
        if common::str_field(row, "set") != "bench" {
            continue;
        }
        let Some(class) = row.get("error").and_then(Value::as_str) else {
            continue;
        };
        let field = |k: &str| common::unhex(common::str_field(row, k));
        let err = match class {
            "help" => ParseError::Help,
            "syntax" => ParseError::BadSyntax(field("arg")),
            "unknown" => ParseError::Unknown(field("name")),
            "missing" => ParseError::MissingValue(field("name")),
            "invalid" => {
                let name = field("name");
                let value = field("value");
                let kind = FLAG_KINDS
                    .iter()
                    .find(|(n, _)| n.as_bytes() == name.as_slice())
                    .map(|(_, k)| *k)
                    .unwrap_or_else(|| panic!("unknown flag in {row:?}"));
                let reason = expected_reason(kind, common::str_field(row, "reason"), &value);
                ParseError::InvalidValue {
                    name,
                    value,
                    reason,
                }
            }
            _ => panic!("unknown error class {class}"),
        };
        let args: Vec<OsString> = row
            .get("args")
            .and_then(Value::as_array)
            .unwrap_or_else(|| panic!("no args in {row:?}"))
            .iter()
            .map(|a| OsString::from_vec(common::unhex(a.as_str().unwrap_or(""))))
            .collect();

        let out = Command::new(BIN)
            .args(&args)
            .env_remove("RUST_BACKTRACE")
            .output()
            .expect("run elysian_bench");
        let mut want = err.message().unwrap_or_default();
        if !want.is_empty() {
            want.push(b'\n');
        }
        want.extend_from_slice(USAGE.as_bytes());
        assert_eq!(out.status.code(), Some(err.exit_code()), "{args:?}");
        assert!(out.stdout.is_empty(), "{args:?}");
        assert!(
            out.stderr == want,
            "{args:?}: got {:?}, want {:?}",
            String::from_utf8_lossy(&out.stderr),
            String::from_utf8_lossy(&want)
        );
        replayed += 1;
    }
    assert_eq!(replayed, 43);
}

#[test]
fn a_short_run_reports_its_settings_and_results() {
    let server = start_tcp(None);
    let (stdout, stderr) = run_against(
        server.addr,
        &["-vus", "2", "-duration", "1s", "-keys", "10", "-live=false"],
    );
    assert!(stderr.is_empty(), "{stderr}");

    let header = format!(
        "\n  █ ElysianDB TCP Bench\n  ─────────────────────\n  addr      : {}\n  vus       : 2\n  duration  : 1s\n  rps limit : unlimited\n  keys      : 10\n  set ratio : 0.10\n  payload   : 16 bytes\n  warmup    : true\n\n  ▶ Warmup: SET keys…\n  ✓ Warmup completed in ",
        server.addr
    );
    assert!(stdout.starts_with(&header), "{stdout}");
    for label in [
        "  ─────────────────────────────────────────────────────────────────",
        "  █ RESULTS",
        "  Latency (ms)",
    ] {
        assert!(stdout.lines().any(|l| l == label), "{label} in {stdout}");
    }
    let requests = count(&stdout, "  requests       : ");
    assert!(requests > 0, "{stdout}");
    assert_eq!(count(&stdout, "  successes      : "), requests);
    assert_eq!(count(&stdout, "  errors         : "), 0);
    assert_eq!(count(&stdout, "  misses (GET)   : "), 0);
    assert!(line(&stdout, "  throughput     : ").ends_with(" req/s"));
    assert!(line(&stdout, "  transfer in    : ").ends_with("B/s"));
    assert_eq!(
        line(&stdout, "  success        : "),
        format!("[{}] 100.0%", "█".repeat(40))
    );
    assert!(!stdout.contains("pairs/s"), "{stdout}");
    assert!(!stdout.contains('\r'), "{stdout}");
    assert!(
        stdout.ends_with("  ─────────────────────────────────────────────────────────────────\n")
    );

    let (stdout, _) = run_against(
        server.addr,
        &[
            "-vus",
            "2",
            "-duration",
            "2s",
            "-keys",
            "10",
            "-warmup=false",
        ],
    );
    assert!(
        stdout.contains("\n\r  1s  | VUs: 2   | req/s: "),
        "{stdout:?}"
    );
    let live = stdout
        .split('\r')
        .nth(1)
        .unwrap_or_else(|| panic!("no live line in {stdout:?}"));
    for part in [
        " | p50: ",
        " | p95: ",
        " | errs: 0      | miss: 0      | in: ",
        " | out: ",
    ] {
        assert!(live.contains(part), "{part} in {live:?}");
    }
    assert_eq!(count(&stdout, "  errors         : "), 0);
}

#[test]
fn a_paired_run_with_a_rate_limit_and_credentials_has_no_errors() {
    let server = start_tcp(Some("bench-token"));
    let (stdout, _) = run_against(
        server.addr,
        &[
            "-pair",
            "-vus",
            "2",
            "-duration",
            "1s",
            "-keys",
            "5",
            "-rps",
            "200",
            "-payload",
            "0",
            "-warmup=false",
            "-live=false",
            "-auth",
            "bench-token",
        ],
    );
    assert!(
        stdout.contains("  rps limit : 200\n  keys      : 5\n  mode      : SET→GET paired (1:1)\n  set ratio : (ignored in -pair)\n  payload   : 0 bytes\n  warmup    : false\n  mode      : SET→GET paired (1:1)\n\n"),
        "{stdout}"
    );
    assert!(!stdout.contains("Warmup"), "{stdout}");
    let requests = count(&stdout, "  requests       : ");
    assert!(requests > 0 && requests <= 450, "{stdout}");
    assert_eq!(count(&stdout, "  errors         : "), 0);
    assert_eq!(count(&stdout, "  misses (GET)   : "), 0);
    assert!(line(&stdout, "  throughput (pairs) : ").ends_with(" pairs/s"));

    let (stdout, _) = run_against(
        server.addr,
        &[
            "-vus",
            "2",
            "-duration",
            "10s",
            "-warmup=false",
            "-live=false",
            "-auth",
            "wrong",
        ],
    );
    assert_eq!(count(&stdout, "  requests       : "), 2);
    assert_eq!(count(&stdout, "  errors         : "), 2);
}

#[test]
fn connections_that_fail_are_counted_as_errors() {
    let addr = closed_port();
    let (stdout, _) = run_against(addr, &["-vus", "2", "-keys", "3", "-live=false"]);
    assert!(
        stdout.contains("  ⚠ Warmup completed with 4 errors in "),
        "{stdout}"
    );
    assert_eq!(count(&stdout, "  requests       : "), 2);
    assert_eq!(count(&stdout, "  successes      : "), 0);
    assert_eq!(count(&stdout, "  errors         : "), 2);
    assert_eq!(
        line(&stdout, "  success        : "),
        format!("[{}]   0.0%", "░".repeat(40))
    );
    assert_eq!(line(&stdout, "    min          : "), "  0.00");
}

#[test]
fn no_key_to_pick_ends_the_run_like_an_unrecovered_panic() {
    let server = start_tcp(None);
    let addr = server.addr.to_string();
    let out = bench(&[
        "-addr",
        &addr,
        "-vus",
        "1",
        "-keys",
        "0",
        "-duration",
        "5s",
        "-live=false",
    ]);
    assert_eq!(out.status.code(), Some(2));
    assert_eq!(
        String::from_utf8_lossy(&out.stderr),
        "fatal error: invalid argument to Intn\n"
    );
}
