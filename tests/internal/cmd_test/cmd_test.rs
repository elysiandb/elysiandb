use std::io::{self, Cursor, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicI32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use elysiandb::api;
use elysiandb::cmd;
use elysiandb::configuration::Config;
use elysiandb::durable::filepath;
use elysiandb::globals;
use elysiandb::gocompat::errors;
use elysiandb::gocompat::json::{self, Map};
use elysiandb::security::{self, BasicUser, Role};
use elysiandb::storage;

use crate::tempdir::TempDir;

fn strip_ansi(b: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(b.len());
    let mut i = 0;
    while i < b.len() {
        if b[i] == 0x1b {
            i += 1;
            while i < b.len() && (b[i].is_ascii_digit() || b[i] == b'[' || b[i] == b';') {
                i += 1;
            }
            if i < b.len() {
                i += 1;
            }
        } else {
            out.push(b[i]);
            i += 1;
        }
    }
    out
}

fn contains(haystack: &[u8], needle: &str) -> bool {
    haystack
        .windows(needle.len())
        .any(|w| w == needle.as_bytes())
}

fn text(b: &[u8]) -> String {
    String::from_utf8_lossy(b).into_owned()
}

#[derive(Clone, Default)]
struct Buffer(Arc<Mutex<Vec<u8>>>);

impl Buffer {
    fn bytes(&self) -> Vec<u8> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl Write for Buffer {
    fn write(&mut self, p: &[u8]) -> io::Result<usize> {
        self.0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .extend_from_slice(p);
        Ok(p.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct StdinGuard(Option<cmd::Input>);

impl Drop for StdinGuard {
    fn drop(&mut self) {
        cmd::set_stdin(self.0.take());
    }
}

fn mock_stdin(input: &str) -> StdinGuard {
    StdinGuard(cmd::set_stdin(Some(Box::new(Cursor::new(
        input.as_bytes().to_vec(),
    )))))
}

struct PrintfGuard(Option<cmd::PrintfHook>);

impl Drop for PrintfGuard {
    fn drop(&mut self) {
        cmd::set_printf(self.0.take());
    }
}

fn capture_cmd_output() -> (Buffer, PrintfGuard) {
    let buf = Buffer::default();
    let mut sink = buf.clone();
    let previous = cmd::set_printf(Some(Box::new(move |text: &[u8]| {
        let _ = sink.write_all(text);
    })));
    (buf, PrintfGuard(previous))
}

struct StdoutGuard(Option<cmd::Output>);

impl Drop for StdoutGuard {
    fn drop(&mut self) {
        cmd::set_stdout(self.0.take());
    }
}

fn pipe_stdout() -> (Buffer, StdoutGuard) {
    let buf = Buffer::default();
    let previous = cmd::set_stdout(Some(Box::new(buf.clone())));
    (buf, StdoutGuard(previous))
}

struct PasswordGuard(Option<cmd::ReadPasswordHook>);

impl Drop for PasswordGuard {
    fn drop(&mut self) {
        cmd::set_read_password(self.0.take());
    }
}

fn stub_read_password(f: impl FnMut(i32) -> io::Result<Vec<u8>> + Send + 'static) -> PasswordGuard {
    PasswordGuard(cmd::set_read_password(Some(Box::new(f))))
}

struct ExitGuard(Option<cmd::ExitHook>);

impl Drop for ExitGuard {
    fn drop(&mut self) {
        cmd::set_exit(self.0.take());
    }
}

struct ArgsGuard(Option<Vec<Vec<u8>>>);

impl Drop for ArgsGuard {
    fn drop(&mut self) {
        cmd::set_os_args(self.0.take());
    }
}

fn auth(enabled: bool, mode: &str) -> Config {
    let mut cfg = Config::default();
    cfg.security.authentication.enabled = enabled;
    cfg.security.authentication.mode = mode.to_string();
    cfg
}

fn init_cmd_store(mut cfg: Config) -> Option<TempDir> {
    let mut dir = None;
    if cfg.store.folder.is_empty() {
        let d = TempDir::new();
        cfg.store.folder = d.path().to_string_lossy().into_owned();
        dir = Some(d);
    }
    if cfg.store.shards == 0 {
        cfg.store.shards = 8;
    }
    globals::set_config(Some(Arc::new(cfg)));
    storage::load_db();
    storage::load_json_db();
    dir
}

fn set_config(cfg: Config) {
    globals::set_config(Some(Arc::new(cfg)));
}

fn create_basic_user(username: &str, password: &str, role: Role) {
    if let Err(err) = security::create_basic_user(&BasicUser {
        username: username.as_bytes().to_vec(),
        password: password.as_bytes().to_vec(),
        role,
    }) {
        panic!("failed to create user: {err}");
    }
}

fn object(text: &str) -> Map {
    let mut m = Map::new();
    json::unmarshal(text.as_bytes(), &mut m).unwrap_or_else(|e| panic!("{text}: {e}"));
    m
}

fn write_entity(entity: &str, doc: &str) {
    let errs = api::write_entity(entity.as_bytes(), &mut object(doc));
    assert!(errs.is_empty(), "cannot write {doc}");
}

fn read_entity(entity: &str, id: &str) -> Option<Map> {
    api::read_entity_by_id(entity.as_bytes(), id.as_bytes())
}

fn path_arg(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn test_delete_user_auth_disabled() {
    let _dir = init_cmd_store(auth(false, ""));

    let (buf, _restore) = capture_cmd_output();

    cmd::delete_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "Authentication is disabled") {
        panic!("expected message, got: {}", text(&clean));
    }
}

#[test]
fn test_delete_user_wrong_mode() {
    let _dir = init_cmd_store(auth(true, "token"));

    let (buf, _restore) = capture_cmd_output();

    cmd::delete_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "only supports basic") {
        panic!("expected message, got: {}", text(&clean));
    }
}

#[test]
fn test_create_user_auth_disabled() {
    let _dir = init_cmd_store(auth(false, ""));

    let (buf, _restore) = capture_cmd_output();

    cmd::create_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "Authentication is disabled") {
        panic!("expected message, got: {}", text(&clean));
    }
}

#[test]
fn test_create_user_wrong_mode() {
    let _dir = init_cmd_store(auth(true, "token"));

    let (buf, _restore) = capture_cmd_output();

    cmd::create_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "only supports basic") {
        panic!("expected message, got: {}", text(&clean));
    }
}

#[test]
fn test_delete_user_ok() {
    let _dir = init_cmd_store(auth(true, "basic"));

    create_basic_user("john", "x", Role::user());

    let _restore_in = mock_stdin("john\n");

    let (buf, _restore) = capture_cmd_output();

    cmd::delete_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "deleted successfully") {
        panic!("expected deletion success message, got: {}", text(&clean));
    }
}

#[test]
fn test_delete_user_unknown() {
    let _dir = init_cmd_store(auth(true, "basic"));

    let _restore_in = mock_stdin("ghost\n");

    let (buf, _restore) = capture_cmd_output();

    cmd::delete_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "deleted successfully") {
        panic!(
            "expected deletion message even for unknown user, got: {}",
            text(&clean)
        );
    }
}

