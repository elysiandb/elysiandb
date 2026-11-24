mod r#match;
mod regexp;
mod replace;
mod runner;
pub mod syntax;

pub use self::r#match::{Capture, Group, Match};
pub use self::regexp::{
    COMPILED, DEBUG, DEFAULT_UNMARSHAL_OPTIONS, ECMASCRIPT, EXPLICIT_CAPTURE, Error, IGNORE_CASE,
    IGNORE_PATTERN_WHITESPACE, MULTILINE, NONE, RE2, Regexp, SINGLELINE, UNICODE, compile, escape,
    must_compile, unescape,
};
pub use self::syntax::{RIGHT_TO_LEFT, RegexOptions};
