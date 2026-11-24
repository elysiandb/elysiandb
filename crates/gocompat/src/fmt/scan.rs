use std::fmt;
use std::io::{self, Read};

use crate::strconv;
use crate::utf8::{self, Rune};

const EOF: Rune = -1;
const HUGE_WID: usize = 1 << 30;

#[derive(Debug)]
pub enum ScanError {
    Eof,
    Io(io::Error),
    UnexpectedNewline,
    ExpectedNewline,
    ExpectedInteger,
    Integer(Vec<u8>),
}

impl fmt::Display for ScanError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ScanError::Eof => f.write_str("end of input"),
            ScanError::Io(e) => e.fmt(f),
            ScanError::UnexpectedNewline => f.write_str("unexpected newline"),
            ScanError::ExpectedNewline => f.write_str("expected newline"),
            ScanError::ExpectedInteger => f.write_str("expected integer"),
            ScanError::Integer(tok) => write!(
                f,
                "{:?} is not a valid integer",
                String::from_utf8_lossy(tok)
            ),
        }
    }
}

impl std::error::Error for ScanError {}

struct ReadRune<'a> {
    reader: &'a mut dyn Read,
    buf: [u8; utf8::UTF_MAX],
    pending: usize,
    pend_buf: [u8; utf8::UTF_MAX],
    peek_rune: Rune,
}

impl ReadRune<'_> {
    fn read_byte(&mut self) -> Result<u8, ScanError> {
        if self.pending > 0 {
            let b = self.pend_buf[0];
            self.pend_buf.copy_within(1.., 0);
            self.pending -= 1;
            return Ok(b);
        }
        let mut one = [0u8; 1];
        loop {
            match self.reader.read(&mut one) {
                Ok(0) => return Err(ScanError::Eof),
                Ok(_) => return Ok(one[0]),
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(ScanError::Io(e)),
            }
        }
    }

    fn read_rune(&mut self) -> Result<Rune, ScanError> {
        if self.peek_rune >= 0 {
            let rr = self.peek_rune;
            self.peek_rune = !self.peek_rune;
            return Ok(rr);
        }
        self.buf[0] = self.read_byte()?;
        if self.buf[0] < utf8::RUNE_SELF {
            let rr = Rune::from(self.buf[0]);
            self.peek_rune = !rr;
            return Ok(rr);
        }
        let mut n = 1;
        while !utf8::full_rune(&self.buf[..n]) {
            match self.read_byte() {
                Ok(b) => {
                    self.buf[n] = b;
                    n += 1;
                }
                Err(ScanError::Eof) => break,
                Err(e) => return Err(e),
            }
        }
        let (rr, size) = utf8::decode_rune(&self.buf[..n]);
        if size < n {
            let extra = n - size;
            self.pend_buf[self.pending..self.pending + extra].copy_from_slice(&self.buf[size..n]);
            self.pending += extra;
        }
        self.peek_rune = !rr;
        Ok(rr)
    }

    fn unread_rune(&mut self) {
        if self.peek_rune < 0 {
            self.peek_rune = !self.peek_rune;
        }
    }
}

struct State<'a> {
    rs: ReadRune<'a>,
    buf: Vec<u8>,
    count: usize,
    at_eof: bool,
    nl_is_end: bool,
    nl_is_space: bool,
    arg_limit: usize,
}

fn is_space(r: Rune) -> bool {
    const SPACE: [(Rune, Rune); 10] = [
        (0x0009, 0x000d),
        (0x0020, 0x0020),
        (0x0085, 0x0085),
        (0x00a0, 0x00a0),
        (0x1680, 0x1680),
        (0x2000, 0x200a),
        (0x2028, 0x2029),
        (0x202f, 0x202f),
        (0x205f, 0x205f),
        (0x3000, 0x3000),
    ];
    if !(0..1 << 16).contains(&r) {
        return false;
    }
    for (lo, hi) in SPACE {
        if r < lo {
            return false;
        }
        if r <= hi {
            return true;
        }
    }
    false
}

fn index_rune(s: &str, r: Rune) -> bool {
    s.chars().any(|c| c as Rune == r)
}