#[test]
fn test_create_user_ok() {
    let _dir = init_cmd_store(auth(true, "basic"));

    let _restore_in = mock_stdin("alice\n1\n");

    let _password = stub_read_password(|_| Ok(b"secret-password".to_vec()));

    let (buf, _restore) = capture_cmd_output();

    cmd::create_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "created successfully") {
        panic!("expected success message, got: {}", text(&clean));
    }
}

#[test]
fn test_create_user_password_mismatch() {
    let _dir = init_cmd_store(auth(true, "basic"));

    let _restore_in = mock_stdin("bob\n2\n");

    let call = Arc::new(AtomicUsize::new(0));
    let calls = call.clone();
    let _password = stub_read_password(move |_| {
        if calls.load(Ordering::SeqCst) == 0 {
            calls.fetch_add(1, Ordering::SeqCst);
            return Ok(b"pass1".to_vec());
        }
        Ok(b"pass2".to_vec())
    });

    let (buf, stdout) = pipe_stdout();

    cmd::create_user();

    drop(stdout);

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "Passwords do not match") {
        panic!("expected mismatch message, got: {}", text(&clean));
    }
}

#[test]
fn test_get_available_commands() {
    let cmds = cmd::get_available_commands();

    let present = |name: &str| cmds.get(name).is_some_and(|help| !help.is_empty());
    if !present(cmd::SERVER_COMMAND)
        || !present(cmd::CREATE_USER_COMMAND)
        || !present(cmd::DELETE_USER_COMMAND)
        || !present(cmd::HELP_COMMAND)
        || !present(cmd::CHANGE_PASSWORD_COMMAND)
        || !present(cmd::RESET_COMMAND)
        || !present(cmd::BACKUP_COMMAND)
        || !present(cmd::RESTORE_COMMAND)
    {
        panic!("expected all commands to be present");
    }
}

