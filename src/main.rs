use std::ffi::OsStr;
use std::os::unix::ffi::{OsStrExt, OsStringExt};
use std::path::Path;
use std::sync::Arc;

use elysiandb::boot;
use elysiandb::cmd;
use elysiandb::configuration;
use elysiandb::globals::{self, BLUE, BOLD, GOLD, GRAY, RESET};
use elysiandb::gocompat::flag::FlagSet;
use elysiandb::kv;
use elysiandb::log::{self, Options, Value};
use elysiandb::metrics;
use elysiandb::security;
use elysiandb::sync;
use elysiandb::transport::http::observe;

const USAGE: &str = "Usage: elysiandb [-config <path>] [<command> [<arguments>]]

Options:
  -config <path>  Path to configuration file (default \"elysian.yaml\")

Commands: backup, change-password, create-user, delete-user, export-legacy, help, reset, restore, server (default).
Run \"elysiandb help\" to describe them.
";

fn pad(text: &str, width: usize) -> String {
    let mut out = text.to_string();
    for _ in text.chars().count()..width {
        out.push(' ');
    }
    out
}

fn banner() {
    let line = "═".repeat(67);
    let mut out = format!("\n{BLUE}");
    out.push_str(&format!(" ╔{line}╗\n"));
    out.push_str(&format!(
        " ║ {GOLD}{BOLD}{}{RESET}{BLUE}   ║\n",
        pad("ElysianDB", 63)
    ));
    out.push_str(&format!(
        " ║ {GRAY}{}{RESET}{BLUE}   ║\n",
        pad("A modern, lightweight KV datastore", 63)
    ));
    out.push_str(&format!(
        " ║ {GOLD}{}{RESET}{BLUE}   ║\n",
        pad("→ Instant REST API, out of the box", 63)
    ));
    out.push_str(&format!(" ╚{line}╝{RESET}\n"));
    cmd::print(out);
}

fn raise_open_file_limit() {
    let mut lim = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    if unsafe { libc::getrlimit(libc::RLIMIT_NOFILE, &mut lim) } != 0
        || lim.rlim_cur == lim.rlim_max
    {
        return;
    }

    let mut raised = lim;
    raised.rlim_cur = raised.rlim_max;
    adjust_file_limit(&mut raised);
    unsafe {
        libc::setrlimit(libc::RLIMIT_NOFILE, &raised);
    }
}

#[cfg(target_os = "macos")]
fn adjust_file_limit(lim: &mut libc::rlimit) {
    let mut value: u32 = 0;
    let mut len: libc::size_t = std::mem::size_of::<u32>();
    let name = c"kern.maxfilesperproc";
    let rc = unsafe {
        libc::sysctlbyname(
            name.as_ptr(),
            (&raw mut value).cast::<libc::c_void>(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return;
    }
    if lim.rlim_cur > libc::rlim_t::from(value) {
        lim.rlim_cur = libc::rlim_t::from(value);
    }
}

#[cfg(not(target_os = "macos"))]
fn adjust_file_limit(_: &mut libc::rlimit) {}

fn main() {
    metrics::started_at();
    sync::install_base_panic_hook();
    observe::install_panic_hook();
    raise_open_file_limit();
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_IGN);
    }

    sync::run_fatal(run);
}

fn run() {
    let mut flags = FlagSet::new(USAGE);
    flags.string("config", b"elysian.yaml");
    let args: Vec<Vec<u8>> = std::env::args_os().skip(1).map(|a| a.into_vec()).collect();
    if let Err(err) = flags.parse(args) {
        let mut out = Vec::new();
        let _ = flags.write_error(&err, &mut out);
        cmd::write_stderr(out);
        std::process::exit(err.exit_code());
    }

    let config_filename = flags.get_string("config").to_vec();
    let path = Path::new(OsStr::from_bytes(&config_filename));

    let cfg = match configuration::load_config(path) {
        Ok(cfg) => cfg,
        Err(err) => log::fatal(
            "cannot load the configuration",
            kv![
                "component" => "boot",
                "path" => Value::Str(&config_filename),
                "err" => log::display(&err),
            ],
        ),
    };

    if let Err(err) = log::configure(&Options {
        level: cfg.log.level.clone(),
        format: cfg.log.format.clone(),
        buffer_lines: cfg.log.buffer_lines,
    }) {
        log::fatal(
            "invalid log configuration",
            kv![
                "component" => "boot",
                "path" => Value::Str(&config_filename),
                "err" => err,
            ],
        );
    }

    if log::format() == log::FORMAT_TEXT && log::stdout_is_terminal() {
        banner();
    }

    globals::set_config(Some(Arc::new(cfg)));
    boot::log_configuration(&config_filename);

    let args = flags.args().to_vec();
    cmd::set_flag_args(args.clone());

    if args.is_empty() || args[0] == cmd::SERVER_COMMAND.as_bytes() {
        boot::apply_memory_limit();
        boot::init_db();
        init_admin();
        cmd::start_server();
        return;
    }

    let handlers = cmd::get_handlers();
    let handler = std::str::from_utf8(&args[0])
        .ok()
        .and_then(|name| handlers.get(name))
        .copied();
    let Some(handler) = handler else {
        let mut out = GOLD.as_bytes().to_vec();
        out.extend_from_slice(b"Unknown command: ");
        out.extend_from_slice(&args[0]);
        out.extend_from_slice(RESET.as_bytes());
        out.push(b'\n');
        cmd::print(out);
        print_list_of_commands();
        return;
    };

    if args[0] == cmd::HELP_COMMAND.as_bytes() {
        handler();
        return;
    }

    if cmd::runs_without_storage(&args) {
        handler();
        return;
    }

    if args[0] == cmd::RESTORE_COMMAND.as_bytes() {
        boot::init_bulk_tool();
    } else {
        boot::init_tool();
    }
    init_admin();
    handler();
    cmd::close_storage();
}

fn init_admin() {
    if let Err(err) = security::init_admin_user_if_not_exists() {
        log::error(
            "default admin creation failed",
            kv!["component" => "security", "err" => log::display(&err)],
        );
    }
    security::protect_sessions_file();
}

fn print_list_of_commands() {
    cmd::print_help();
}
