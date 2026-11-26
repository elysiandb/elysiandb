use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use elysiandb::gocompat::json::Value;

use super::archives::archive;
use super::backup_archives::expected_reason;
use super::common::{read_jsonl, str_field};
use super::pty;
use super::tempdir::TempDir;

pub(super) const BIN: &str = env!("CARGO_BIN_EXE_elysiandb");

pub(super) const USAGE: &str = "Usage: elysiandb [-config <path>] [<command> [<arguments>]]

Options:
  -config <path>  Path to configuration file (default \"elysian.yaml\")

Commands: backup, change-password, create-user, delete-user, export-legacy, help, reset, restore, server (default).
Run \"elysiandb help\" to describe them.
";

const GO_USAGE: &str = "\u{27e8}go:usage\u{27e9}";
const GO_FLAG: &str = "\u{27e8}go:flag\u{27e9}";
const GO_OS: &str = "\u{27e8}go:os\u{27e9}";
const GO_YAML_SYNTAX: &str = "\u{27e8}go:yaml-syntax\u{27e9}";
const GO_GZIP: &str = "\u{27e8}go:gzip\u{27e9}";
const GO_HTTP_CLIENT: &str = "\u{27e8}go:http-client\u{27e9}";

const RED: &str = "\x1b[38;5;196m";
const RESET: &str = "\x1b[0m";

const ROWS: usize = 115;
const LOCAL_ROWS: usize = 32;
const INVALID_ROWS: usize = 76;
const REMOTE_ROWS: usize = 7;

const NO_SUCH_FILE: &str = "No such file or directory (os error 2)";
const NOT_A_TERMINAL: &str = "Inappropriate ioctl for device (os error 25)";

const MARKERS: [(&str, &str, &str); 15] = [
    (
        "remote refused",
        GO_HTTP_CLIENT,
        "request to http://127.0.0.1:<PORT>/api/backup failed: connection refused",
    ),
    ("help -h", GO_USAGE, USAGE),
    ("help -help", GO_USAGE, USAGE),
    ("help --help", GO_USAGE, USAGE),
    ("unknown flag", GO_FLAG, "unknown flag: -x"),
    ("unknown flag", GO_USAGE, USAGE),
    ("missing value", GO_FLAG, "missing value for flag: -config"),
    ("missing value", GO_USAGE, USAGE),
    ("missing config", GO_OS, NO_SUCH_FILE),
    ("missing config server", GO_OS, NO_SUCH_FILE),
    (
        "invalid yaml",
        GO_YAML_SYNTAX,
        "line 1: did not find expected ',' or ']'",
    ),
    (
        "backup missing folder",
        GO_OS,
        "stat missing-folder: No such file or directory (os error 2)",
    ),
    ("create-user user mode", GO_OS, NOT_A_TERMINAL),
    ("change-password user mode", GO_OS, NOT_A_TERMINAL),
    ("restore not gzip", GO_GZIP, "invalid gzip header"),
];

const GO_TEXTS: [(&str, &str, &str, &str); 1] = [(
    "restore missing file",
    "stdout",
    "open no-such.tar.gz: no such file or directory",
    "open no-such.tar.gz: No such file or directory (os error 2)",
)];

const REWORDED: [(&str, &str); 4] = [
    (
        "archives/invalid/entity-entry-empty-name.tar.gz",
        "invalid entity entry {Name: Documents:1}",
    ),
    (
        "archives/invalid/entity-entry-negative.tar.gz",
        "invalid entity entry {Name:books Documents:-1}",
    ),
    (
        "archives/invalid/schema-missing-id.tar.gz",
        "describes %!q(<nil>) instead of",
    ),
    (
        "archives/invalid/schema-number-id.tar.gz",
        "describes %!q(float64=1) instead of",
    ),
];

pub(super) fn cli_config(mode: &str, port: u16) -> String {
    let auth = match mode {
        "token" => "    enabled: true\n    mode: token\n    token: cli-token\n",
        "user" => "    enabled: true\n    mode: user\n",
        "basic" => "    enabled: true\n    mode: basic\n",
        _ => "    enabled: false\n",
    };
    format!(
        "store:\n  folder: data\n  shards: 4\nserver:\n  http: {{ enabled: true, host: 127.0.0.1, port: {port} }}\n  tcp: {{ enabled: false, host: 127.0.0.1, port: 1 }}\nlog:\n  level: info\n  format: text\nsecurity:\n  authentication:\n{auth}"
    )
}

fn compat_archive() -> Vec<u8> {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    for dir in ["tests", "test"] {
        let p = root
            .join(dir)
            .join("compat/testdata/archives/v0.2.4.tar.gz");
        if let Ok(data) = std::fs::read(&p) {
            return data;
        }
    }
    panic!("the v0.2.4 compatibility archive is missing");
}

fn write(path: &Path, data: &[u8]) {
    std::fs::write(path, data).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
}

fn digits(b: &[u8], at: usize, n: usize) -> bool {
    at + n <= b.len() && b[at..at + n].iter().all(u8::is_ascii_digit)
}

fn time_at(b: &[u8], i: usize) -> Option<usize> {
    let shape = digits(b, i, 4)
        && b.get(i + 4) == Some(&b'-')
        && digits(b, i + 5, 2)
        && b.get(i + 7) == Some(&b'-')
        && digits(b, i + 8, 2)
        && b.get(i + 10) == Some(&b'T')
        && digits(b, i + 11, 2)
        && b.get(i + 13) == Some(&b':')
        && digits(b, i + 14, 2)
        && b.get(i + 16) == Some(&b':')
        && digits(b, i + 17, 2);
    if !shape {
        return None;
    }
    let mut j = i + 19;
    if b.get(j) == Some(&b'.') && digits(b, j + 1, 1) {
        j += 1;
        while digits(b, j, 1) {
            j += 1;
        }
    }
    match b.get(j) {
        Some(b'Z') => Some(j + 1),
        Some(b'+' | b'-')
            if digits(b, j + 1, 2) && b.get(j + 3) == Some(&b':') && digits(b, j + 4, 2) =>
        {
            Some(j + 6)
        }
        _ => None,
    }
}

fn stamp_at(b: &[u8], i: usize) -> Option<usize> {
    if !digits(b, i, 8) {
        return None;
    }
    if b.get(i + 8) == Some(&b'-') && digits(b, i + 9, 6) {
        return Some(i + 15);
    }
    if b.get(i + 8) == Some(&b'T') && digits(b, i + 9, 6) {
        if b.get(i + 15) == Some(&b'Z') {
            return Some(i + 16);
        }
        return Some(i + 15);
    }
    None
}

fn replace_matches(s: &str, find: impl Fn(&[u8], usize) -> Option<usize>, with: &str) -> String {
    let b = s.as_bytes();
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if let Some(end) = find(b, i) {
            out.extend_from_slice(with.as_bytes());
            i = end;
            continue;
        }
        out.push(b[i]);
        i += 1;
    }
    String::from_utf8(out).expect("masked output is UTF-8")
}

