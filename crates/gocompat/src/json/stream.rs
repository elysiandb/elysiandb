use super::decode::{Decode, DecodeState};
use super::encode::{Encode, EncodeState, append_indent};
use super::error::{Error, ErrorKind};
use super::scanner::{Scan, Scanner, SyntaxError, is_space};
use super::value::Value;
use std::io::{self, Read, Write};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TokenState {
    TopValue,
    ArrayStart,
    ArrayValue,
    ArrayComma,
    ObjectStart,
    ObjectKey,
    ObjectColon,
    ObjectValue,
    ObjectComma,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Token {
    Delim(char),
    Null,
    Bool(bool),
    Number(f64),
    String(String),
}

enum ReadFailure {
    Eof,
    Io(io::Error),
}

pub struct Decoder<R> {
    r: R,
    buf: Vec<u8>,
    len: usize,
    scanp: usize,
    scanned: i64,
    scan: Scanner,
    err: Option<Error>,
    token_state: TokenState,
    token_stack: Vec<TokenState>,
    disallow_unknown_fields: bool,
}

const MIN_READ: usize = 512;

impl<R: Read> Decoder<R> {
    pub fn new(r: R) -> Decoder<R> {
        Decoder {
            r,
            buf: Vec::new(),
            len: 0,
            scanp: 0,
            scanned: 0,
            scan: Scanner::new(),
            err: None,
            token_state: TokenState::TopValue,
            token_stack: Vec::new(),
            disallow_unknown_fields: false,
        }
    }

    pub fn disallow_unknown_fields(&mut self) {
        self.disallow_unknown_fields = true;
    }

    pub fn buffered(&self) -> &[u8] {
        &self.buf[self.scanp..self.len]
    }

    pub fn input_offset(&self) -> i64 {
        self.scanned + self.scanp as i64
    }

    pub fn into_inner(self) -> R {
        self.r
    }

    pub fn decode<T: Decode + ?Sized>(&mut self, v: &mut T) -> Result<(), Error> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        self.token_prepare_for_decode()?;
        if !self.token_value_allowed() {
            return Err(self.not_at_value());
        }
        let n = self.read_value()?;
        let start = self.scanp;
        self.scanp += n;
        let result = {
            let mut d = DecodeState::new(&self.buf[start..start + n]);
            d.disallow_unknown_fields(self.disallow_unknown_fields);
            match v.decode(&mut d) {
                Ok(()) => d.finish(),
                Err(e) => Err(e),
            }
        };
        self.token_value_end();
        result
    }

    fn not_at_value(&self) -> Error {
        let offset = self.input_offset();
        let rest = &self.buf[self.scanp..self.len];
        match rest.iter().position(|&c| !is_space(c)) {
            Some(j) => {
                let position = self.scanned + (self.scanp + j) as i64 + 1;
                SyntaxError::unexpected(offset, position, &rest[j..]).into()
            }
            None => SyntaxError::unexpected_end(offset).into(),
        }
    }

    fn syntax_at(&self, offset: i64, at: usize) -> Error {
        let position = self.scanned + at as i64 + 1;
        SyntaxError::unexpected(offset, position, &self.buf[at..self.len]).into()
    }

    fn read_value(&mut self) -> Result<usize, Error> {
        self.scan.reset();
        let mut scanp = self.scanp;
        let mut failure: Option<ReadFailure> = None;
        loop {
            while scanp < self.len {
                let c = self.buf[scanp];
                self.scan.bytes += 1;
                match self.scan.step(c) {
                    Scan::End => {
                        self.scan.bytes -= 1;
                        return Ok(scanp - self.scanp);
                    }
                    Scan::EndObject | Scan::EndArray => {
                        if self.scan.end_value_space() == Scan::End {
                            scanp += 1;
                            return Ok(scanp - self.scanp);
                        }
                    }
                    Scan::Error => {
                        let position = self.scanned + scanp as i64 + 1;
                        let err = self
                            .scan
                            .take_err()
                            .map(|e| e.located(position, &self.buf[scanp..self.len]))
                            .unwrap_or_else(|| SyntaxError::unexpected_end(position));
                        let err = Error::from(err);
                        self.err = Some(err.clone());
                        return Err(err);
                    }
                    _ => {}
                }
                scanp += 1;
            }
            if let Some(f) = failure.take() {
                let err = match f {
                    ReadFailure::Eof => {
                        if self.scan.step(b' ') == Scan::End {
                            return Ok(scanp - self.scanp);
                        }
                        if self.buf[..self.len].iter().any(|&c| !is_space(c)) {
                            Error::new(ErrorKind::UnexpectedEof, self.input_offset())
                        } else {
                            Error::new(ErrorKind::Eof, self.input_offset())
                        }
                    }
                    ReadFailure::Io(e) => Error::from(e),
                };
                self.err = Some(err.clone());
                return Err(err);
            }
            let n = scanp - self.scanp;
            failure = self.refill().err();
            scanp = self.scanp + n;
        }
    }

    fn refill(&mut self) -> Result<(), ReadFailure> {
        if self.scanp > 0 {
            self.scanned += self.scanp as i64;
            self.buf.copy_within(self.scanp..self.len, 0);
            self.len -= self.scanp;
            self.scanp = 0;
        }
        if self.buf.len() - self.len < MIN_READ {
            let cap = 2 * self.buf.len() + MIN_READ;
            self.buf.resize(cap, 0);
        }
        loop {
            match self.r.read(&mut self.buf[self.len..]) {
                Ok(0) => return Err(ReadFailure::Eof),
                Ok(n) => {
                    self.len += n;
                    return Ok(());
                }
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ReadFailure::Io(e)),
            }
        }
    }

    fn peek(&mut self) -> Result<u8, Error> {
        let mut failure: Option<ReadFailure> = None;
        loop {
            for i in self.scanp..self.len {
                let c = self.buf[i];
                if is_space(c) {
                    continue;
                }
                self.scanp = i;
                return Ok(c);
            }
            if let Some(f) = failure.take() {
                return Err(match f {
                    ReadFailure::Eof => Error::new(ErrorKind::Eof, self.input_offset()),
                    ReadFailure::Io(e) => Error::from(e),
                });
            }
            failure = self.refill().err();
        }
    }

    fn token_prepare_for_decode(&mut self) -> Result<(), Error> {
        match self.token_state {
            TokenState::ArrayComma => {
                let c = self.peek()?;
                if c != b',' {
                    return Err(self.syntax_at(self.input_offset(), self.scanp));
                }
                self.scanp += 1;
                self.token_state = TokenState::ArrayValue;
            }
            TokenState::ObjectColon => {
                let c = self.peek()?;
                if c != b':' {
                    return Err(self.syntax_at(self.input_offset(), self.scanp));
                }
                self.scanp += 1;
                self.token_state = TokenState::ObjectValue;
            }
            _ => {}
        }
        Ok(())
    }

    fn token_value_allowed(&self) -> bool {
        matches!(
            self.token_state,
            TokenState::TopValue
                | TokenState::ArrayStart
                | TokenState::ArrayValue
                | TokenState::ObjectValue
        )
    }

    fn token_value_end(&mut self) {
        match self.token_state {
            TokenState::ArrayStart | TokenState::ArrayValue => {
                self.token_state = TokenState::ArrayComma;
            }
            TokenState::ObjectValue => self.token_state = TokenState::ObjectComma,
            _ => {}
        }
    }

    fn token_error(&self) -> Error {
        self.syntax_at(self.input_offset(), self.scanp)
    }

    fn pop_token_state(&mut self) {
        self.token_state = self.token_stack.pop().unwrap_or(TokenState::TopValue);
        self.token_value_end();
    }

    pub fn token(&mut self) -> Result<Token, Error> {
        loop {
            let c = self.peek()?;
            match c {
                b'[' => {
                    if !self.token_value_allowed() {
                        return Err(self.token_error());
                    }
                    self.scanp += 1;
                    self.token_stack.push(self.token_state);
                    self.token_state = TokenState::ArrayStart;
                    return Ok(Token::Delim('['));
                }
                b']' => {
                    if self.token_state != TokenState::ArrayStart
                        && self.token_state != TokenState::ArrayComma
                    {
                        return Err(self.token_error());
                    }
                    self.scanp += 1;
                    self.pop_token_state();
                    return Ok(Token::Delim(']'));
                }
                b'{' => {
                    if !self.token_value_allowed() {
                        return Err(self.token_error());
                    }
                    self.scanp += 1;
                    self.token_stack.push(self.token_state);
                    self.token_state = TokenState::ObjectStart;
                    return Ok(Token::Delim('{'));
                }
                b'}' => {
                    if self.token_state != TokenState::ObjectStart
                        && self.token_state != TokenState::ObjectComma
                    {
                        return Err(self.token_error());
                    }
                    self.scanp += 1;
                    self.pop_token_state();
                    return Ok(Token::Delim('}'));
                }
                b':' => {
                    if self.token_state != TokenState::ObjectColon {
                        return Err(self.token_error());
                    }
                    self.scanp += 1;
                    self.token_state = TokenState::ObjectValue;
                }
                b',' => {
                    if self.token_state == TokenState::ArrayComma {
                        self.scanp += 1;
                        self.token_state = TokenState::ArrayValue;
                        continue;
                    }
                    if self.token_state == TokenState::ObjectComma {
                        self.scanp += 1;
                        self.token_state = TokenState::ObjectKey;
                        continue;
                    }
                    return Err(self.token_error());
                }
                b'"' if self.token_state == TokenState::ObjectStart
                    || self.token_state == TokenState::ObjectKey =>
                {
                    let old = self.token_state;
                    self.token_state = TokenState::TopValue;
                    let mut x = String::new();
                    let result = self.decode(&mut x);
                    self.token_state = old;
                    result?;
                    self.token_state = TokenState::ObjectColon;
                    return Ok(Token::String(x));
                }
                _ => {
                    if !self.token_value_allowed() {
                        return Err(self.token_error());
                    }
                    let mut x = Value::Null;
                    self.decode(&mut x)?;
                    return Ok(match &mut x {
                        Value::Bool(b) => Token::Bool(*b),
                        Value::Number(f) => Token::Number(*f),
                        Value::Int(n) => Token::Number(*n as f64),
                        Value::String(s) => Token::String(std::mem::take(s)),
                        _ => Token::Null,
                    });
                }
            }
        }
    }

    pub fn more(&mut self) -> bool {
        matches!(self.peek(), Ok(c) if c != b']' && c != b'}')
    }
}