#[test]
fn test_get_handlers() {
    let h = cmd::get_handlers();

    let present = |name: &str| h.contains_key(name);
    if !present(cmd::SERVER_COMMAND)
        || !present(cmd::CREATE_USER_COMMAND)
        || !present(cmd::DELETE_USER_COMMAND)
        || !present(cmd::HELP_COMMAND)
        || !present(cmd::CHANGE_PASSWORD_COMMAND)
        || !present(cmd::RESET_COMMAND)
        || !present(cmd::BACKUP_COMMAND)
        || !present(cmd::RESTORE_COMMAND)
    {
        panic!("expected all handlers to be present");
    }
}

#[test]
fn test_print_help() {
    let (buf, _restore) = capture_cmd_output();

    cmd::print_help();

    let out = strip_ansi(&buf.bytes());

    let expected = [
        cmd::SERVER_COMMAND,
        cmd::CREATE_USER_COMMAND,
        cmd::DELETE_USER_COMMAND,
        cmd::HELP_COMMAND,
        cmd::CHANGE_PASSWORD_COMMAND,
        cmd::RESET_COMMAND,
        cmd::BACKUP_COMMAND,
        cmd::RESTORE_COMMAND,
    ];

    for e in expected {
        if !contains(&out, e) {
            panic!("expected help output to contain {e}, got: {}", text(&out));
        }
    }
}

#[test]
fn test_change_password_auth_disabled() {
    let _dir = init_cmd_store(auth(false, ""));

    let (buf, _restore) = capture_cmd_output();

    cmd::change_password();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "Authentication is disabled") {
        panic!("expected message, got: {}", text(&clean));
    }
}

#[test]
fn test_change_password_wrong_mode() {
    let _dir = init_cmd_store(auth(true, "token"));

    let (buf, _restore) = capture_cmd_output();

    cmd::change_password();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "only supports basic") {
        panic!("expected message, got: {}", text(&clean));
    }
}

#[test]
fn test_change_password_user_not_found() {
    let dir = TempDir::new();
    let mut cfg = auth(true, "basic");
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    set_config(cfg);

    let _restore_in = mock_stdin("ghost\n");

    let calls = Arc::new(AtomicUsize::new(0));
    let counter = calls.clone();
    let _password = stub_read_password(move |_| {
        counter.fetch_add(1, Ordering::SeqCst);
        Ok(b"whatever".to_vec())
    });

    let (buf, _restore) = capture_cmd_output();

    cmd::change_password();

    let clean = strip_ansi(&buf.bytes());

    if !contains(&clean, "Failed to update password") && !contains(&clean, "not found") {
        panic!("expected not found error, got: {}", text(&clean));
    }

    let n = calls.load(Ordering::SeqCst);
    if n != 2 {
        panic!("expected 2 password prompts, got {n}");
    }
}

#[test]
fn test_change_password_mismatch() {
    let _dir = init_cmd_store(auth(true, "basic"));

    create_basic_user("bob", "x", Role::user());

    let _restore_in = mock_stdin("bob\n");

    let call = Arc::new(AtomicUsize::new(0));
    let calls = call.clone();
    let _password = stub_read_password(move |_| {
        if calls.load(Ordering::SeqCst) == 0 {
            calls.fetch_add(1, Ordering::SeqCst);
            return Ok(b"pass1".to_vec());
        }
        Ok(b"pass2".to_vec())
    });

    let (_discarded, stdout) = pipe_stdout();
    let (buf, printf) = capture_cmd_output();

    cmd::change_password();

    drop(stdout);
    drop(printf);

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "Passwords do not match") {
        panic!("expected mismatch message, got: {}", text(&clean));
    }
}

