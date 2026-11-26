pub mod base64vlq;
mod consumer;
mod decode;
mod mappings;
pub mod path;
pub mod url;

pub use consumer::{Consumer, parse};
pub use mappings::{Mapping, parse_mappings};

use gocompat::json::{SyntaxError, SyntaxReason, quote_text};
use std::fmt;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Json(SyntaxError),
    Type {
        path: String,
        expected: &'static str,
        got: &'static str,
    },
    Range {
        path: String,
        literal: String,
    },
    Version(i64),
    MappingsEmpty,
    UnexpectedEndOfMappings,
    Url(url::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Json(e) => {
                if e.at_eof || e.reason == SyntaxReason::UnexpectedEnd {
                    f.write_str("unexpected end of input")
                } else {
                    write!(f, "byte {}: {}", e.offset, e.message())
                }
            }
            Error::Type {
                path,
                expected,
                got,
            } => {
                if path.is_empty() {
                    write!(f, "the value must be {expected}, got {got}")
                } else {
                    write!(
                        f,
                        "{} must be {expected}, got {got}",
                        quote_text(path.as_bytes())
                    )
                }
            }
            Error::Range { path, literal } => {
                write!(
                    f,
                    "{}: {literal} is out of range",
                    quote_text(path.as_bytes())
                )
            }
            Error::Version(v) => write!(f, "got version={v}, but only 3rd version is supported"),
            Error::MappingsEmpty => f.write_str("mappings are empty"),
            Error::UnexpectedEndOfMappings => f.write_str("unexpected end of mappings"),
            Error::Url(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for Error {}
