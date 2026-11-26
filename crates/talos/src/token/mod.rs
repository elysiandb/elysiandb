#[allow(clippy::module_inception)]
mod token;
mod token_const;

pub use token::{Token, is_id, is_keyword, is_unreserved_word};
pub use token_const::*;
