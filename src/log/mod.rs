mod buffer;
mod handler;
mod level;
#[allow(clippy::module_inception)]
mod log;
mod throttle;
mod value;

pub use buffer::{DEFAULT_QUERY_LIMIT, Entry, MAX_QUERY_LIMIT, Page, Query, buffer_enabled, logs};
pub use level::{LevelInfo, configured_level, current_level, set_level_for};
pub use log::{
    FORMAT_JSON, FORMAT_TEXT, Level, Logger, Options, configure, counts, debug, enabled, error,
    fatal, for_component, format, info, level_name, parse_level, set_exit_hook, set_output, since,
    stdout_is_terminal, warn,
};
pub use throttle::{Throttle, with_suppressed};
pub use value::{Attr, AttrValue, JsonText, ToValue, Value, display, group, json};