#[test]
fn test_change_password_ok() {
    let _dir = init_cmd_store(auth(true, "basic"));

    create_basic_user("alice", "old", Role::user());

    let _restore_in = mock_stdin("alice\n");

    let _password = stub_read_password(|_| Ok(b"new-password".to_vec()));

    let (buf, _restore) = capture_cmd_output();

    cmd::change_password();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "updated successfully") {
        panic!("expected success message, got: {}", text(&clean));
    }
}

#[test]
fn test_reset_all_abort_on_no() {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    set_config(cfg);

    let _restore_in = mock_stdin("no\n");

    let (buf, _restore) = capture_cmd_output();

    cmd::reset_all();

    let out = buf.bytes();

    if !contains(&out, "Reset aborted.") {
        panic!("expected abort message, got: {}", text(&out));
    }

    if let Err(err) = std::fs::metadata(dir.path()) {
        panic!("expected directory to still exist, got: {err}");
    }
}

#[test]
fn test_reset_all_with_force_deletes_folder() {
    let dir = TempDir::new();
    let mut cfg = Config::default();
    cfg.store.folder = dir.path().to_string_lossy().into_owned();
    set_config(cfg);

    let _args = ArgsGuard(cmd::set_os_args(Some(vec![
        b"elysiandb".to_vec(),
        cmd::RESET_COMMAND.as_bytes().to_vec(),
        b"--force".to_vec(),
    ])));

    let _restore_in = mock_stdin("yes\n");

    let (_buf, stdout) = pipe_stdout();

    cmd::reset_all();

    drop(stdout);

    match std::fs::metadata(dir.path()) {
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        other => panic!("expected directory to be removed, got: {other:?}"),
    }
}

#[test]
fn test_delete_user_default_admin_is_kept() {
    let _dir = init_cmd_store(auth(true, "user"));

    create_basic_user(security::DEFAULT_ADMIN_USERNAME, "x", Role::admin());

    let _restore_in = mock_stdin("admin\n");

    let (buf, _restore) = capture_cmd_output();

    cmd::delete_user();

    let clean = strip_ansi(&buf.bytes());
    if !contains(&clean, "was not deleted") || contains(&clean, "deleted successfully") {
        panic!("expected the deletion to be refused, got: {}", text(&clean));
    }

    if let Err(err) =
        security::get_basic_user_by_username(security::DEFAULT_ADMIN_USERNAME.as_bytes())
    {
        panic!("the default admin must still exist: {err}");
    }
}

#[test]
fn test_print_help_lists_commands_in_order() {
    let (buf, _restore) = capture_cmd_output();

    cmd::print_help();

    let out = text(&strip_ansi(&buf.bytes()));
    let index = |name: &str| out.find(name).map_or(-1, |i| i as i64);
    if index(cmd::BACKUP_COMMAND) > index(cmd::RESTORE_COMMAND)
        || index(cmd::CHANGE_PASSWORD_COMMAND) > index(cmd::SERVER_COMMAND)
    {
        panic!("expected the commands in alphabetical order, got: {out}");
    }
}

