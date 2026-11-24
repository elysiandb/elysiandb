use super::parser::Parser;
use crate::file::{Idx, Position};
use crate::token::{self, Token};
use gocompat::fmt::{Arg, sprintf};
use gocompat::utf8::Rune;
use std::fmt;

pub const ERR_UNEXPECTED_TOKEN: &str = "Unexpected token %v";
pub const ERR_UNEXPECTED_END_OF_INPUT: &str = "Unexpected end of input";
pub const ERR_UNEXPECTED_ESCAPE: &str = "Unexpected escape";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub position: Position,
    pub message: String,
}

impl Error {
    pub fn error(&self) -> String {
        let filename = if self.position.filename.is_empty() {
            "(anonymous)"
        } else {
            self.position.filename.as_str()
        };
        format!(
            "{}: Line {}:{} {}",
            filename, self.position.line, self.position.column, self.message
        )
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error())
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ErrorList(pub Vec<Error>);

impl ErrorList {
    pub fn add(&mut self, position: Position, msg: String) {
        self.0.push(Error {
            position,
            message: msg,
        });
    }

    pub fn reset(&mut self) {
        self.0.clear();
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    fn less(&self, i: usize, j: usize) -> bool {
        let x = &self.0[i].position;
        let y = &self.0[j].position;
        if x.filename < y.filename {
            return true;
        }
        if x.filename == y.filename {
            if x.line < y.line {
                return true;
            }
            if x.line == y.line {
                return x.column < y.column;
            }
        }
        false
    }

    pub fn sort(&mut self) {
        let mut idx: Vec<usize> = (0..self.0.len()).collect();
        gocompat::sort::slice(&mut idx, |&a, &b| self.less(a, b));
        let sorted: Vec<Error> = idx.into_iter().map(|i| self.0[i].clone()).collect();
        self.0 = sorted;
    }

    pub fn error(&self) -> String {
        match self.0.len() {
            0 => "no errors".to_string(),
            1 => self.0[0].error(),
            n => format!("{} (and {} more errors)", self.0[0].error(), n - 1),
        }
    }

    pub fn err(self) -> Option<ErrorList> {
        if self.0.is_empty() { None } else { Some(self) }
    }
}

impl fmt::Display for ErrorList {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.error())
    }
}

impl std::error::Error for ErrorList {}

pub enum Place {
    Offset(usize),
    Idx(Idx),
}

impl Parser {
    pub fn error(&mut self, place: Place, msg: &str, args: &[Arg<'_>]) -> usize {
        let idx = match place {
            Place::Offset(o) => self.idx_of(o),
            Place::Idx(0) => self.idx_of(self.chr_offset),
            Place::Idx(i) => i,
        };
        let position = self.position(idx);
        let msg = String::from_utf8_lossy(&sprintf(msg, args)).into_owned();
        self.errors.add(position, msg);
        self.errors.len() - 1
    }

    pub fn error_idx(&mut self, idx: Idx, msg: &str) {
        self.error(Place::Idx(idx), msg, &[]);
    }

    pub fn error_offset(&mut self, offset: usize, msg: &str) {
        self.error(Place::Offset(offset), msg, &[]);
    }

    pub fn error_unexpected(&mut self, idx: Idx, chr: Rune) {
        if chr == -1 {
            self.error_idx(idx, ERR_UNEXPECTED_END_OF_INPUT);
            return;
        }
        let ill = token::ILLEGAL.string();
        self.error(
            Place::Idx(idx),
            ERR_UNEXPECTED_TOKEN,
            &[Arg::Str(ill.as_bytes())],
        );
    }

    pub fn error_unexpected_token(&mut self, tkn: Token) {
        if tkn == token::EOF {
            self.error_idx(0, ERR_UNEXPECTED_END_OF_INPUT);
            return;
        }
        let mut value = tkn.string().into_bytes();
        let idx = self.idx;
        match tkn {
            token::BOOLEAN | token::NULL => value = self.literal.clone(),
            token::IDENTIFIER => {
                self.error_idx(idx, "Unexpected identifier");
                return;
            }
            token::KEYWORD => {
                self.error_idx(idx, "Unexpected reserved word");
                return;
            }
            token::ESCAPED_RESERVED_WORD => {
                self.error_idx(idx, "Keyword must not contain escaped characters");
                return;
            }
            token::NUMBER => {
                self.error_idx(idx, "Unexpected number");
                return;
            }
            token::STRING => {
                self.error_idx(idx, "Unexpected string");
                return;
            }
            _ => {}
        }
        self.error(Place::Idx(idx), ERR_UNEXPECTED_TOKEN, &[Arg::Str(&value)]);
    }
}