fn mask_after(s: &str, prefix: &str, chars: impl Fn(u8) -> bool, with: &str) -> String {
    let p = prefix.as_bytes();
    replace_matches(
        s,
        |b, i| {
            if !b[i..].starts_with(p) {
                return None;
            }
            let mut j = i + p.len();
            while j < b.len() && chars(b[j]) {
                j += 1;
            }
            (j > i + p.len()).then_some(j)
        },
        &format!("{prefix}{with}"),
    )
}

fn mask_backup_bytes(s: &str) -> String {
    let mut out = Vec::new();
    for line in s.split_inclusive('\n') {
        if let Some(at) = line.find("msg=\"backup written\"") {
            let rest = &line[at..];
            let mut cut = None;
            let mut from = 0;
            while let Some(k) = rest[from..].find(" bytes=") {
                let start = from + k + " bytes=".len();
                let end = start + rest[start..].bytes().take_while(u8::is_ascii_digit).count();
                if end > start {
                    cut = Some((at + start, at + end));
                    break;
                }
                from = start;
            }
            if let Some((a, b)) = cut {
                out.push(format!("{}<N>{}", &line[..a], &line[b..]));
                continue;
            }
        }
        out.push(line.to_string());
    }
    out.concat()
}

pub(super) fn mask(s: &str, dir: &Path) -> String {
    let mut s = s.replace(BIN, "<BIN>");
    s = s.replace(&*dir.to_string_lossy(), "<DIR>");
    s = replace_matches(&s, time_at, "<T>");
    s = replace_matches(&s, stamp_at, "<STAMP>");
    s = mask_after(&s, "127.0.0.1:", |c| c.is_ascii_digit(), "<PORT>");
    for key in [
        "duration_ms=",
        "startup_ms=",
        "took_ms=",
        "estimated_seconds=",
    ] {
        s = mask_after(&s, key, |c| c.is_ascii_digit() || c == b'.', "<N>");
    }
    mask_backup_bytes(&s)
}

