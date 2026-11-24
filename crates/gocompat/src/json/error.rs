use super::quote::quote_text;
use super::scanner::SyntaxError;
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expected {
    Integer,
    Number,
    String,
    Boolean,
    Object,
    Array,
    ArrayOfStrings,
    ObjectOfStrings,
}

impl Expected {
    pub fn text(self) -> &'static str {
        match self {
            Expected::Integer => "an integer",
            Expected::Number => "a number",
            Expected::String => "a string",
            Expected::Boolean => "a boolean",
            Expected::Object => "an object",
            Expected::Array => "an array",
            Expected::ArrayOfStrings => "an array of strings",
            Expected::ObjectOfStrings => "an object of strings",
        }
    }

    pub const fn array_of(element: Expected) -> Expected {
        match element {
            Expected::String => Expected::ArrayOfStrings,
            _ => Expected::Array,
        }
    }

    pub const fn object_of(element: Expected) -> Expected {
        match element {
            Expected::String => Expected::ObjectOfStrings,
            _ => Expected::Object,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    String,
    Number,
    Boolean,
    Null,
    Object,
    Array,
}

impl Kind {
    pub fn text(self) -> &'static str {
        match self {
            Kind::String => "a string",
            Kind::Number => "a number",
            Kind::Boolean => "a boolean",
            Kind::Null => "null",
            Kind::Object => "an object",
            Kind::Array => "an array",
        }
    }

    pub fn of(first: u8) -> Kind {
        match first {
            b'{' => Kind::Object,
            b'[' => Kind::Array,
            b'"' => Kind::String,
            b't' | b'f' => Kind::Boolean,
            b'n' => Kind::Null,
            _ => Kind::Number,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Subject {
    Body,
    Value,
    File,
}

impl Subject {
    fn text(self) -> &'static str {
        match self {
            Subject::Body => "the body",
            Subject::Value => "the value",
            Subject::File => "the file",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Path(pub Vec<PathSegment>);

impl Path {
    pub fn is_root(&self) -> bool {
        self.0.is_empty()
    }

    pub fn segments(&self) -> &[PathSegment] {
        &self.0
    }
}

impl fmt::Display for Path {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, seg) in self.0.iter().enumerate() {
            match seg {
                PathSegment::Key(k) => {
                    if i > 0 {
                        f.write_str(".")?;
                    }
                    f.write_str(k)?;
                }
                PathSegment::Index(n) => write!(f, "[{n}]")?,
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub enum ErrorKind {
    Syntax(SyntaxError),
    Type {
        path: Path,
        expected: Expected,
        got: Kind,
    },
    Range {
        path: Path,
        literal: String,
    },
    Base64 {
        path: Path,
        byte: usize,
    },
    UnknownField {
        key: String,
    },
    Quoted {
        path: Path,
        expected: Expected,
    },
    Eof,
    UnexpectedEof,
    Io(Arc<std::io::Error>),
    UnsupportedNumber(f64),
    InvalidRaw(SyntaxError),
    Custom(String),
}

#[derive(Clone, Debug)]
pub struct Error {
    kind: ErrorKind,
    offset: i64,
}

impl Error {
    pub fn new(kind: ErrorKind, offset: i64) -> Error {
        Error { kind, offset }
    }

    pub fn custom(message: impl Into<String>) -> Error {
        Error::new(ErrorKind::Custom(message.into()), 0)
    }

    pub fn kind(&self) -> &ErrorKind {
        &self.kind
    }

    pub fn offset(&self) -> i64 {
        self.offset
    }

    pub fn is_eof(&self) -> bool {
        matches!(self.kind, ErrorKind::Eof)
    }

    pub fn syntax(&self) -> Option<&SyntaxError> {
        match &self.kind {
            ErrorKind::Syntax(e) => Some(e),
            _ => None,
        }
    }

    pub fn message(&self, subject: Subject) -> String {
        match &self.kind {
            ErrorKind::Syntax(e) => e.to_string(),
            ErrorKind::Io(e) => e.to_string(),
            ErrorKind::Custom(m) => m.clone(),
            ErrorKind::UnsupportedNumber(_) | ErrorKind::InvalidRaw(_) => {
                format!("cannot encode JSON: {}", self.reason(subject))
            }
            _ => format!("invalid JSON: {}", self.reason(subject)),
        }
    }

    pub fn reason(&self, subject: Subject) -> String {
        match &self.kind {
            ErrorKind::Syntax(e) => e.without_prefix(),
            ErrorKind::Type {
                path,
                expected,
                got,
            } => {
                if path.is_root() {
                    format!(
                        "{} must be {}, got {}",
                        subject.text(),
                        expected.text(),
                        got.text()
                    )
                } else {
                    format!("\"{path}\" must be {}, got {}", expected.text(), got.text())
                }
            }
            ErrorKind::Range { path, literal } => {
                if path.is_root() {
                    format!("{literal} is out of range")
                } else {
                    format!("\"{path}\": {literal} is out of range")
                }
            }
            ErrorKind::Base64 { path, byte } => {
                if path.is_root() {
                    format!("invalid base64 at byte {byte}")
                } else {
                    format!("\"{path}\": invalid base64 at byte {byte}")
                }
            }
            ErrorKind::UnknownField { key } => {
                format!("unknown field {}", quote_text(key.as_bytes()))
            }
            ErrorKind::Quoted { path, expected } => {
                if path.is_root() {
                    format!(
                        "{} must be a string holding {}",
                        subject.text(),
                        expected.text()
                    )
                } else {
                    format!("\"{path}\" must be a string holding {}", expected.text())
                }
            }
            ErrorKind::Eof | ErrorKind::UnexpectedEof => "unexpected end of input".to_string(),
            ErrorKind::Io(e) => e.to_string(),
            ErrorKind::UnsupportedNumber(f) => {
                let name = if f.is_nan() {
                    "NaN"
                } else if *f > 0.0 {
                    "Infinity"
                } else {
                    "-Infinity"
                };
                format!("{name} is not a valid JSON number")
            }
            ErrorKind::InvalidRaw(_) => "a raw value is not valid JSON".to_string(),
            ErrorKind::Custom(m) => m.clone(),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message(Subject::Value))
    }
}

impl std::error::Error for Error {}

impl From<SyntaxError> for Error {
    fn from(e: SyntaxError) -> Error {
        let offset = e.offset;
        Error::new(ErrorKind::Syntax(e), offset)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::new(ErrorKind::Io(Arc::new(e)), 0)
    }
}