#[test]
fn test_run_backup_writes_an_archive_in_the_output_folder() {
    let _dir = init_cmd_store(Config::default());
    write_entity("book", r#"{"id":"b1"}"#);
    let output_dir = TempDir::new();
    let output = path_arg(output_dir.path());

    for args in [
        vec!["--output".to_string(), output.clone()],
        vec![format!("--output={output}")],
        vec!["-o".to_string(), output.clone()],
    ] {
        let path: PathBuf = match cmd::run_backup(&args) {
            Ok(path) => path,
            Err(err) => panic!("backup {args:?} failed: {err}"),
        };

        let base = filepath::base(&path).to_string_lossy().into_owned();
        if filepath::dir(&path) != output_dir.path()
            || !base.starts_with("elysiandb-backup-")
            || !path.to_string_lossy().ends_with(".tar.gz")
        {
            panic!("unexpected backup path {}", path.display());
        }

        let _ = std::fs::remove_file(&path);
    }
}

#[test]
fn test_run_backup_rejects_invalid_arguments() {
    let _dir = init_cmd_store(Config::default());
    let file_dir = TempDir::new();
    let file = file_dir.path().join("file");
    let _ = std::fs::write(&file, b"x");
    let missing_dir = TempDir::new();

    for args in [
        vec!["--output".to_string()],
        vec!["--unknown".to_string()],
        vec![
            "--output".to_string(),
            path_arg(&missing_dir.path().join("missing")),
        ],
        vec!["--output".to_string(), path_arg(&file)],
    ] {
        if cmd::run_backup(&args).is_ok() {
            panic!("expected backup {args:?} to fail");
        }
    }
}

#[test]
fn test_run_restore_asks_for_confirmation() {
    let _first = init_cmd_store(Config::default());
    write_entity("book", r#"{"id":"b1"}"#);
    let output = TempDir::new();
    let path = match cmd::run_backup(&["--output".to_string(), path_arg(output.path())]) {
        Ok(path) => path,
        Err(err) => panic!("backup failed: {err}"),
    };
    let path = path_arg(&path);

    let _second = init_cmd_store(Config::default());
    write_entity("keep", r#"{"id":"k1"}"#);

    let (_buf, _restore_output) = capture_cmd_output();

    match cmd::run_restore(
        std::slice::from_ref(&path),
        &mut Cursor::new(b"no\n".to_vec()),
    ) {
        Err(err) if errors::is(err.as_ref(), &cmd::ERR_RESTORE_ABORTED) => {}
        other => panic!("expected the restore to be aborted, got {:?}", other.err()),
    }

    if read_entity("keep", "k1").is_none() {
        panic!("an aborted restore must leave the data untouched");
    }

    match cmd::run_restore(&[path], &mut Cursor::new(b"yes\n".to_vec())) {
        Ok(report) if report.entities.get(b"book".as_slice()) == Some(&1) => {}
        Ok(report) => panic!("expected the restore to succeed, got {report:?}"),
        Err(err) => panic!("expected the restore to succeed, got {err}"),
    }

    if read_entity("keep", "k1").is_some() || read_entity("book", "b1").is_none() {
        panic!("expected the data of the archive only");
    }
}

#[test]
fn test_run_restore_with_force_does_not_ask() {
    let _first = init_cmd_store(Config::default());
    write_entity("book", r#"{"id":"b1"}"#);
    let output = TempDir::new();
    let path = cmd::run_backup(&["--output".to_string(), path_arg(output.path())])
        .map(|p| path_arg(&p))
        .unwrap_or_default();

    let _second = init_cmd_store(Config::default());
    if let Err(err) = cmd::run_restore(&["--force".to_string(), path], &mut Cursor::new(Vec::new()))
    {
        panic!("expected the restore to succeed, got {err}");
    }

    if read_entity("book", "b1").is_none() {
        panic!("expected the document of the archive");
    }
}

#[test]
fn test_run_restore_refuses_invalid_input() {
    let _dir = init_cmd_store(Config::default());
    let invalid_dir = TempDir::new();
    let invalid = path_arg(&invalid_dir.path().join("invalid.tar.gz"));
    let _ = std::fs::write(&invalid, b"not an archive");
    let missing_dir = TempDir::new();
    let missing = path_arg(&missing_dir.path().join("missing"));

    for args in [
        vec![],
        vec!["--force".to_string()],
        vec!["a".to_string(), "b".to_string()],
        vec!["--unknown".to_string(), invalid.clone()],
        vec![missing.clone()],
        vec![invalid.clone(), "--force".to_string()],
    ] {
        match cmd::run_restore(&args, &mut Cursor::new(b"yes\n".to_vec())) {
            Err(err) if !errors::is(err.as_ref(), &cmd::ERR_RESTORE_ABORTED) => {}
            other => panic!("expected restore {args:?} to fail, got {:?}", other.err()),
        }
    }
}

#[test]
fn test_backup_and_restore_handlers_exit_on_error() {
    let _dir = init_cmd_store(Config::default());
    cmd::set_flag_args(Vec::new());

    let (buf, _restore_output) = capture_cmd_output();

    let code = Arc::new(AtomicI32::new(0));
    let seen = code.clone();
    let _exit = ExitGuard(cmd::set_exit(Some(Box::new(move |c| {
        seen.store(c, Ordering::SeqCst);
    }))));

    cmd::restore();
    let got = code.load(Ordering::SeqCst);
    if got != 1 || !text(&strip_ansi(&buf.bytes())).contains("Restore failed") {
        panic!(
            "expected the restore handler to exit with 1, got {got}: {}",
            text(&buf.bytes())
        );
    }
}
