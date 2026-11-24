use crate::deps::big;
use crate::string_ascii::{ParseErr, string_to_int};

pub(crate) fn string_to_big_int(s: &[u8]) -> Option<big::Int> {
    match string_to_int(s) {
        Ok(n) => Some(big::Int::new_int(n)),
        Err(ParseErr::Range) => Some(big::Int::set_string(s, 0).unwrap_or_default()),
        Err(ParseErr::Syntax) if s == b"-0" => Some(big::Int::new()),
        Err(ParseErr::Syntax) => None,
    }
}