pub(super) struct Run {
    pub(super) stdout: String,
    pub(super) stderr: String,
    pub(super) exit: i64,
}

pub(super) fn run_cli(dir: &Path, args: &[String], stdin: &str, env: &[(String, String)]) -> Run {
    let mut cmd = Command::new(BIN);
    cmd.args(args)
        .current_dir(dir)
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("HOME", dir)
        .env("TZ", "UTC")
        .env("NO_COLOR", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (k, v) in env {
        cmd.env(k, v);
    }
    let mut child = cmd
        .spawn()
        .unwrap_or_else(|e| panic!("cannot start {BIN}: {e}"));
    if let Some(mut input) = child.stdin.take() {
        let _ = input.write_all(stdin.as_bytes());
    }
    let out = child.wait_with_output().expect("wait for the CLI");
    Run {
        stdout: mask(&String::from_utf8_lossy(&out.stdout), dir),
        stderr: mask(&String::from_utf8_lossy(&out.stderr), dir),
        exit: i64::from(out.status.code().unwrap_or(-1)),
    }
}

fn strings(row: &Value, key: &str) -> Vec<String> {
    match row.get(key) {
        Some(Value::Array(items)) => items
            .iter()
            .map(|v| v.as_str().expect("string item").to_string())
            .collect(),
        _ => Vec::new(),
    }
}

pub(super) fn env_of(row: &Value) -> Vec<(String, String)> {
    strings(row, "env")
        .into_iter()
        .map(|kv| {
            let (k, v) = kv.split_once('=').expect("KEY=VALUE");
            (k.to_string(), v.to_string())
        })
        .collect()
}

fn resolve(name: &str, stream: &str, text: &str) -> String {
    let mut out = text.to_string();
    for (case, marker, d27) in MARKERS {
        if case == name {
            out = out.replacen(marker, d27, 1);
        }
    }
    for (case, on, go, d27) in GO_TEXTS {
        if case == name && on == stream {
            assert!(
                out.contains(go),
                "{name}: the Go text is not where expected"
            );
            out = out.replace(go, d27);
        }
    }
    assert!(
        !out.contains('\u{27e8}'),
        "{name}: unresolved marker in {out:?}"
    );
    out
}

fn setup_local(dir: &Path, name: &str) {
    std::fs::create_dir_all(dir.join("out")).expect("out folder");
    write(&dir.join("out/keep"), b"x");
    write(&dir.join("c.yaml"), cli_config("", 1).as_bytes());
    write(&dir.join("u.yaml"), cli_config("user", 1).as_bytes());
    write(&dir.join("archive.tar.gz"), &compat_archive());
    write(&dir.join("garbage.tar.gz"), b"not a gzip stream");
    match name {
        "invalid yaml" => write(&dir.join("bad.yaml"), b"store: [x\n"),
        "validation error" => write(&dir.join("v.yaml"), b"log:\n  level: loud\n"),
        _ => {}
    }
}

fn check(name: &str, row: &Value, got: &Run, stdout: &str, stderr: &str) {
    assert_eq!(got.stdout, stdout, "{name}: stdout");
    assert_eq!(got.stderr, stderr, "{name}: stderr");
    assert_eq!(
        Some(got.exit),
        row.get("exit").and_then(Value::as_i64),
        "{name}: exit code"
    );
}

fn expected_rows() -> BTreeMap<String, Value> {
    read_jsonl("ops/archives/expected.jsonl")
        .into_iter()
        .map(|row| (format!("archives/{}", str_field(&row, "file")), row))
        .collect()
}

fn invalid_restore(name: &str, row: &Value, expected: &BTreeMap<String, Value>) -> String {
    let file = str_field(row, "archive");
    let exp = expected
        .get(file)
        .unwrap_or_else(|| panic!("{name}: {file} is not in expected.jsonl"));
    let go_reason = str_field(exp, "reason");
    let data = archive(file.strip_prefix("archives/").expect("archives/ prefix"));
    let reason = expected_reason(exp, &data).unwrap_or_else(|| panic!("{name}: no reason"));

    let stdout = str_field(row, "stdout");
    let lines: Vec<&str> = stdout.split_inclusive('\n').collect();
    let at = lines
        .iter()
        .position(|l| l.contains("Restore failed: "))
        .unwrap_or_else(|| panic!("{name}: no failure line"));
    let go_line = format!("{RED}Restore failed: {go_reason}{RESET}\n");
    match REWORDED.iter().find(|(f, _)| *f == file) {
        Some((_, go_text)) => assert!(
            lines[at].contains(go_text) && lines[at] != go_line,
            "{name}: the Go wording replaced by D27 is not where expected"
        ),
        None => assert_eq!(lines[at], go_line, "{name}: failure line"),
    }

    let mut out: Vec<String> = lines.iter().map(|l| l.to_string()).collect();
    out[at] = format!("{RED}Restore failed: {reason}{RESET}\n");
    out.concat()
}

#[test]
fn cli_golden_matches_go() {
    let rows = read_jsonl("ops/cli/cases.jsonl");
    assert_eq!(rows.len(), ROWS);
    let expected = expected_rows();

    let (mut local, mut invalid, mut remote) = (0usize, 0usize, Vec::new());
    for (i, row) in rows.iter().enumerate() {
        let name = str_field(row, "name");
        let args = strings(row, "args");
        let stdin = str_field(row, "stdin");

        if name.starts_with("remote ") {
            remote.push(row);
            continue;
        }

        let dir = TempDir::new();
        let path = dir.path().join(format!("case{i:03}"));
        std::fs::create_dir(&path).expect("case folder");

        if row.get("archive").is_some() {
            write(&path.join("c.yaml"), cli_config("", 1).as_bytes());
            let file = str_field(row, "archive");
            write(
                &path.join("archive.tar.gz"),
                &archive(file.strip_prefix("archives/").expect("archives/ prefix")),
            );
            let got = run_cli(&path, &args, stdin, &env_of(row));
            let stdout = invalid_restore(name, row, &expected);
            check(name, row, &got, &stdout, str_field(row, "stderr"));
            invalid += 1;
            continue;
        }

        setup_local(&path, name);
        let got = run_cli(&path, &args, stdin, &env_of(row));
        let stdout = resolve(name, "stdout", str_field(row, "stdout"));
        let stderr = resolve(name, "stderr", str_field(row, "stderr"));
        check(name, row, &got, &stdout, &stderr);
        local += 1;
    }

    assert_eq!(local, LOCAL_ROWS);
    assert_eq!(invalid, INVALID_ROWS);
    assert_eq!(replay_remote(&remote), REMOTE_ROWS);
    assert_eq!(local + invalid + remote.len(), ROWS);
}

fn free_port() -> u16 {
    loop {
        let ln = std::net::TcpListener::bind("127.0.0.1:0").expect("free port");
        let port = ln.local_addr().expect("local address").port();
        if port != 8088 && port != 8089 {
            return port;
        }
    }
}

fn remote_mode(name: &str) -> &'static str {
    match name {
        "remote off" | "remote refused" => "off",
        "remote token" | "remote token wrong" => "token",
        "remote user env password" | "remote user wrong password" => "user",
        "remote basic" => "basic",
        other => panic!("unknown remote case {other}"),
    }
}