impl<'a> State<'a> {
    fn new(r: &'a mut dyn Read, nl_is_space: bool, nl_is_end: bool) -> State<'a> {
        State {
            rs: ReadRune {
                reader: r,
                buf: [0; utf8::UTF_MAX],
                pending: 0,
                pend_buf: [0; utf8::UTF_MAX],
                peek_rune: -1,
            },
            buf: Vec::new(),
            count: 0,
            at_eof: false,
            nl_is_end,
            nl_is_space,
            arg_limit: HUGE_WID,
        }
    }

    fn read_rune(&mut self) -> Result<Rune, ScanError> {
        if self.at_eof || self.count >= self.arg_limit {
            return Err(ScanError::Eof);
        }
        match self.rs.read_rune() {
            Ok(r) => {
                self.count += 1;
                if self.nl_is_end && r == '\n' as Rune {
                    self.at_eof = true;
                }
                Ok(r)
            }
            Err(ScanError::Eof) => {
                self.at_eof = true;
                Err(ScanError::Eof)
            }
            Err(e) => Err(e),
        }
    }

    fn get_rune(&mut self) -> Result<Rune, ScanError> {
        match self.read_rune() {
            Ok(r) => Ok(r),
            Err(ScanError::Eof) => Ok(EOF),
            Err(e) => Err(e),
        }
    }

    fn unread_rune(&mut self) {
        self.rs.unread_rune();
        self.at_eof = false;
        self.count = self.count.saturating_sub(1);
    }

    fn write_rune(&mut self, r: Rune) {
        utf8::append_rune(&mut self.buf, r);
    }

    fn skip_space(&mut self) -> Result<(), ScanError> {
        loop {
            let r = self.get_rune()?;
            if r == EOF {
                return Ok(());
            }
            if r == '\r' as Rune && self.peek("\n")? {
                continue;
            }
            if r == '\n' as Rune {
                if self.nl_is_space {
                    continue;
                }
                return Err(ScanError::UnexpectedNewline);
            }
            if !is_space(r) {
                self.unread_rune();
                return Ok(());
            }
        }
    }

    fn token(&mut self, skip_space: bool) -> Result<(), ScanError> {
        if skip_space {
            self.skip_space()?;
        }
        loop {
            let r = self.get_rune()?;
            if r == EOF {
                break;
            }
            if is_space(r) {
                self.unread_rune();
                break;
            }
            self.write_rune(r);
        }
        Ok(())
    }

    fn consume(&mut self, ok: &str, accept: bool) -> Result<bool, ScanError> {
        let r = self.get_rune()?;
        if r == EOF {
            return Ok(false);
        }
        if index_rune(ok, r) {
            if accept {
                self.write_rune(r);
            }
            return Ok(true);
        }
        if accept {
            self.unread_rune();
        }
        Ok(false)
    }

    fn peek(&mut self, ok: &str) -> Result<bool, ScanError> {
        let r = self.get_rune()?;
        if r != EOF {
            self.unread_rune();
        }
        Ok(index_rune(ok, r))
    }

    fn not_eof(&mut self) -> Result<(), ScanError> {
        let r = self.get_rune()?;
        if r == EOF {
            return Err(ScanError::Eof);
        }
        self.unread_rune();
        Ok(())
    }

    fn accept(&mut self, ok: &str) -> Result<bool, ScanError> {
        self.consume(ok, true)
    }

    fn convert_string(&mut self) -> Result<Vec<u8>, ScanError> {
        self.skip_space()?;
        self.not_eof()?;
        self.token(true)?;
        Ok(self.buf.clone())
    }

    fn scan_base_prefix(&mut self) -> Result<(u32, &'static str, bool), ScanError> {
        if !self.peek("0")? {
            return Ok((0, "0123456789_", false));
        }
        self.accept("0")?;
        if self.peek("bB")? {
            self.consume("bB", true)?;
            return Ok((0, "01_", true));
        }
        if self.peek("oO")? {
            self.consume("oO", true)?;
            return Ok((0, "01234567_", true));
        }
        if self.peek("xX")? {
            self.consume("xX", true)?;
            return Ok((0, "0123456789aAbBcCdDeEfF_", true));
        }
        Ok((0, "01234567_", true))
    }

    fn scan_number(&mut self, digits: &str, have_digits: bool) -> Result<Vec<u8>, ScanError> {
        if !have_digits {
            self.not_eof()?;
            if !self.accept(digits)? {
                return Err(ScanError::ExpectedInteger);
            }
        }
        while self.accept(digits)? {}
        Ok(self.buf.clone())
    }

    fn scan_int(&mut self) -> Result<i64, ScanError> {
        self.skip_space()?;
        self.not_eof()?;
        self.accept("+-")?;
        let (base, digits, have_digits) = self.scan_base_prefix()?;
        let tok = self.scan_number(digits, have_digits)?;
        strconv::parse_int(&tok, base, 64).map_err(|_| ScanError::Integer(tok))
    }

    fn finish_line(&mut self) -> Result<(), ScanError> {
        if self.nl_is_end {
            loop {
                let r = self.get_rune()?;
                if r == '\n' as Rune || r == EOF {
                    break;
                }
                if !is_space(r) {
                    return Err(ScanError::ExpectedNewline);
                }
            }
        }
        Ok(())
    }
}

pub fn fscanf_word(r: &mut dyn Read) -> (Vec<u8>, Result<(), ScanError>) {
    let mut s = State::new(r, false, false);
    let mut word = Vec::new();
    let result = s.skip_space().and_then(|()| {
        word = s.convert_string()?;
        Ok(())
    });
    (word, result)
}

pub fn fscanln_word(r: &mut dyn Read) -> (Vec<u8>, Result<(), ScanError>) {
    let mut s = State::new(r, false, true);
    let mut word = Vec::new();
    let result = s.convert_string().and_then(|w| {
        word = w;
        s.finish_line()
    });
    (word, result)
}

pub fn fscanln_int(r: &mut dyn Read) -> (i64, Result<(), ScanError>) {
    let mut s = State::new(r, false, true);
    let mut n = 0;
    let result = s.scan_int().and_then(|v| {
        n = v;
        s.finish_line()
    });
    (n, result)
}
