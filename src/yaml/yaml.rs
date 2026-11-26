use super::decode::{Decoder, Target};
use super::node::{Document, parse};
use super::yamlh::Mark;
use crate::gocompat::json::quote_text;
use std::fmt;

pub const PREFIX: &str = "invalid configuration file: ";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxKind {
    Reader,
    Scanner,
    Parser,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub kind: SyntaxKind,
    pub problem: String,
    pub context: &'static str,
    pub problem_mark: Mark,
    pub context_mark: Mark,
    pub problem_offset: usize,
    pub problem_value: i64,
}

impl SyntaxError {
    pub fn line(&self) -> Option<usize> {
        if self.problem_mark.line != 0 {
            if self.kind == SyntaxKind::Scanner {
                return Some(self.problem_mark.line + 1);
            }
            return Some(self.problem_mark.line);
        }
        if self.context_mark.line != 0 {
            return Some(self.context_mark.line);
        }
        None
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Expected {
    Integer,
    Boolean,
    String,
    Strings,
    Array,
    Mapping,
    Any,
}

impl Expected {
    pub fn text(self) -> &'static str {
        match self {
            Expected::Integer => "an integer",
            Expected::Boolean => "a boolean",
            Expected::String => "a string",
            Expected::Strings => "an array of strings",
            Expected::Array => "an array",
            Expected::Mapping => "a mapping",
            Expected::Any => "a value",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Got {
    Scalar(String),
    Sequence,
    Mapping,
}

impl Got {
    pub fn text(&self) -> String {
        match self {
            Got::Scalar(s) => quote_text(s.as_bytes()),
            Got::Sequence => "a sequence".to_string(),
            Got::Mapping => "a mapping".to_string(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathSegment {
    Key(String),
    Index(usize),
}

pub fn dotted(path: &[PathSegment]) -> String {
    let mut out = String::new();
    for segment in path {
        match segment {
            PathSegment::Key(k) => {
                if !out.is_empty() {
                    out.push('.');
                }
                out.push_str(k);
            }
            PathSegment::Index(i) => {
                out.push('[');
                out.push_str(&i.to_string());
                out.push(']');
            }
        }
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IssueKind {
    Mismatch { expected: Expected, got: Got },
    OutOfRange { text: String },
    Key { got: Got },
    Unknown,
    Duplicate,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Issue {
    pub line: usize,
    pub path: Vec<PathSegment>,
    pub kind: IssueKind,
}

impl Issue {
    pub fn dotted_path(&self) -> String {
        dotted(&self.path)
    }

    pub fn detail(&self) -> String {
        let path = self.dotted_path();
        let quoted = quote_text(path.as_bytes());
        let line = self.line;
        match &self.kind {
            IssueKind::Mismatch { expected, got } if path.is_empty() => format!(
                "line {line}: the file must be {}, got {}",
                expected.text(),
                got.text()
            ),
            IssueKind::Mismatch { expected, got } => format!(
                "line {line}: {quoted} must be {}, got {}",
                expected.text(),
                got.text()
            ),
            IssueKind::OutOfRange { text } if path.is_empty() => {
                format!("line {line}: {text} is out of range")
            }
            IssueKind::OutOfRange { text } => {
                format!("line {line}: {quoted}: {text} is out of range")
            }
            IssueKind::Key { got } if path.is_empty() => format!(
                "line {line}: keys of the file must be strings, got {}",
                got.text()
            ),
            IssueKind::Key { got } => format!(
                "line {line}: keys of {quoted} must be strings, got {}",
                got.text()
            ),
            IssueKind::Unknown => format!("line {line}: unknown key {quoted}"),
            IssueKind::Duplicate => format!("line {line}: duplicate key {quoted}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    Syntax(SyntaxError),
    UnknownAnchor(String),
    RecursiveAnchor(String),
    TagMismatch {
        resolved: String,
        tag: String,
        text: String,
    },
    Binary,
    Merge,
    MapKey(Got),
    Aliasing,
    Type(Vec<Issue>),
}

impl Error {
    fn lines(&self) -> Vec<String> {
        let single = match self {
            Error::Syntax(e) => match e.line() {
                Some(line) => format!("line {line}: {}", e.problem),
                None => e.problem.clone(),
            },
            Error::UnknownAnchor(a) => format!("unknown anchor '{a}' referenced"),
            Error::RecursiveAnchor(a) => format!("anchor '{a}' value contains itself"),
            Error::TagMismatch {
                resolved,
                tag,
                text,
            } => format!(
                "cannot decode {resolved} {} as a {tag}",
                quote_text(text.as_bytes())
            ),
            Error::Binary => "!!binary value contains invalid base64 data".to_string(),
            Error::Merge => "map merge requires map or sequence of maps as the value".to_string(),
            Error::MapKey(got) => format!("invalid map key: {}", got.text()),
            Error::Aliasing => "document contains excessive aliasing".to_string(),
            Error::Type(issues) => return issues.iter().map(Issue::detail).collect(),
        };
        vec![single]
    }

    pub fn detail(&self) -> String {
        self.lines().join("\n")
    }

    pub fn message(&self) -> String {
        self.lines()
            .iter()
            .map(|line| format!("{PREFIX}{line}"))
            .collect::<Vec<_>>()
            .join("\n")
    }

    pub fn issues(&self) -> &[Issue] {
        match self {
            Error::Type(issues) => issues,
            _ => &[],
        }
    }

    fn keys_of(&self, kind: &IssueKind) -> Vec<String> {
        self.issues()
            .iter()
            .filter(|i| &i.kind == kind)
            .map(Issue::dotted_path)
            .collect()
    }

    pub fn unknown_keys(&self) -> Vec<String> {
        self.keys_of(&IssueKind::Unknown)
    }

    pub fn duplicate_keys(&self) -> Vec<String> {
        self.keys_of(&IssueKind::Duplicate)
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message())
    }
}

impl std::error::Error for Error {}

const INLINE_DEPTH: usize = 128;

fn decode_bound(doc: &Document) -> usize {
    let aliases = doc.nodes.iter().filter(|n| n.alias.is_some()).count();
    doc.depth().saturating_mul(aliases + 1)
}

fn run<T: Target>(doc: &Document, out: &mut T, strict: bool) -> Result<(), Error> {
    let mut d = Decoder::new(doc, strict);
    d.unmarshal(0, out)?;
    d.finish()
}

fn decode<T: Target + Send>(data: &[u8], out: &mut T, strict: bool) -> Result<(), Error> {
    let Some(doc) = parse(data)? else {
        return Ok(());
    };
    let bound = decode_bound(&doc);
    if bound <= INLINE_DEPTH {
        return run(&doc, out, strict);
    }
    let stack = bound
        .saturating_mul(16 * 1024)
        .saturating_add(1 << 20)
        .min(1 << 30);
    let doc = &doc;
    std::thread::scope(|scope| {
        let handle = std::thread::Builder::new()
            .name("yaml".to_string())
            .stack_size(stack)
            .spawn_scoped(scope, move || run(doc, out, strict));
        match handle {
            Ok(h) => match h.join() {
                Ok(result) => result,
                Err(payload) => std::panic::resume_unwind(payload),
            },
            Err(e) => panic!("cannot start the YAML decoding thread: {e}"),
        }
    })
}

pub fn unmarshal<T: Target + Send>(data: &[u8], out: &mut T) -> Result<(), Error> {
    decode(data, out, false)
}

pub fn unmarshal_strict<T: Target + Send>(data: &[u8], out: &mut T) -> Result<(), Error> {
    decode(data, out, true)
}
