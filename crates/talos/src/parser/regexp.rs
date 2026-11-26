use super::lexer::{digit_value, is_identifier_part};
use gocompat::fmt::{Arg, sprintf};
use gocompat::strconv::append_int;
use gocompat::utf8::{self, RUNE_ERROR, RUNE_SELF, Rune};
use std::fmt;

pub const WHITESPACE_CHARS: &str = " \x0c\n\r\t\x0b\u{a0}\u{1680}\u{2000}\u{2001}\u{2002}\u{2003}\u{2004}\u{2005}\u{2006}\u{2007}\u{2008}\u{2009}\u{200a}\u{2028}\u{2029}\u{202f}\u{205f}\u{3000}\u{feff}";
pub const RE2_DOT: &str = "[^\r\n\u{2028}\u{2029}]";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexpParseError {
    pub offset: usize,
    pub err: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexpErrorIncompatible(pub RegexpParseError);

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RegexpSyntaxError(pub RegexpParseError);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TransformError {
    Incompatible(RegexpErrorIncompatible),
    Syntax(RegexpSyntaxError),
}

impl TransformError {
    pub fn is_incompatible(&self) -> bool {
        matches!(self, TransformError::Incompatible(_))
    }

    pub fn message(&self) -> &str {
        match self {
            TransformError::Incompatible(e) => &e.0.err,
            TransformError::Syntax(e) => &e.0.err,
        }
    }
}

impl fmt::Display for TransformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.message())
    }
}

impl std::error::Error for TransformError {}

struct RegExpParser<'a> {
    str: &'a [u8],
    length: usize,
    chr: Rune,
    chr_offset: usize,
    offset: usize,
    err: Option<TransformError>,
    go_regexp: Vec<u8>,
    pass_offset: Option<usize>,
    dot_all: bool,
    unicode: bool,
}

pub fn transform_reg_exp(
    pattern: &[u8],
    dot_all: bool,
    unicode: bool,
) -> Result<Vec<u8>, TransformError> {
    if pattern.is_empty() {
        return Ok(Vec::new());
    }
    let mut parser = RegExpParser {
        str: pattern,
        length: pattern.len(),
        chr: 0,
        chr_offset: 0,
        offset: 0,
        err: None,
        go_regexp: Vec::new(),
        pass_offset: Some(0),
        dot_all,
        unicode,
    };
    parser.parse();
    if let Some(err) = parser.err {
        return Err(err);
    }
    Ok(parser.result_string())
}

const BACKSLASH: Rune = b'\\' as Rune;

