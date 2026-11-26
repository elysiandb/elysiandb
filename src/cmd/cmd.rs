use std::collections::BTreeMap;
use std::fs::File;
use std::io::{self, ErrorKind, Read, Write};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;

use crate::globals::{BOLD, RESET};
use crate::gocompat::fmt::{fscanln_int, fscanln_word};
use crate::sync::Mutex;

use super::backup::backup;
use super::change_password::change_password;
use super::create_user::create_user;
use super::delete_user::delete_user;
use super::export_legacy::export_legacy;
use super::reset::reset_all;
use super::restore::restore;

pub const CREATE_USER_COMMAND: &str = "create-user";
pub const DELETE_USER_COMMAND: &str = "delete-user";
pub const SERVER_COMMAND: &str = "server";
pub const HELP_COMMAND: &str = "help";
pub const CHANGE_PASSWORD_COMMAND: &str = "change-password";
pub const RESET_COMMAND: &str = "reset";
pub const BACKUP_COMMAND: &str = "backup";
pub const RESTORE_COMMAND: &str = "restore";
pub const EXPORT_LEGACY_COMMAND: &str = "export-legacy";

pub type Handler = fn();
pub type PrintfHook = Box<dyn FnMut(&[u8]) + Send>;
pub type ReadPasswordHook = Box<dyn FnMut(i32) -> io::Result<Vec<u8>> + Send>;
pub type ExitHook = Box<dyn FnMut(i32) + Send>;
pub type Input = Box<dyn Read + Send>;
pub type Output = Box<dyn Write + Send>;

pub const END_OF_INPUT: &str = "end of input";

static PRINTF: Mutex<Option<PrintfHook>> = Mutex::new(None);
static READ_PASSWORD: Mutex<Option<ReadPasswordHook>> = Mutex::new(None);
static EXIT: Mutex<Option<ExitHook>> = Mutex::new(None);
static STDIN: Mutex<Option<Input>> = Mutex::new(None);
static STDOUT: Mutex<Option<Output>> = Mutex::new(None);
static OS_ARGS: Mutex<Option<Vec<Vec<u8>>>> = Mutex::new(None);
static FLAG_ARGS: Mutex<Vec<Vec<u8>>> = Mutex::new(Vec::new());

pub fn get_available_commands() -> BTreeMap<&'static str, &'static str> {
    BTreeMap::from([
        (SERVER_COMMAND, "Start ElysianDB server"),
        (
            CREATE_USER_COMMAND,
            "Create a new user (needs security.authentication.mode = basic or user)",
        ),
        (
            DELETE_USER_COMMAND,
            "Delete an existing user (needs security.authentication.mode = basic or user)",
        ),
        (
            CHANGE_PASSWORD_COMMAND,
            "Change password for an existing user (needs security.authentication.mode = basic or user)",
        ),
        (
            RESET_COMMAND,
            "Reset the database by deleting all stored data and resets users (asks for yes, requires --force)",
        ),
        (
            BACKUP_COMMAND,
            "Write a full backup archive: elysiandb backup [--output <folder>] [--url <address of a live instance> [--user <name>] [--token <token>]]",
        ),
        (
            RESTORE_COMMAND,
            "Replace all the data with a backup archive: elysiandb restore <file> [--force]",
        ),
        (
            EXPORT_LEGACY_COMMAND,
            "Write the data as a 0.2.4 data folder: elysiandb export-legacy --output <folder>",
        ),
        (HELP_COMMAND, "List available commands"),
    ])
}

pub fn get_handlers() -> BTreeMap<&'static str, Handler> {
    BTreeMap::from([
        (CREATE_USER_COMMAND, create_user as Handler),
        (DELETE_USER_COMMAND, delete_user as Handler),
        (CHANGE_PASSWORD_COMMAND, change_password as Handler),
        (RESET_COMMAND, reset_all as Handler),
        (BACKUP_COMMAND, backup as Handler),
        (RESTORE_COMMAND, restore as Handler),
        (EXPORT_LEGACY_COMMAND, export_legacy as Handler),
        (HELP_COMMAND, print_help as Handler),
    ])
}

pub fn print_help() {
    for (name, help) in get_available_commands() {
        printf(format!("  {BOLD}{name}{RESET}  {help}\n"));
    }
}

pub fn set_printf(hook: Option<PrintfHook>) -> Option<PrintfHook> {
    std::mem::replace(&mut *PRINTF.lock(), hook)
}

pub fn set_read_password(hook: Option<ReadPasswordHook>) -> Option<ReadPasswordHook> {
    std::mem::replace(&mut *READ_PASSWORD.lock(), hook)
}

pub fn set_exit(hook: Option<ExitHook>) -> Option<ExitHook> {
    std::mem::replace(&mut *EXIT.lock(), hook)
}

pub fn set_stdin(input: Option<Input>) -> Option<Input> {
    std::mem::replace(&mut *STDIN.lock(), input)
}

pub fn set_stdout(output: Option<Output>) -> Option<Output> {
    std::mem::replace(&mut *STDOUT.lock(), output)
}

