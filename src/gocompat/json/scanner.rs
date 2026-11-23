use std::fmt;

const MAX_NESTING_DEPTH: usize = 10000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Scan {
    Continue,
    BeginLiteral,
    BeginObject,
    ObjectKey,
    ObjectValue,
    EndObject,
    BeginArray,
    ArrayValue,
    EndArray,
    SkipSpace,
    End,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ParseState {
    ObjectKey,
    ObjectValue,
    ArrayValue,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    BeginValue,
    BeginValueOrEmpty,
    BeginStringOrEmpty,
    BeginString,
    EndValue,
    EndTop,
    InString,
    InStringEsc,
    InStringEscU,
    InStringEscU1,
    InStringEscU12,
    InStringEscU123,
    Neg,
    One,
    Zero,
    Dot,
    Dot0,
    E,
    ESign,
    E0,
    T,
    Tr,
    Tru,
    F,
    Fa,
    Fal,
    Fals,
    N,
    Nu,
    Nul,
    Error,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxReason {
    UnexpectedCharacter,
    InvalidEscape,
    InvalidNumber,
    NestingTooDeep,
    UnexpectedEnd,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub reason: SyntaxReason,
    pub offset: i64,
    pub byte: Option<u8>,
    pub at_eof: bool,
    character: Option<String>,
}

impl SyntaxError {
    pub fn message(&self) -> String {
        match self.reason {
            SyntaxReason::UnexpectedEnd => "unexpected end of input".to_string(),
            SyntaxReason::InvalidEscape => "invalid escape sequence".to_string(),
            SyntaxReason::InvalidNumber => "invalid number".to_string(),
            SyntaxReason::NestingTooDeep => "nesting too deep".to_string(),
            SyntaxReason::UnexpectedCharacter => match &self.character {
                Some(c) => format!("unexpected character '{c}'"),
                None => "invalid UTF-8".to_string(),
            },
        }
    }

    pub fn with_input(mut self, data: &[u8]) -> SyntaxError {
        if self.reason == SyntaxReason::UnexpectedCharacter && self.offset > 0 {
            let pos = (self.offset - 1) as usize;
            self.character = display_char(&data[pos.min(data.len())..]);
        }
        self
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.at_eof || self.reason == SyntaxReason::UnexpectedEnd {
            write!(f, "invalid JSON: unexpected end of input")
        } else {
            write!(
                f,
                "invalid JSON at byte {}: {}",
                self.offset,
                self.message()
            )
        }
    }
}

impl std::error::Error for SyntaxError {}

fn display_char(rest: &[u8]) -> Option<String> {
    let c = *rest.first()?;
    if c < 0x20 {
        return Some(format!("\\^{}", (c ^ 0x40) as char));
    }
    if c == 0x7f {
        return Some("\\^?".to_string());
    }
    if c < 0x80 {
        return Some((c as char).to_string());
    }
    let (r, size) = crate::gocompat::utf8::decode_rune(rest);
    if r == crate::gocompat::utf8::RUNE_ERROR && size == 1 {
        return None;
    }
    std::str::from_utf8(&rest[..size]).ok().map(str::to_string)
}

pub struct Scanner {
    step: Step,
    end_top: bool,
    parse_state: Vec<ParseState>,
    err: Option<SyntaxError>,
    pub bytes: i64,
    at_eof: bool,
}

impl Default for Scanner {
    fn default() -> Self {
        Self::new()
    }
}

impl Scanner {
    pub fn new() -> Scanner {
        Scanner {
            step: Step::BeginValue,
            end_top: false,
            parse_state: Vec::new(),
            err: None,
            bytes: 0,
            at_eof: false,
        }
    }

    pub fn reset(&mut self) {
        self.step = Step::BeginValue;
        self.parse_state.clear();
        self.err = None;
        self.end_top = false;
        self.at_eof = false;
    }

    pub fn err(&self) -> Option<&SyntaxError> {
        self.err.as_ref()
    }

    pub fn take_err(&mut self) -> Option<SyntaxError> {
        self.err.take()
    }

    pub fn eof(&mut self) -> Scan {
        if self.err.is_some() {
            return Scan::Error;
        }
        if self.end_top {
            return Scan::End;
        }
        self.at_eof = true;
        self.step(b' ');
        if self.end_top {
            return Scan::End;
        }
        if self.err.is_none() {
            self.err = Some(SyntaxError {
                reason: SyntaxReason::UnexpectedEnd,
                offset: self.bytes,
                byte: None,
                at_eof: true,
                character: None,
            });
        }
        Scan::Error
    }

    fn push_parse_state(&mut self, c: u8, state: ParseState, success: Scan) -> Scan {
        self.parse_state.push(state);
        if self.parse_state.len() <= MAX_NESTING_DEPTH {
            return success;
        }
        self.error(c, SyntaxReason::NestingTooDeep)
    }

    fn pop_parse_state(&mut self) {
        self.parse_state.pop();
        if self.parse_state.is_empty() {
            self.step = Step::EndTop;
            self.end_top = true;
        } else {
            self.step = Step::EndValue;
        }
    }

    fn error(&mut self, c: u8, reason: SyntaxReason) -> Scan {
        self.step = Step::Error;
        self.err = Some(SyntaxError {
            reason,
            offset: self.bytes,
            byte: Some(c),
            at_eof: self.at_eof,
            character: None,
        });
        Scan::Error
    }

    pub fn step(&mut self, c: u8) -> Scan {
        match self.step {
            Step::BeginValue => self.state_begin_value(c),
            Step::BeginValueOrEmpty => self.state_begin_value_or_empty(c),
            Step::BeginStringOrEmpty => self.state_begin_string_or_empty(c),
            Step::BeginString => self.state_begin_string(c),
            Step::EndValue => self.state_end_value(c),
            Step::EndTop => self.state_end_top(c),
            Step::InString => self.state_in_string(c),
            Step::InStringEsc => self.state_in_string_esc(c),
            Step::InStringEscU => self.state_esc_hex(c, Step::InStringEscU1),
            Step::InStringEscU1 => self.state_esc_hex(c, Step::InStringEscU12),
            Step::InStringEscU12 => self.state_esc_hex(c, Step::InStringEscU123),
            Step::InStringEscU123 => self.state_esc_hex(c, Step::InString),
            Step::Neg => self.state_neg(c),
            Step::One => self.state_1(c),
            Step::Zero => self.state_0(c),
            Step::Dot => self.state_dot(c),
            Step::Dot0 => self.state_dot0(c),
            Step::E => self.state_e(c),
            Step::ESign => self.state_e_sign(c),
            Step::E0 => self.state_e0(c),
            Step::T => self.state_literal(c, b'r', Step::Tr),
            Step::Tr => self.state_literal(c, b'u', Step::Tru),
            Step::Tru => self.state_literal(c, b'e', Step::EndValue),
            Step::F => self.state_literal(c, b'a', Step::Fa),
            Step::Fa => self.state_literal(c, b'l', Step::Fal),
            Step::Fal => self.state_literal(c, b's', Step::Fals),
            Step::Fals => self.state_literal(c, b'e', Step::EndValue),
            Step::N => self.state_literal(c, b'u', Step::Nu),
            Step::Nu => self.state_literal(c, b'l', Step::Nul),
            Step::Nul => self.state_literal(c, b'l', Step::EndValue),
            Step::Error => Scan::Error,
        }
    }

    fn state_begin_value_or_empty(&mut self, c: u8) -> Scan {
        if is_space(c) {
            return Scan::SkipSpace;
        }
        if c == b']' {
            return self.state_end_value(c);
        }
        self.state_begin_value(c)
    }

    fn state_begin_value(&mut self, c: u8) -> Scan {
        if is_space(c) {
            return Scan::SkipSpace;
        }
        match c {
            b'{' => {
                self.step = Step::BeginStringOrEmpty;
                self.push_parse_state(c, ParseState::ObjectKey, Scan::BeginObject)
            }
            b'[' => {
                self.step = Step::BeginValueOrEmpty;
                self.push_parse_state(c, ParseState::ArrayValue, Scan::BeginArray)
            }
            b'"' => {
                self.step = Step::InString;
                Scan::BeginLiteral
            }
            b'-' => {
                self.step = Step::Neg;
                Scan::BeginLiteral
            }
            b'0' => {
                self.step = Step::Zero;
                Scan::BeginLiteral
            }
            b't' => {
                self.step = Step::T;
                Scan::BeginLiteral
            }
            b'f' => {
                self.step = Step::F;
                Scan::BeginLiteral
            }
            b'n' => {
                self.step = Step::N;
                Scan::BeginLiteral
            }
            b'1'..=b'9' => {
                self.step = Step::One;
                Scan::BeginLiteral
            }
            _ => self.error(c, SyntaxReason::UnexpectedCharacter),
        }
    }

    fn state_begin_string_or_empty(&mut self, c: u8) -> Scan {
        if is_space(c) {
            return Scan::SkipSpace;
        }
        if c == b'}' {
            if let Some(last) = self.parse_state.last_mut() {
                *last = ParseState::ObjectValue;
            }
            return self.state_end_value(c);
        }
        self.state_begin_string(c)
    }

    fn state_begin_string(&mut self, c: u8) -> Scan {
        if is_space(c) {
            return Scan::SkipSpace;
        }
        if c == b'"' {
            self.step = Step::InString;
            return Scan::BeginLiteral;
        }
        self.error(c, SyntaxReason::UnexpectedCharacter)
    }

    fn state_end_value(&mut self, c: u8) -> Scan {
        let Some(&ps) = self.parse_state.last() else {
            self.step = Step::EndTop;
            self.end_top = true;
            return self.state_end_top(c);
        };
        if is_space(c) {
            self.step = Step::EndValue;
            return Scan::SkipSpace;
        }
        match ps {
            ParseState::ObjectKey => {
                if c == b':' {
                    if let Some(last) = self.parse_state.last_mut() {
                        *last = ParseState::ObjectValue;
                    }
                    self.step = Step::BeginValue;
                    return Scan::ObjectKey;
                }
                self.error(c, SyntaxReason::UnexpectedCharacter)
            }
            ParseState::ObjectValue => {
                if c == b',' {
                    if let Some(last) = self.parse_state.last_mut() {
                        *last = ParseState::ObjectKey;
                    }
                    self.step = Step::BeginString;
                    return Scan::ObjectValue;
                }
                if c == b'}' {
                    self.pop_parse_state();
                    return Scan::EndObject;
                }
                self.error(c, SyntaxReason::UnexpectedCharacter)
            }
            ParseState::ArrayValue => {
                if c == b',' {
                    self.step = Step::BeginValue;
                    return Scan::ArrayValue;
                }
                if c == b']' {
                    self.pop_parse_state();
                    return Scan::EndArray;
                }
                self.error(c, SyntaxReason::UnexpectedCharacter)
            }
        }
    }

    fn state_end_top(&mut self, c: u8) -> Scan {
        if !is_space(c) {
            self.error(c, SyntaxReason::UnexpectedCharacter);
        }
        Scan::End
    }

    fn state_in_string(&mut self, c: u8) -> Scan {
        if c == b'"' {
            self.step = Step::EndValue;
            return Scan::Continue;
        }
        if c == b'\\' {
            self.step = Step::InStringEsc;
            return Scan::Continue;
        }
        if c < 0x20 {
            return self.error(c, SyntaxReason::UnexpectedCharacter);
        }
        Scan::Continue
    }

    fn state_in_string_esc(&mut self, c: u8) -> Scan {
        match c {
            b'b' | b'f' | b'n' | b'r' | b't' | b'\\' | b'/' | b'"' => {
                self.step = Step::InString;
                Scan::Continue
            }
            b'u' => {
                self.step = Step::InStringEscU;
                Scan::Continue
            }
            _ => self.error(c, SyntaxReason::InvalidEscape),
        }
    }

    fn state_esc_hex(&mut self, c: u8, next: Step) -> Scan {
        if c.is_ascii_hexdigit() {
            self.step = next;
            return Scan::Continue;
        }
        self.error(c, SyntaxReason::InvalidEscape)
    }

    fn state_neg(&mut self, c: u8) -> Scan {
        if c == b'0' {
            self.step = Step::Zero;
            return Scan::Continue;
        }
        if (b'1'..=b'9').contains(&c) {
            self.step = Step::One;
            return Scan::Continue;
        }
        self.error(c, SyntaxReason::InvalidNumber)
    }

    fn state_1(&mut self, c: u8) -> Scan {
        if c.is_ascii_digit() {
            self.step = Step::One;
            return Scan::Continue;
        }
        self.state_0(c)
    }

    fn state_0(&mut self, c: u8) -> Scan {
        if c == b'.' {
            self.step = Step::Dot;
            return Scan::Continue;
        }
        if c == b'e' || c == b'E' {
            self.step = Step::E;
            return Scan::Continue;
        }
        self.state_end_value(c)
    }

    fn state_dot(&mut self, c: u8) -> Scan {
        if c.is_ascii_digit() {
            self.step = Step::Dot0;
            return Scan::Continue;
        }
        self.error(c, SyntaxReason::InvalidNumber)
    }

    fn state_dot0(&mut self, c: u8) -> Scan {
        if c.is_ascii_digit() {
            return Scan::Continue;
        }
        if c == b'e' || c == b'E' {
            self.step = Step::E;
            return Scan::Continue;
        }
        self.state_end_value(c)
    }

    fn state_e(&mut self, c: u8) -> Scan {
        if c == b'+' || c == b'-' {
            self.step = Step::ESign;
            return Scan::Continue;
        }
        self.state_e_sign(c)
    }

    fn state_e_sign(&mut self, c: u8) -> Scan {
        if c.is_ascii_digit() {
            self.step = Step::E0;
            return Scan::Continue;
        }
        self.error(c, SyntaxReason::InvalidNumber)
    }

    fn state_e0(&mut self, c: u8) -> Scan {
        if c.is_ascii_digit() {
            return Scan::Continue;
        }
        self.state_end_value(c)
    }

    fn state_literal(&mut self, c: u8, want: u8, next: Step) -> Scan {
        if c == want {
            self.step = next;
            return Scan::Continue;
        }
        self.error(c, SyntaxReason::UnexpectedCharacter)
    }
}

pub fn is_space(c: u8) -> bool {
    c <= b' ' && (c == b' ' || c == b'\t' || c == b'\r' || c == b'\n')
}

pub fn check_valid(data: &[u8]) -> Result<(), SyntaxError> {
    let mut scan = Scanner::new();
    for &c in data {
        scan.bytes += 1;
        if scan.step(c) == Scan::Error {
            return Err(scan
                .take_err()
                .map(|e| e.with_input(data))
                .expect("scanner error"));
        }
    }
    if scan.eof() == Scan::Error {
        return Err(scan
            .take_err()
            .map(|e| e.with_input(data))
            .expect("scanner error"));
    }
    Ok(())
}

pub fn valid(data: &[u8]) -> bool {
    check_valid(data).is_ok()
}