fn run_remote(dir: &Path, row: &Value, port: u16) {
    let name = str_field(row, "name");
    let url = format!("http://127.0.0.1:{port}");
    let args: Vec<String> = strings(row, "args")
        .into_iter()
        .map(|a| a.replace("http://127.0.0.1:<PORT>", &url))
        .collect();
    let got = run_cli(dir, &args, str_field(row, "stdin"), &env_of(row));
    let stdout = resolve(name, "stdout", str_field(row, "stdout"));
    let stderr = resolve(name, "stderr", str_field(row, "stderr"));
    check(name, row, &got, &stdout, &stderr);
}

fn stop_server(server: &mut std::process::Child, mode: &str) {
    unsafe {
        libc::kill(server.id() as libc::pid_t, libc::SIGINT);
    }
    let status = server.wait().expect("wait for the server");
    assert_eq!(status.code(), Some(0), "{mode} server: {status:?}");
}

fn replay_remote(rows: &[&Value]) -> usize {
    let mut played = 0;
    for mode in ["off", "token", "user", "basic"] {
        let cases: Vec<&Value> = rows
            .iter()
            .copied()
            .filter(|r| {
                let name = str_field(r, "name");
                name != "remote refused" && remote_mode(name) == mode
            })
            .collect();
        let dir = TempDir::new();
        std::fs::create_dir(dir.path().join("out")).expect("out folder");
        let port = free_port();
        write(
            &dir.path().join("s.yaml"),
            cli_config(mode, port).as_bytes(),
        );
        let mut server = Command::new(BIN)
            .args(["-config", "s.yaml"])
            .current_dir(dir.path())
            .env_clear()
            .env("PATH", std::env::var_os("PATH").unwrap_or_default())
            .env("HOME", dir.path())
            .env("TZ", "UTC")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("start the server");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(20);
        while std::net::TcpStream::connect(("127.0.0.1", port)).is_err() {
            assert!(
                std::time::Instant::now() < deadline,
                "the {mode} server did not start"
            );
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        std::thread::sleep(std::time::Duration::from_millis(300));

        for row in &cases {
            run_remote(dir.path(), row, port);
            played += 1;
        }

        stop_server(&mut server, mode);
    }

    for row in rows
        .iter()
        .filter(|r| str_field(r, "name") == "remote refused")
    {
        let dir = TempDir::new();
        std::fs::create_dir(dir.path().join("out")).expect("out folder");
        write(&dir.path().join("s.yaml"), cli_config("off", 1).as_bytes());
        run_remote(dir.path(), row, free_port());
        played += 1;
    }

    played
}

const RESTORE_ROWS: usize = 19;

const RESTORE_CONFIG: &str = "store:\n  folder: data\n  shards: 4\nserver:\n  http: { enabled: true, host: 127.0.0.1, port: 1 }\n  tcp: { enabled: true, host: 127.0.0.1, port: 2 }\napi:\n  schema:\n    enabled: true\nlog:\n  level: info\n  format: text\nsecurity:\n  authentication:\n    enabled: false\n";

#[test]
fn restore_rows_print_the_boot_lines_of_go() {
    let rows = read_jsonl("ops/archives/restore.jsonl");
    assert_eq!(rows.len(), RESTORE_ROWS);

    for (i, row) in rows.iter().enumerate() {
        let name = str_field(row, "name");
        let dir = TempDir::new();
        let path = dir.path().join(format!("restore{i:02}"));
        std::fs::create_dir_all(path.join("out")).expect("case folder");
        write(&path.join("s.yaml"), RESTORE_CONFIG.as_bytes());
        write(
            &path.join("archive.tar.gz"),
            &archive(str_field(row, "file")),
        );

        let got = run_cli(
            &path,
            &strings(row, "args"),
            str_field(row, "stdin"),
            &env_of(row),
        );
        check(
            name,
            row,
            &got,
            str_field(row, "stdout"),
            str_field(row, "stderr"),
        );
    }
}

fn create_user_on_terminal(password: &[u8], confirm: Option<&[u8]>) -> (Vec<u8>, i64) {
    let dir = TempDir::new();
    write(&dir.path().join("u.yaml"), cli_config("user", 1).as_bytes());

    let mut term = pty::open();
    let mut child = Command::new(BIN)
        .args(["-config", "u.yaml", "create-user"])
        .current_dir(dir.path())
        .env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default())
        .env("TZ", "UTC")
        .stdin(Stdio::from(term.slave_copy()))
        .stdout(Stdio::from(term.slave_copy()))
        .stderr(Stdio::null())
        .spawn()
        .expect("start the CLI");

    assert!(term.wait_for(b"Enter new username: "), "no username prompt");
    term.type_text(b"ann\n");
    assert!(term.wait_for(b"Choice: "), "no role prompt");
    term.type_text(b"2\n");
    assert!(term.wait_for(b"Enter new password: "), "no password prompt");
    std::thread::sleep(std::time::Duration::from_millis(300));
    term.type_text(password);
    if let Some(confirm) = confirm {
        assert!(
            term.wait_for(b"Re-enter new password: "),
            "no confirmation prompt"
        );
        std::thread::sleep(std::time::Duration::from_millis(300));
        term.type_text(confirm);
    }

    let status = child.wait().expect("wait for the CLI");
    term.drain();
    (term.seen, i64::from(status.code().unwrap_or(-1)))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

#[test]
fn password_prompt_reads_the_terminal_without_echo() {
    let (seen, exit) = create_user_on_terminal(b"secret-11\x08\n", Some(b"secret-1\r"));
    assert_eq!(exit, 0);
    assert!(
        contains(
            &seen,
            b"\x1b[38;5;220mUser 'ann' created successfully.\x1b[0m"
        ),
        "{}",
        String::from_utf8_lossy(&seen)
    );
    assert!(!contains(&seen, b"secret"), "the password was echoed");
}

#[test]
fn password_prompt_at_the_end_of_input_fails() {
    let (seen, exit) = create_user_on_terminal(b"\x04", None);
    assert_eq!(exit, 0);
    assert!(
        contains(
            &seen,
            b"\x1b[38;5;196mFailed to read password: end of input\x1b[0m"
        ),
        "{}",
        String::from_utf8_lossy(&seen)
    );
}