pub struct Encoder<W> {
    w: W,
    err: Option<Error>,
    escape_html: bool,
    prefix: String,
    indent: String,
}

impl<W: Write> Encoder<W> {
    pub fn new(w: W) -> Encoder<W> {
        Encoder {
            w,
            err: None,
            escape_html: true,
            prefix: String::new(),
            indent: String::new(),
        }
    }

    pub fn set_escape_html(&mut self, on: bool) {
        self.escape_html = on;
    }

    pub fn set_indent(&mut self, prefix: &str, indent: &str) {
        self.prefix = prefix.to_string();
        self.indent = indent.to_string();
    }

    pub fn get_ref(&self) -> &W {
        &self.w
    }

    pub fn get_mut(&mut self) -> &mut W {
        &mut self.w
    }

    pub fn into_inner(self) -> W {
        self.w
    }

    pub fn encode<T: Encode + ?Sized>(&mut self, v: &T) -> Result<(), Error> {
        if let Some(e) = &self.err {
            return Err(e.clone());
        }
        let mut e = EncodeState::new(self.escape_html);
        v.encode(&mut e)?;
        e.write_byte(b'\n');
        let mut b = e.into_bytes();
        if !self.prefix.is_empty() || !self.indent.is_empty() {
            let mut out = Vec::with_capacity(2 * b.len());
            append_indent(&mut out, &b, self.prefix.as_bytes(), self.indent.as_bytes())?;
            b = out;
        }
        if let Err(err) = self.w.write_all(&b) {
            let err = Error::from(err);
            self.err = Some(err.clone());
            return Err(err);
        }
        Ok(())
    }
}
