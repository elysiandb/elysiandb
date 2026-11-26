use std::fmt;
use std::io::{self, Write};

use crate::json::quote_text;
use crate::strconv::{parse_bool, parse_float, parse_int};
use crate::time::{Duration, parse_duration};

#[derive(Clone, Debug, PartialEq)]
pub enum Value {
    Bool(bool),
    String(Vec<u8>),
    Int(i64),
    Float64(f64),
    Duration(Duration),
}

impl Value {
    pub fn is_bool(&self) -> bool {
        matches!(self, Value::Bool(_))
    }

    fn set(&mut self, s: &[u8]) -> Result<(), String> {
        match self {
            Value::Bool(b) => match parse_bool(s) {
                Ok(v) => {
                    *b = v;
                    Ok(())
                }
                Err(e) => {
                    *b = e.value;
                    Err(e.to_string())
                }
            },
            Value::String(v) => {
                *v = s.to_vec();
                Ok(())
            }
            Value::Int(i) => match parse_int(s, 0, 64) {
                Ok(v) => {
                    *i = v;
                    Ok(())
                }
                Err(e) => {
                    *i = e.value;
                    Err(e.to_string())
                }
            },
            Value::Float64(f) => match parse_float(s, 64) {
                Ok(v) => {
                    *f = v;
                    Ok(())
                }
                Err(e) => {
                    *f = e.value;
                    Err(e.to_string())
                }
            },
            Value::Duration(d) => match parse_duration(s) {
                Ok(v) => {
                    *d = v;
                    Ok(())
                }
                Err(e) => {
                    *d = Duration(0);
                    Err(e.to_string())
                }
            },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ParseError {
    Help,
    BadSyntax(Vec<u8>),
    Unknown(Vec<u8>),
    MissingValue(Vec<u8>),
    InvalidValue {
        name: Vec<u8>,
        value: Vec<u8>,
        reason: String,
    },
}

impl ParseError {
    pub fn message(&self) -> Option<Vec<u8>> {
        let mut out = Vec::new();
        match self {
            ParseError::Help => return None,
            ParseError::BadSyntax(arg) => {
                out.extend_from_slice(b"invalid flag syntax: ");
                out.extend_from_slice(arg);
            }
            ParseError::Unknown(name) => {
                out.extend_from_slice(b"unknown flag: -");
                out.extend_from_slice(name);
            }
            ParseError::MissingValue(name) => {
                out.extend_from_slice(b"missing value for flag: -");
                out.extend_from_slice(name);
            }
            ParseError::InvalidValue {
                name,
                value,
                reason,
            } => {
                out.extend_from_slice(b"invalid value ");
                out.extend_from_slice(quote_text(value).as_bytes());
                out.extend_from_slice(b" for -");
                out.extend_from_slice(name);
                out.extend_from_slice(b": ");
                out.extend_from_slice(reason.as_bytes());
            }
        }
        Some(out)
    }

    pub fn exit_code(&self) -> i32 {
        match self {
            ParseError::Help => 0,
            _ => 2,
        }
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.message() {
            Some(msg) => f.write_str(&String::from_utf8_lossy(&msg)),
            None => f.write_str("help requested"),
        }
    }
}

impl std::error::Error for ParseError {}

pub struct FlagSet {
    usage: String,
    flags: Vec<(&'static str, Value)>,
    args: Vec<Vec<u8>>,
}

impl FlagSet {
    pub fn new(usage: impl Into<String>) -> FlagSet {
        FlagSet {
            usage: usage.into(),
            flags: Vec::new(),
            args: Vec::new(),
        }
    }

    fn define(&mut self, name: &'static str, value: Value) -> &mut FlagSet {
        assert!(self.value(name).is_none(), "flag defined twice: -{name}");
        self.flags.push((name, value));
        self
    }

    pub fn bool(&mut self, name: &'static str, default: bool) -> &mut FlagSet {
        self.define(name, Value::Bool(default))
    }

    pub fn string(&mut self, name: &'static str, default: &[u8]) -> &mut FlagSet {
        self.define(name, Value::String(default.to_vec()))
    }

    pub fn int(&mut self, name: &'static str, default: i64) -> &mut FlagSet {
        self.define(name, Value::Int(default))
    }

    pub fn float64(&mut self, name: &'static str, default: f64) -> &mut FlagSet {
        self.define(name, Value::Float64(default))
    }

    pub fn duration(&mut self, name: &'static str, default: Duration) -> &mut FlagSet {
        self.define(name, Value::Duration(default))
    }

    pub fn usage(&self) -> &str {
        &self.usage
    }

    pub fn args(&self) -> &[Vec<u8>] {
        &self.args
    }

    pub fn value(&self, name: &str) -> Option<&Value> {
        self.flags.iter().find(|(n, _)| *n == name).map(|(_, v)| v)
    }

    pub fn get_bool(&self, name: &str) -> bool {
        match self.value(name) {
            Some(Value::Bool(b)) => *b,
            _ => panic!("no boolean flag -{name}"),
        }
    }

    pub fn get_string(&self, name: &str) -> &[u8] {
        match self.value(name) {
            Some(Value::String(s)) => s,
            _ => panic!("no string flag -{name}"),
        }
    }

    pub fn get_int(&self, name: &str) -> i64 {
        match self.value(name) {
            Some(Value::Int(i)) => *i,
            _ => panic!("no integer flag -{name}"),
        }
    }

    pub fn get_float64(&self, name: &str) -> f64 {
        match self.value(name) {
            Some(Value::Float64(f)) => *f,
            _ => panic!("no number flag -{name}"),
        }
    }

    pub fn get_duration(&self, name: &str) -> Duration {
        match self.value(name) {
            Some(Value::Duration(d)) => *d,
            _ => panic!("no duration flag -{name}"),
        }
    }

    pub fn parse<I>(&mut self, args: I) -> Result<(), ParseError>
    where
        I: IntoIterator,
        I::Item: Into<Vec<u8>>,
    {
        self.args = args.into_iter().map(Into::into).collect();
        let mut pos = 0;
        let result = loop {
            match self.parse_one(&mut pos) {
                Ok(true) => {}
                Ok(false) => break Ok(()),
                Err(e) => break Err(e),
            }
        };
        self.args.drain(..pos);
        result
    }

    fn parse_one(&mut self, pos: &mut usize) -> Result<bool, ParseError> {
        let Some(s) = self.args.get(*pos) else {
            return Ok(false);
        };
        if s.len() < 2 || s[0] != b'-' {
            return Ok(false);
        }
        let mut num_minuses = 1;
        if s[1] == b'-' {
            num_minuses += 1;
            if s.len() == 2 {
                *pos += 1;
                return Ok(false);
            }
        }
        let name = &s[num_minuses..];
        if name.is_empty() || name[0] == b'-' || name[0] == b'=' {
            return Err(ParseError::BadSyntax(s.clone()));
        }
        *pos += 1;
        let (name, value) = match name[1..].iter().position(|&c| c == b'=') {
            Some(i) => (name[..=i].to_vec(), Some(name[i + 2..].to_vec())),
            None => (name.to_vec(), None),
        };
        let Some(idx) = self.flags.iter().position(|(n, _)| n.as_bytes() == name) else {
            if name == b"help" || name == b"h" {
                return Err(ParseError::Help);
            }
            return Err(ParseError::Unknown(name));
        };
        let value = if self.flags[idx].1.is_bool() {
            value.unwrap_or_else(|| b"true".to_vec())
        } else {
            match value {
                Some(v) => v,
                None => match self.args.get(*pos) {
                    Some(v) => {
                        let v = v.clone();
                        *pos += 1;
                        v
                    }
                    None => return Err(ParseError::MissingValue(name)),
                },
            }
        };
        if let Err(reason) = self.flags[idx].1.set(&value) {
            return Err(ParseError::InvalidValue {
                name,
                value,
                reason,
            });
        }
        Ok(true)
    }

    pub fn write_error(&self, err: &ParseError, out: &mut dyn Write) -> io::Result<()> {
        if let Some(msg) = err.message() {
            out.write_all(&msg)?;
            out.write_all(b"\n")?;
        }
        out.write_all(self.usage.as_bytes())
    }

    pub fn parse_or_exit<I>(&mut self, args: I)
    where
        I: IntoIterator,
        I::Item: Into<Vec<u8>>,
    {
        if let Err(err) = self.parse(args) {
            let stderr = io::stderr();
            let mut lock = stderr.lock();
            let _ = self.write_error(&err, &mut lock);
            let _ = lock.flush();
            std::process::exit(err.exit_code());
        }
    }
}
