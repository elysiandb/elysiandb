use super::buffer::resize_buffer;
use super::handler::{handle, handler_enabled};
use super::level::cancel_override;
use super::value::Attr;
use crate::gocompat::errors::Error;
use crate::gocompat::json::quote_text;
use crate::gocompat::strings::{to_lower, trim_space};
use crate::sync::Mutex;
use std::io::Write;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::time::Instant;

pub const FORMAT_TEXT: &str = "text";
pub const FORMAT_JSON: &str = "json";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Level(pub i64);

impl Level {
    pub const DEBUG: Level = Level(-4);
    pub const INFO: Level = Level(0);
    pub const WARN: Level = Level(4);
    pub const ERROR: Level = Level(8);
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Options {
    pub level: String,
    pub format: String,
    pub buffer_lines: i64,
}

pub(super) struct State {
    pub(super) out: Option<Box<dyn Write + Send>>,
    pub(super) format: &'static str,
    pub(super) color: bool,
}

pub(super) static STATE: Mutex<State> = Mutex::new(State {
    out: None,
    format: FORMAT_TEXT,
    color: false,
});

static LEVEL: AtomicI64 = AtomicI64::new(0);
pub(super) static CONFIGURED_LEVEL: AtomicI64 = AtomicI64::new(0);
pub(super) static WARNING_COUNT: AtomicU64 = AtomicU64::new(0);
pub(super) static ERROR_COUNT: AtomicU64 = AtomicU64::new(0);
static EXIT_HOOK: Mutex<Option<fn(i32) -> !>> = Mutex::new(None);

pub(super) fn current_level_value() -> Level {
    Level(LEVEL.load(Ordering::SeqCst))
}

pub(super) fn set_level_value(level: Level) {
    LEVEL.store(level.0, Ordering::SeqCst);
}

pub fn configure(opts: &Options) -> Result<(), Error> {
    let lvl = parse_level(&opts.level)?;

    let format = to_lower(trim_space(opts.format.as_bytes()));
    let format = match format.as_slice() {
        b"" | b"text" => FORMAT_TEXT,
        b"json" => FORMAT_JSON,
        _ => {
            return Err(Error::new(format!(
                "unknown log format {}: use text or json",
                quote_text(opts.format.as_bytes())
            )));
        }
    };

    {
        let mut state = STATE.lock();
        state.format = format;
        state.color = format == FORMAT_TEXT && stdout_is_terminal() && no_color_unset();
    }

    CONFIGURED_LEVEL.store(lvl.0, Ordering::SeqCst);
    cancel_override();
    set_level_value(lvl);
    resize_buffer(opts.buffer_lines);

    Ok(())
}

fn no_color_unset() -> bool {
    std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty())
}

pub fn set_output(w: Option<Box<dyn Write + Send>>, color: bool) {
    let mut state = STATE.lock();
    state.out = w;
    state.color = color;
}

pub fn format() -> &'static str {
    STATE.lock().format
}

pub fn stdout_is_terminal() -> bool {
    unsafe { libc::isatty(libc::STDOUT_FILENO) == 1 }
}

pub fn parse_level(value: impl AsRef<[u8]>) -> Result<Level, Error> {
    let value = value.as_ref();
    match to_lower(trim_space(value)).as_slice() {
        b"" | b"info" => Ok(Level::INFO),
        b"debug" => Ok(Level::DEBUG),
        b"warn" | b"warning" => Ok(Level::WARN),
        b"error" => Ok(Level::ERROR),
        _ => Err(Error::new(format!(
            "unknown log level {}: use debug, info, warn or error",
            quote_text(value)
        ))),
    }
}

pub fn level_name(level: Level) -> &'static str {
    if level < Level::INFO {
        "debug"
    } else if level < Level::WARN {
        "info"
    } else if level < Level::ERROR {
        "warn"
    } else {
        "error"
    }
}

pub fn enabled(level: Level) -> bool {
    level >= current_level_value()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Logger {
    component: &'static str,
}

pub const fn for_component(component: &'static str) -> Logger {
    Logger { component }
}

impl Logger {
    pub fn component(&self) -> &'static str {
        self.component
    }

    pub fn with_group(&self, _name: &str) -> Logger {
        *self
    }

    pub fn debug(&self, msg: &str, attrs: &[Attr<'_>]) {
        log(Level::DEBUG, msg, Some(self.component), attrs);
    }

    pub fn info(&self, msg: &str, attrs: &[Attr<'_>]) {
        log(Level::INFO, msg, Some(self.component), attrs);
    }

    pub fn warn(&self, msg: &str, attrs: &[Attr<'_>]) {
        log(Level::WARN, msg, Some(self.component), attrs);
    }

    pub fn error(&self, msg: &str, attrs: &[Attr<'_>]) {
        log(Level::ERROR, msg, Some(self.component), attrs);
    }
}

fn log(level: Level, msg: &str, component: Option<&str>, attrs: &[Attr<'_>]) {
    if !handler_enabled(level) {
        return;
    }
    handle(level, msg, component, attrs);
}

pub fn debug(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::DEBUG, msg, None, attrs);
}

pub fn info(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::INFO, msg, None, attrs);
}

pub fn warn(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::WARN, msg, None, attrs);
}

pub fn error(msg: &str, attrs: &[Attr<'_>]) {
    log(Level::ERROR, msg, None, attrs);
}

pub fn fatal(msg: &str, attrs: &[Attr<'_>]) -> ! {
    log(Level::ERROR, msg, None, attrs);
    exit(1)
}

pub fn set_exit_hook(hook: Option<fn(i32) -> !>) {
    *EXIT_HOOK.lock() = hook;
}

fn exit(code: i32) -> ! {
    let hook = *EXIT_HOOK.lock();
    match hook {
        Some(h) => h(code),
        None => std::process::exit(code),
    }
}

pub fn counts() -> (u64, u64) {
    (
        WARNING_COUNT.load(Ordering::Relaxed),
        ERROR_COUNT.load(Ordering::Relaxed),
    )
}

pub fn since(start: Instant) -> f64 {
    let micros = i64::try_from(start.elapsed().as_micros()).unwrap_or(i64::MAX);
    micros as f64 / 1000.0
}