pub fn set_os_args(args: Option<Vec<Vec<u8>>>) -> Option<Vec<Vec<u8>>> {
    std::mem::replace(&mut *OS_ARGS.lock(), args)
}

pub fn set_flag_args(args: Vec<Vec<u8>>) {
    *FLAG_ARGS.lock() = args;
}

pub fn flag_args() -> Vec<Vec<u8>> {
    FLAG_ARGS.lock().clone()
}

pub(crate) fn command_args() -> Vec<Vec<u8>> {
    let args = FLAG_ARGS.lock();
    if args.is_empty() {
        return Vec::new();
    }

    args[1..].to_vec()
}

pub(crate) fn os_args() -> Vec<Vec<u8>> {
    if let Some(args) = OS_ARGS.lock().as_ref() {
        return args.clone();
    }

    std::env::args_os().map(|a| a.as_bytes().to_vec()).collect()
}

pub fn printf(text: impl AsRef<[u8]>) {
    let text = text.as_ref();
    let mut hook = PRINTF.lock();
    match hook.as_mut() {
        Some(f) => f(text),
        None => {
            drop(hook);
            print(text);
        }
    }
}

pub fn print(text: impl AsRef<[u8]>) {
    let text = text.as_ref();
    let mut out = STDOUT.lock();
    match out.as_mut() {
        Some(w) => {
            let _ = w.write_all(text);
        }
        None => write_fd(libc::STDOUT_FILENO, text),
    }
}

pub fn println(text: impl AsRef<[u8]>) {
    let mut line = text.as_ref().to_vec();
    line.push(b'\n');
    print(line);
}

pub(crate) fn write_fd(fd: libc::c_int, text: &[u8]) {
    let mut out = ManuallyDrop::new(unsafe { File::from_raw_fd(fd) });
    if let Err(e) = out.write_all(text)
        && e.kind() == ErrorKind::BrokenPipe
    {
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            libc::raise(libc::SIGPIPE);
        }
    }
}

pub fn exit(code: i32) {
    let mut hook = EXIT.lock();
    match hook.as_mut() {
        Some(f) => f(code),
        None => std::process::exit(code),
    }
}

pub(crate) fn with_stdin<T>(f: impl FnOnce(&mut dyn Read) -> T) -> T {
    let mut input = STDIN.lock();
    match input.as_mut() {
        Some(r) => f(r.as_mut()),
        None => {
            let mut stdin = ManuallyDrop::new(unsafe { File::from_raw_fd(libc::STDIN_FILENO) });
            f(&mut *stdin)
        }
    }
}

pub(crate) fn scanln_word() -> Vec<u8> {
    with_stdin(|r| fscanln_word(r).0)
}

pub(crate) fn scanln_int() -> i64 {
    with_stdin(|r| fscanln_int(r).0)
}

pub(crate) fn read_line() -> Vec<u8> {
    with_stdin(|r| read_line_from(r))
}

pub(crate) fn read_line_from(r: &mut dyn Read) -> Vec<u8> {
    let mut reader = io::BufReader::with_capacity(4096, r);
    let mut line = Vec::new();
    let _ = io::BufRead::read_until(&mut reader, b'\n', &mut line);
    line
}

pub fn read_password(fd: i32) -> io::Result<Vec<u8>> {
    let mut hook = READ_PASSWORD.lock();
    match hook.as_mut() {
        Some(f) => f(fd),
        None => {
            drop(hook);
            terminal_password(fd)
        }
    }
}

pub fn terminal_password(fd: i32) -> io::Result<Vec<u8>> {
    let mut saved: libc::termios = unsafe { std::mem::zeroed() };
    if unsafe { libc::tcgetattr(fd, &mut saved) } != 0 {
        return Err(io::Error::last_os_error());
    }

    let mut state = saved;
    state.c_lflag &= !libc::ECHO;
    state.c_lflag |= libc::ICANON | libc::ISIG;
    state.c_iflag |= libc::ICRNL;
    if unsafe { libc::tcsetattr(fd, libc::TCSANOW, &state) } != 0 {
        return Err(io::Error::last_os_error());
    }

    let result = read_password_line(fd);
    unsafe {
        libc::tcsetattr(fd, libc::TCSANOW, &saved);
    }

    result
}

fn read_password_line(fd: i32) -> io::Result<Vec<u8>> {
    let mut ret = Vec::new();
    loop {
        let mut b = 0u8;
        let n = unsafe { libc::read(fd, (&raw mut b).cast::<libc::c_void>(), 1) };
        if n > 0 {
            match b {
                b'\x08' => {
                    ret.pop();
                }
                b'\n' => return Ok(ret),
                b'\r' => {}
                _ => ret.push(b),
            }
            continue;
        }

        if n == 0 {
            if !ret.is_empty() {
                return Ok(ret);
            }
            return Err(io::Error::new(ErrorKind::UnexpectedEof, END_OF_INPUT));
        }

        return Err(io::Error::last_os_error());
    }
}
