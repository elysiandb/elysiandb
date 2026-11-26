mod error;
mod expression;
mod lexer;
#[allow(clippy::module_inception)]
mod parser;
mod regexp;
mod scope;
mod statement;

pub use error::{
    ERR_UNEXPECTED_END_OF_INPUT, ERR_UNEXPECTED_ESCAPE, ERR_UNEXPECTED_TOKEN, Error, ErrorList,
    Place,
};
pub use lexer::{
    IllegalNumericLiteral, ParserState, is_identifier, parse_number_literal, parse_string_literal,
};
pub use parser::{
    IGNORE_REG_EXP_ERRORS, LoaderError, Mode, Options, Parser, ParserOption, SourceMapLoader,
    parse_file, parse_function, with_disable_source_maps, with_source_map_loader,
};
pub use regexp::{
    RE2_DOT, RegexpErrorIncompatible, RegexpParseError, RegexpSyntaxError, TransformError,
    WHITESPACE_CHARS, transform_reg_exp,
};
pub use scope::Scope;
pub use statement::extract_source_map_line;