impl RegExpParser<'_> {
    fn result_string(&self) -> Vec<u8> {
        match self.pass_offset {
            Some(p) => self.str[..p].to_vec(),
            None => self.go_regexp.clone(),
        }
    }

    fn parse(&mut self) {
        self.read();
        self.scan();
    }

    fn read(&mut self) {
        if self.offset < self.length {
            self.chr_offset = self.offset;
            let mut chr = Rune::from(self.str[self.offset]);
            let mut width = 1;
            if chr >= Rune::from(RUNE_SELF) {
                let (c, w) = utf8::decode_rune(&self.str[self.offset..]);
                chr = c;
                width = w;
                if chr == RUNE_ERROR && width == 1 {
                    self.error(true, "Invalid UTF-8 character", &[]);
                    return;
                }
            }
            self.offset += width;
            self.chr = chr;
        } else {
            self.chr_offset = self.length;
            self.chr = -1;
        }
    }

    fn stop_passing(&mut self) {
        if let Some(p) = self.pass_offset {
            self.go_regexp.reserve(3 * self.str.len() / 2);
            self.go_regexp.extend_from_slice(&self.str[..p]);
        }
        self.pass_offset = None;
    }

    fn write(&mut self, p: &[u8]) {
        if self.pass_offset.is_some() {
            self.stop_passing();
        }
        self.go_regexp.extend_from_slice(p);
    }

    fn write_byte(&mut self, b: u8) {
        self.write(&[b]);
    }

    fn write_string(&mut self, s: &str) {
        self.write(s.as_bytes());
    }

    fn scan(&mut self) {
        while self.chr != -1 {
            match self.chr {
                0x5C => {
                    self.read();
                    self.scan_escape(false);
                }
                0x28 => {
                    self.pass();
                    self.scan_group();
                }
                0x5B => self.scan_bracket(),
                0x29 => {
                    self.error(true, "Unmatched ')'", &[]);
                    return;
                }
                0x2E => {
                    if self.dot_all {
                        self.pass();
                        continue;
                    }
                    self.write_string(RE2_DOT);
                    self.read();
                }
                _ => self.pass(),
            }
        }
    }

    fn scan_group(&mut self) {
        let s = &self.str[self.chr_offset..];
        if s.len() > 1 && s[0] == b'?' {
            let ch = s[1];
            let two = &self.str[self.chr_offset..self.chr_offset + 2];
            if ch == b'=' || ch == b'!' {
                self.error(false, "re2: Invalid (%s) <lookahead>", &[Arg::Str(two)]);
                return;
            } else if ch == b'<' {
                self.error(false, "re2: Invalid (%s) <lookbehind>", &[Arg::Str(two)]);
                return;
            } else if ch != b':' {
                self.error(true, "Invalid group", &[]);
                return;
            }
        }
        while self.chr != -1 && self.chr != 0x29 {
            match self.chr {
                0x5C => {
                    self.read();
                    self.scan_escape(false);
                }
                0x28 => {
                    self.pass();
                    self.scan_group();
                }
                0x5B => self.scan_bracket(),
                0x2E => {
                    if self.dot_all {
                        self.pass();
                        continue;
                    }
                    self.write_string(RE2_DOT);
                    self.read();
                }
                _ => {
                    self.pass();
                    continue;
                }
            }
        }
        if self.chr != 0x29 {
            self.error(true, "Unterminated group", &[]);
            return;
        }
        self.pass();
    }

    fn scan_bracket(&mut self) {
        let s = &self.str[self.chr_offset..];
        if s.starts_with(b"[]") {
            self.write_string("[^\u{0}-\u{1FFFF}]");
            self.offset += 1;
            self.read();
            return;
        }
        if s.starts_with(b"[^]") {
            self.write_string("[\u{0}-\u{1FFFF}]");
            self.offset += 2;
            self.read();
            return;
        }
        self.pass();
        while self.chr != -1 {
            if self.chr == 0x5D {
                break;
            } else if self.chr == BACKSLASH {
                self.read();
                self.scan_escape(true);
                continue;
            }
            self.pass();
        }
        if self.chr != 0x5D {
            self.error(true, "Unterminated character class", &[]);
            return;
        }
        self.pass();
    }

    fn write_hex_escape(&mut self, value: i64) {
        let mut tmp: Vec<u8> = if value >= 16 {
            b"\\x".to_vec()
        } else {
            b"\\x0".to_vec()
        };
        append_int(&mut tmp, value, 16);
        self.write(&tmp);
    }

    fn scan_escape(&mut self, in_class: bool) {
        let offset = self.chr_offset;
        let length: u32;
        let base: u32;
        match self.chr {
            0x30..=0x37 => {
                let mut value: i64 = 0;
                let mut size = 0;
                loop {
                    let digit = digit_value(self.chr);
                    if digit >= 8 {
                        break;
                    }
                    value = value.wrapping_mul(8).wrapping_add(digit);
                    self.read();
                    size += 1;
                }
                if size == 1 {
                    if value != 0 {
                        self.error(
                            false,
                            "re2: Invalid \\%d <backreference>",
                            &[Arg::Int(value)],
                        );
                        return;
                    }
                    self.pass_string(offset - 1, self.chr_offset);
                    return;
                }
                self.write_hex_escape(value);
                return;
            }
            0x38 | 0x39 => {
                self.read();
                let s = self.str[offset..self.chr_offset].to_vec();
                self.error(false, "re2: Invalid \\%s <backreference>", &[Arg::Str(&s)]);
                return;
            }
            0x78 => {
                self.read();
                length = 2;
                base = 16;
            }
            0x75 => {
                self.read();
                if self.chr == Rune::from(b'{') && self.unicode {
                    self.read();
                    length = 0;
                    base = 16;
                } else {
                    length = 4;
                    base = 16;
                }
            }
            0x62 if in_class => {
                self.write(b"\\x08");
                self.read();
                return;
            }
            0x62 | 0x42 | 0x64 | 0x44 | 0x77 | 0x57 | 0x5C | 0x66 | 0x6E | 0x72 | 0x74 | 0x76 => {
                self.pass_string(offset - 1, self.offset);
                self.read();
                return;
            }
            0x63 => {
                self.read();
                let value: i64;
                if (0x61..=0x7A).contains(&self.chr) {
                    value = i64::from(self.chr - 0x61 + 1);
                } else if (0x41..=0x5A).contains(&self.chr) {
                    value = i64::from(self.chr - 0x41 + 1);
                } else {
                    self.write_byte(b'c');
                    return;
                }
                self.write_hex_escape(value);
                self.read();
                return;
            }
            0x73 => {
                if in_class {
                    self.write_string(WHITESPACE_CHARS);
                } else {
                    self.write_string(&format!("[{WHITESPACE_CHARS}]"));
                }
                self.read();
                return;
            }
            0x53 => {
                if in_class {
                    self.error(false, "S in class", &[]);
                    return;
                }
                self.write_string(&format!("[^{WHITESPACE_CHARS}]"));
                self.read();
                return;
            }
            _ => {
                if self.chr == Rune::from(b'$')
                    || self.chr < Rune::from(RUNE_SELF) && !is_identifier_part(self.chr)
                {
                    self.pass_string(offset - 1, self.offset);
                    self.read();
                    return;
                }
                self.pass();
                return;
            }
        }
        let value_offset = self.chr_offset;
        if length > 0 {
            let mut l = length;
            while l > 0 {
                let digit = digit_value(self.chr) as u32;
                if digit >= base {
                    self.pass_string(offset, self.chr_offset);
                    return;
                }
                self.read();
                l -= 1;
            }
        } else {
            while self.chr != Rune::from(b'}') && self.chr != -1 {
                let digit = digit_value(self.chr) as u32;
                if digit >= base {
                    self.error(true, "Invalid Unicode escape", &[]);
                    return;
                }
                self.read();
            }
        }
        if length == 4 || length == 0 {
            self.write(b"\\x{");
            self.pass_string(value_offset, self.chr_offset);
            if length != 0 {
                self.write_byte(b'}');
            }
        } else if length == 2 {
            self.pass_string(offset - 1, value_offset + 2);
        } else {
            self.error(true, "re2: Illegal branch in scanEscape", &[]);
        }
    }

    fn pass(&mut self) {
        if self.pass_offset == Some(self.chr_offset) {
            self.pass_offset = Some(self.offset);
        } else {
            if self.pass_offset.is_some() {
                self.stop_passing();
            }
            if self.chr != -1 {
                utf8::append_rune(&mut self.go_regexp, self.chr);
            }
        }
        self.read();
    }

    fn pass_string(&mut self, start: usize, end: usize) {
        if self.pass_offset == Some(start) {
            self.pass_offset = Some(end);
            return;
        }
        if self.pass_offset.is_some() {
            self.stop_passing();
        }
        self.go_regexp.extend_from_slice(&self.str[start..end]);
    }

    fn error(&mut self, fatal: bool, msg: &str, args: &[Arg<'_>]) {
        if self.err.is_some() {
            return;
        }
        let e = RegexpParseError {
            offset: self.offset,
            err: String::from_utf8_lossy(&sprintf(msg, args)).into_owned(),
        };
        self.err = Some(if fatal {
            TransformError::Syntax(RegexpSyntaxError(e))
        } else {
            TransformError::Incompatible(RegexpErrorIncompatible(e))
        });
        self.offset = self.length;
        self.chr = -1;
    }
}
