use super::error::ERR_UNEXPECTED_END_OF_INPUT;
use super::parser::Parser;
use crate::ast::NumberValue;
use crate::deps::big;
use crate::deps::xtext::rangetable;
use crate::file::Idx;
use crate::token::{self, Token};
use crate::unistring;
use gocompat::fmt::{Arg, sprintf};
use gocompat::strconv::{self, ErrorKind};
use gocompat::unicode::{self, RangeTable};
use gocompat::utf8::{self, RUNE_ERROR, RUNE_SELF, Rune};
use gocompat::utf16;
use std::sync::OnceLock;

struct IdTables {
    neg: RangeTable,
    start_pos: RangeTable,
    cont_pos: RangeTable,
}

fn id_tables() -> &'static IdTables {
    static TABLES: OnceLock<IdTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let neg = rangetable::merge(&[&unicode::PATTERN_SYNTAX, &unicode::PATTERN_WHITE_SPACE]);
        let start_pos =
            rangetable::merge(&[&unicode::LETTER, &unicode::NL, &unicode::OTHER_ID_START]);
        let cont_pos = rangetable::merge(&[
            &start_pos,
            &unicode::MN,
            &unicode::MC,
            &unicode::ND,
            &unicode::PC,
            &unicode::OTHER_ID_CONTINUE,
        ]);
        IdTables {
            neg,
            start_pos,
            cont_pos,
        }
    })
}

const RUNE_SELF_R: Rune = RUNE_SELF as Rune;

pub(super) fn is_decimal_digit(chr: Rune) -> bool {
    (Rune::from(b'0')..=Rune::from(b'9')).contains(&chr)
}

pub fn is_identifier(s: &[u8]) -> bool {
    if s.is_empty() {
        return false;
    }
    let (r, size) = utf8::decode_rune(s);
    if !is_identifier_start(r) {
        return false;
    }
    let mut i = size;
    while i < s.len() {
        let (r, n) = utf8::decode_rune(&s[i..]);
        if !is_identifier_part(r) {
            return false;
        }
        i += n;
    }
    true
}

pub(super) fn digit_value(chr: Rune) -> i64 {
    match chr {
        0x30..=0x39 => i64::from(chr - 0x30),
        0x61..=0x66 => i64::from(chr - 0x61 + 10),
        0x41..=0x46 => i64::from(chr - 0x41 + 10),
        _ => 16,
    }
}

fn is_digit(chr: Rune, base: i64) -> bool {
    digit_value(chr) < base
}

fn is_id_start_unicode(r: Rune) -> bool {
    let t = id_tables();
    unicode::is(&t.start_pos, r) && !unicode::is(&t.neg, r)
}

fn is_id_part_unicode(r: Rune) -> bool {
    let t = id_tables();
    unicode::is(&t.cont_pos, r) && !unicode::is(&t.neg, r) || r == 0x200C || r == 0x200D
}

pub(super) fn is_identifier_start(chr: Rune) -> bool {
    chr == Rune::from(b'$')
        || chr == Rune::from(b'_')
        || chr == Rune::from(b'\\')
        || (0x61..=0x7A).contains(&chr)
        || (0x41..=0x5A).contains(&chr)
        || chr >= RUNE_SELF_R && is_id_start_unicode(chr)
}

pub(super) fn is_identifier_part(chr: Rune) -> bool {
    chr == Rune::from(b'$')
        || chr == Rune::from(b'_')
        || chr == Rune::from(b'\\')
        || (0x61..=0x7A).contains(&chr)
        || (0x41..=0x5A).contains(&chr)
        || (0x30..=0x39).contains(&chr)
        || chr >= RUNE_SELF_R && is_id_part_unicode(chr)
}

pub(super) fn is_line_white_space(chr: Rune) -> bool {
    match chr {
        0x09 | 0x0B | 0x0C | 0x20 | 0xA0 | 0xFEFF => true,
        0x0A | 0x0D | 0x2028 | 0x2029 => false,
        0x85 => false,
        _ => unicode::is_space(chr),
    }
}

pub(super) fn is_line_terminator(chr: Rune) -> bool {
    matches!(chr, 0x0A | 0x0D | 0x2028 | 0x2029)
}

fn rune_string(r: Rune) -> String {
    char::from_u32(r as u32)
        .unwrap_or(char::REPLACEMENT_CHARACTER)
        .to_string()
}

#[derive(Clone, Default)]
pub struct ParserState {
    idx: Idx,
    tok: Token,
    literal: Vec<u8>,
    parsed_literal: unistring::String,
    implicit_semicolon: bool,
    insert_semicolon: bool,
    chr: Rune,
    chr_offset: usize,
    offset: usize,
    error_count: usize,
}

pub(super) const QUOTE_NONE: Rune = -1;

impl Parser {
    fn scan_identifier(&mut self) -> (Vec<u8>, unistring::String, bool, String) {
        let offset = self.chr_offset;
        let mut has_escape = false;
        let mut is_unicode = false;
        let mut length = 0usize;
        while is_identifier_part(self.chr) {
            let mut r = self.chr;
            length += 1;
            if r == Rune::from(b'\\') {
                has_escape = true;
                let distance = self.chr_offset - offset;
                self.read();
                if self.chr != Rune::from(b'u') {
                    return (
                        Vec::new(),
                        unistring::String::default(),
                        false,
                        format!(
                            "Invalid identifier escape character: {} ({})",
                            rune_string(self.chr),
                            rune_string(self.chr)
                        ),
                    );
                }
                let mut value: Rune;
                if self.peek_byte() == Rune::from(b'{') {
                    self.read();
                    value = -1;
                    while value <= utf8::MAX_RUNE {
                        self.read();
                        if self.chr == Rune::from(b'}') {
                            break;
                        }
                        let Some(decimal) = hex2decimal(self.chr as u8) else {
                            return (
                                Vec::new(),
                                unistring::String::default(),
                                false,
                                "Invalid Unicode escape sequence".to_string(),
                            );
                        };
                        if value == -1 {
                            value = decimal;
                        } else {
                            value = (value << 4) | decimal;
                        }
                    }
                    if value == -1 {
                        return (
                            Vec::new(),
                            unistring::String::default(),
                            false,
                            "Invalid Unicode escape sequence".to_string(),
                        );
                    }
                } else {
                    value = 0;
                    for _ in 0..4 {
                        self.read();
                        let Some(decimal) = hex2decimal(self.chr as u8) else {
                            return (
                                Vec::new(),
                                unistring::String::default(),
                                false,
                                format!(
                                    "Invalid identifier escape character: {} ({})",
                                    rune_string(self.chr),
                                    rune_string(self.chr)
                                ),
                            );
                        };
                        value = (value << 4) | decimal;
                    }
                }
                let invalid = format!(
                    "Invalid identifier escape value: {} ({})",
                    rune_string(value),
                    rune_string(value)
                );
                if value == Rune::from(b'\\') {
                    return (Vec::new(), unistring::String::default(), false, invalid);
                } else if distance == 0 {
                    if !is_identifier_start(value) {
                        return (Vec::new(), unistring::String::default(), false, invalid);
                    }
                } else if !is_identifier_part(value) {
                    return (Vec::new(), unistring::String::default(), false, invalid);
                }
                r = value;
            }
            if r >= RUNE_SELF_R {
                is_unicode = true;
                if r > 0xFFFF {
                    length += 1;
                }
            }
            self.read();
        }
        let literal = self.str[offset..self.chr_offset].to_vec();
        let parsed = if has_escape || is_unicode {
            let (parsed, err) = parse_string_literal(&literal, length, is_unicode, false);
            if !err.is_empty() {
                return (Vec::new(), unistring::String::default(), false, err);
            }
            parsed
        } else {
            unistring::String::from_bytes(&literal)
        };
        (literal, parsed, has_escape, String::new())
    }

    pub fn mark(&self) -> ParserState {
        ParserState {
            idx: self.idx,
            tok: self.token,
            literal: self.literal.clone(),
            parsed_literal: self.parsed_literal.clone(),
            implicit_semicolon: self.implicit_semicolon,
            insert_semicolon: self.insert_semicolon,
            chr: self.chr,
            chr_offset: self.chr_offset,
            offset: self.offset,
            error_count: self.errors.len(),
        }
    }

    pub fn restore(&mut self, state: &ParserState) {
        self.idx = state.idx;
        self.token = state.tok;
        self.literal = state.literal.clone();
        self.parsed_literal = state.parsed_literal.clone();
        self.implicit_semicolon = state.implicit_semicolon;
        self.insert_semicolon = state.insert_semicolon;
        self.chr = state.chr;
        self.chr_offset = state.chr_offset;
        self.offset = state.offset;
        self.errors.0.truncate(state.error_count);
    }

    pub fn peek(&mut self) -> Token {
        let (implicit_semicolon, insert_semicolon, chr, chr_offset, offset) = (
            self.implicit_semicolon,
            self.insert_semicolon,
            self.chr,
            self.chr_offset,
            self.offset,
        );
        let (tok, _, _, _) = self.scan();
        self.implicit_semicolon = implicit_semicolon;
        self.insert_semicolon = insert_semicolon;
        self.chr = chr;
        self.chr_offset = chr_offset;
        self.offset = offset;
        tok
    }

    pub fn scan(&mut self) -> (Token, Vec<u8>, unistring::String, Idx) {
        self.implicit_semicolon = false;
        loop {
            self.skip_white_space();
            let idx = self.idx_of(self.chr_offset);
            let mut insert_semicolon = false;
            let mut tkn: Token;
            let mut literal: Vec<u8> = Vec::new();
            let mut parsed_literal = unistring::String::default();
            let chr = self.chr;
            if is_identifier_start(chr) {
                let (lit, parsed, has_escape, err) = self.scan_identifier();
                literal = lit;
                parsed_literal = parsed;
                if !err.is_empty() {
                    tkn = token::ILLEGAL;
                } else {
                    if parsed_literal.len() > 1 {
                        let (kw, strict) = token::is_keyword(&parsed_literal.as_bytes());
                        tkn = kw;
                        if has_escape {
                            self.insert_semicolon = true;
                            if tkn == Token(0) || self.is_binding_id(tkn) {
                                tkn = token::IDENTIFIER;
                            } else {
                                tkn = token::ESCAPED_RESERVED_WORD;
                            }
                            return (tkn, literal, parsed_literal, idx);
                        }
                        match tkn {
                            Token(0) => {}
                            token::KEYWORD => {
                                if !strict {
                                    return (tkn, literal, parsed_literal, idx);
                                }
                            }
                            token::BOOLEAN
                            | token::NULL
                            | token::THIS
                            | token::BREAK
                            | token::THROW
                            | token::YIELD
                            | token::RETURN
                            | token::CONTINUE
                            | token::DEBUGGER => {
                                self.insert_semicolon = true;
                                return (tkn, literal, parsed_literal, idx);
                            }
                            token::ASYNC => {
                                if self.skip_white_space_check_line_terminator() {
                                    self.insert_semicolon = true;
                                    tkn = token::IDENTIFIER;
                                }
                                return (tkn, literal, parsed_literal, idx);
                            }
                            _ => return (tkn, literal, parsed_literal, idx),
                        }
                    }
                    self.insert_semicolon = true;
                    return (token::IDENTIFIER, literal, parsed_literal, idx);
                }
            } else if is_decimal_digit(chr) {
                self.insert_semicolon = true;
                let (t, l) = self.scan_numeric_literal(false);
                return (t, l, parsed_literal, idx);
            } else {
                self.read();
                match chr {
                    -1 => {
                        if self.insert_semicolon {
                            self.insert_semicolon = false;
                            self.implicit_semicolon = true;
                        }
                        tkn = token::EOF;
                    }
                    0x0D | 0x0A | 0x2028 | 0x2029 => {
                        self.insert_semicolon = false;
                        self.implicit_semicolon = true;
                        continue;
                    }
                    0x3A => tkn = token::COLON,
                    0x2E => {
                        if digit_value(self.chr) < 10 {
                            insert_semicolon = true;
                            let (t, l) = self.scan_numeric_literal(true);
                            tkn = t;
                            literal = l;
                        } else if self.chr == Rune::from(b'.') {
                            self.read();
                            if self.chr == Rune::from(b'.') {
                                self.read();
                                tkn = token::ELLIPSIS;
                            } else {
                                tkn = token::ILLEGAL;
                            }
                        } else {
                            tkn = token::PERIOD;
                        }
                    }
                    0x2C => tkn = token::COMMA,
                    0x3B => tkn = token::SEMICOLON,
                    0x28 => tkn = token::LEFT_PARENTHESIS,
                    0x29 => {
                        tkn = token::RIGHT_PARENTHESIS;
                        insert_semicolon = true;
                    }
                    0x5B => tkn = token::LEFT_BRACKET,
                    0x5D => {
                        tkn = token::RIGHT_BRACKET;
                        insert_semicolon = true;
                    }
                    0x7B => tkn = token::LEFT_BRACE,
                    0x7D => {
                        tkn = token::RIGHT_BRACE;
                        insert_semicolon = true;
                    }
                    0x2B => {
                        tkn = self.switch3(
                            token::PLUS,
                            token::ADD_ASSIGN,
                            Rune::from(b'+'),
                            token::INCREMENT,
                        );
                        if tkn == token::INCREMENT {
                            insert_semicolon = true;
                        }
                    }
                    0x2D => {
                        tkn = self.switch3(
                            token::MINUS,
                            token::SUBTRACT_ASSIGN,
                            Rune::from(b'-'),
                            token::DECREMENT,
                        );
                        if tkn == token::DECREMENT {
                            insert_semicolon = true;
                        }
                    }
                    0x2A => {
                        if self.chr == Rune::from(b'*') {
                            self.read();
                            tkn = self.switch2(token::EXPONENT, token::EXPONENT_ASSIGN);
                        } else {
                            tkn = self.switch2(token::MULTIPLY, token::MULTIPLY_ASSIGN);
                        }
                    }
                    0x2F => {
                        if self.chr == Rune::from(b'/') {
                            self.skip_single_line_comment();
                            continue;
                        } else if self.chr == Rune::from(b'*') {
                            if self.skip_multi_line_comment() {
                                self.insert_semicolon = false;
                                self.implicit_semicolon = true;
                            }
                            continue;
                        } else {
                            tkn = self.switch2(token::SLASH, token::QUOTIENT_ASSIGN);
                            insert_semicolon = true;
                        }
                    }
                    0x25 => tkn = self.switch2(token::REMAINDER, token::REMAINDER_ASSIGN),
                    0x5E => tkn = self.switch2(token::EXCLUSIVE_OR, token::EXCLUSIVE_OR_ASSIGN),
                    0x3C => {
                        tkn = self.switch4(
                            token::LESS,
                            token::LESS_OR_EQUAL,
                            Rune::from(b'<'),
                            token::SHIFT_LEFT,
                            token::SHIFT_LEFT_ASSIGN,
                        )
                    }
                    0x3E => {
                        tkn = self.switch6(
                            token::GREATER,
                            token::GREATER_OR_EQUAL,
                            Rune::from(b'>'),
                            token::SHIFT_RIGHT,
                            token::SHIFT_RIGHT_ASSIGN,
                            Rune::from(b'>'),
                            token::UNSIGNED_SHIFT_RIGHT,
                            token::UNSIGNED_SHIFT_RIGHT_ASSIGN,
                        )
                    }
                    0x3D => {
                        if self.chr == Rune::from(b'>') {
                            self.read();
                            if self.implicit_semicolon {
                                tkn = token::ILLEGAL;
                            } else {
                                tkn = token::ARROW;
                            }
                        } else {
                            tkn = self.switch2(token::ASSIGN, token::EQUAL);
                            if tkn == token::EQUAL && self.chr == Rune::from(b'=') {
                                self.read();
                                tkn = token::STRICT_EQUAL;
                            }
                        }
                    }
                    0x21 => {
                        tkn = self.switch2(token::NOT, token::NOT_EQUAL);
                        if tkn == token::NOT_EQUAL && self.chr == Rune::from(b'=') {
                            self.read();
                            tkn = token::STRICT_NOT_EQUAL;
                        }
                    }
                    0x26 => {
                        tkn = self.switch4(
                            token::AND,
                            token::AND_ASSIGN,
                            Rune::from(b'&'),
                            token::LOGICAL_AND,
                            token::LOGICAL_AND_ASSIGN,
                        )
                    }
                    0x7C => {
                        tkn = self.switch4(
                            token::OR,
                            token::OR_ASSIGN,
                            Rune::from(b'|'),
                            token::LOGICAL_OR,
                            token::LOGICAL_OR_ASSIGN,
                        )
                    }
                    0x7E => tkn = token::BITWISE_NOT,
                    0x3F => {
                        if self.chr == Rune::from(b'.') && !is_decimal_digit(self.peek_byte()) {
                            self.read();
                            tkn = token::QUESTION_DOT;
                        } else if self.chr == Rune::from(b'?') {
                            self.read();
                            if self.chr == Rune::from(b'=') {
                                self.read();
                                tkn = token::COALESCE_ASSIGN;
                            } else {
                                tkn = token::COALESCE;
                            }
                        } else {
                            tkn = token::QUESTION_MARK;
                        }
                    }
                    0x22 | 0x27 => {
                        insert_semicolon = true;
                        tkn = token::STRING;
                        let (l, p, err) = self.scan_string(self.chr_offset - 1, true);
                        literal = l;
                        parsed_literal = p;
                        if !err.is_empty() {
                            tkn = token::ILLEGAL;
                        }
                    }
                    0x60 => tkn = token::BACKTICK,
                    0x23 => {
                        if self.chr_offset == 1 && self.chr == Rune::from(b'!') {
                            self.skip_single_line_comment();
                            continue;
                        }
                        let (l, p, _, err) = self.scan_identifier();
                        literal = l;
                        parsed_literal = p;
                        if !err.is_empty() || literal.is_empty() {
                            tkn = token::ILLEGAL;
                        } else {
                            self.insert_semicolon = true;
                            return (token::PRIVATE_IDENTIFIER, literal, parsed_literal, idx);
                        }
                    }
                    _ => {
                        self.error_unexpected(idx, chr);
                        tkn = token::ILLEGAL;
                    }
                }
            }
            self.insert_semicolon = insert_semicolon;
            return (tkn, literal, parsed_literal, idx);
        }
    }

    fn switch2(&mut self, tkn0: Token, tkn1: Token) -> Token {
        if self.chr == Rune::from(b'=') {
            self.read();
            return tkn1;
        }
        tkn0
    }

    fn switch3(&mut self, tkn0: Token, tkn1: Token, chr2: Rune, tkn2: Token) -> Token {
        if self.chr == Rune::from(b'=') {
            self.read();
            return tkn1;
        }
        if self.chr == chr2 {
            self.read();
            return tkn2;
        }
        tkn0
    }

    fn switch4(&mut self, tkn0: Token, tkn1: Token, chr2: Rune, tkn2: Token, tkn3: Token) -> Token {
        if self.chr == Rune::from(b'=') {
            self.read();
            return tkn1;
        }
        if self.chr == chr2 {
            self.read();
            if self.chr == Rune::from(b'=') {
                self.read();
                return tkn3;
            }
            return tkn2;
        }
        tkn0
    }

    #[allow(clippy::too_many_arguments)]
    fn switch6(
        &mut self,
        tkn0: Token,
        tkn1: Token,
        chr2: Rune,
        tkn2: Token,
        tkn3: Token,
        chr3: Rune,
        tkn4: Token,
        tkn5: Token,
    ) -> Token {
        if self.chr == Rune::from(b'=') {
            self.read();
            return tkn1;
        }
        if self.chr == chr2 {
            self.read();
            if self.chr == Rune::from(b'=') {
                self.read();
                return tkn3;
            }
            if self.chr == chr3 {
                self.read();
                if self.chr == Rune::from(b'=') {
                    self.read();
                    return tkn5;
                }
                return tkn4;
            }
            return tkn2;
        }
        tkn0
    }

    pub(super) fn peek_byte(&self) -> Rune {
        if self.offset < self.length {
            return Rune::from(self.str[self.offset]);
        }
        -1
    }

    pub fn read(&mut self) {
        if self.offset < self.length {
            self.chr_offset = self.offset;
            let mut chr = Rune::from(self.str[self.offset]);
            let mut width = 1;
            if chr >= RUNE_SELF_R {
                let (c, w) = utf8::decode_rune(&self.str[self.offset..]);
                chr = c;
                width = w;
                if chr == RUNE_ERROR && width == 1 {
                    self.error_offset(self.chr_offset, "Invalid UTF-8 character");
                }
            }
            self.offset += width;
            self.chr = chr;
        } else {
            self.chr_offset = self.length;
            self.chr = -1;
        }
    }

    fn skip_single_line_comment(&mut self) {
        while self.chr != -1 {
            self.read();
            if is_line_terminator(self.chr) {
                return;
            }
        }
    }

    fn skip_multi_line_comment(&mut self) -> bool {
        let mut has_line_terminator = false;
        self.read();
        while self.chr >= 0 {
            let chr = self.chr;
            if chr == 0x0D || chr == 0x0A || chr == 0x2028 || chr == 0x2029 {
                has_line_terminator = true;
                break;
            }
            self.read();
            if chr == Rune::from(b'*') && self.chr == Rune::from(b'/') {
                self.read();
                return has_line_terminator;
            }
        }
        while self.chr >= 0 {
            let chr = self.chr;
            self.read();
            if chr == Rune::from(b'*') && self.chr == Rune::from(b'/') {
                self.read();
                return has_line_terminator;
            }
        }
        self.error_unexpected(0, self.chr);
        has_line_terminator
    }

    fn skip_white_space_check_line_terminator(&mut self) -> bool {
        loop {
            match self.chr {
                0x20 | 0x09 | 0x0C | 0x0B | 0xA0 | 0xFEFF => {
                    self.read();
                    continue;
                }
                0x0D => {
                    if self.peek_byte() == 0x0A {
                        self.read();
                    }
                    return true;
                }
                0x2028 | 0x2029 | 0x0A => return true,
                _ => {}
            }
            if self.chr >= RUNE_SELF_R && unicode::is_space(self.chr) {
                self.read();
                continue;
            }
            break;
        }
        false
    }

    fn skip_white_space(&mut self) {
        loop {
            match self.chr {
                0x20 | 0x09 | 0x0C | 0x0B | 0xA0 | 0xFEFF => {
                    self.read();
                    continue;
                }
                0x0D | 0x2028 | 0x2029 | 0x0A => {
                    if self.chr == 0x0D && self.peek_byte() == 0x0A {
                        self.read();
                    }
                    if self.insert_semicolon {
                        return;
                    }
                    self.read();
                    continue;
                }
                _ => {}
            }
            if self.chr >= RUNE_SELF_R && unicode::is_space(self.chr) {
                self.read();
                continue;
            }
            break;
        }
    }

    fn scan_mantissa(&mut self, base: i64, allow_separator: bool) {
        while digit_value(self.chr) < base || (allow_separator && self.chr == Rune::from(b'_')) {
            let after_underscore = self.chr == Rune::from(b'_');
            self.read();
            if after_underscore && !is_digit(self.chr, base) {
                self.error_offset(
                    self.chr_offset,
                    "Only one underscore is allowed as numeric separator",
                );
            }
        }
    }

    pub(super) fn scan_escape(&mut self, quote: Rune) -> (usize, bool) {
        let mut length: u32 = 0;
        let mut base: u32 = 0;
        let mut chr = self.chr;
        match chr {
            0x30..=0x37 => {
                length = 3;
                base = 8;
            }
            0x61 | 0x62 | 0x66 | 0x6E | 0x72 | 0x74 | 0x76 | 0x5C | 0x22 | 0x27 => {
                self.read();
                return (1, false);
            }
            0x0D => {
                self.read();
                if self.chr == 0x0A {
                    self.read();
                    return (2, false);
                }
                return (1, false);
            }
            0x0A => {
                self.read();
                return (1, false);
            }
            0x2028 | 0x2029 => {
                self.read();
                return (1, true);
            }
            0x78 => {
                self.read();
                length = 2;
                base = 16;
            }
            0x75 => {
                self.read();
                if self.chr == Rune::from(b'{') {
                    self.read();
                    length = 0;
                    base = 16;
                } else {
                    length = 4;
                    base = 16;
                }
            }
            _ => self.read(),
        }
        if base > 0 {
            let mut value: u32 = 0;
            if length > 0 {
                while length > 0 && self.chr != quote && self.chr >= 0 {
                    let digit = digit_value(self.chr) as u32;
                    if digit >= base {
                        break;
                    }
                    value = value.wrapping_mul(base).wrapping_add(digit);
                    self.read();
                    length -= 1;
                }
            } else {
                while self.chr != quote && self.chr >= 0 && value < utf8::MAX_RUNE as u32 {
                    if self.chr == Rune::from(b'}') {
                        self.read();
                        break;
                    }
                    let digit = digit_value(self.chr) as u32;
                    if digit >= base {
                        break;
                    }
                    value = value.wrapping_mul(base).wrapping_add(digit);
                    self.read();
                }
            }
            chr = value as Rune;
        }
        if chr >= RUNE_SELF_R {
            if chr > 0xFFFF {
                return (2, true);
            }
            return (1, true);
        }
        (1, false)
    }

    pub fn scan_string(
        &mut self,
        offset: usize,
        parse: bool,
    ) -> (Vec<u8>, unistring::String, String) {
        let mut quote = Rune::from(self.str[offset]);
        let mut length = 0usize;
        let mut is_unicode = false;
        let mut newline = false;
        while self.chr != quote {
            let chr = self.chr;
            if chr == 0x0A || chr == 0x0D || chr < 0 {
                newline = true;
                break;
            }
            if quote == Rune::from(b'/') && (self.chr == 0x2028 || self.chr == 0x2029) {
                newline = true;
                break;
            }
            self.read();
            if chr == Rune::from(b'\\') {
                if self.chr == 0x0A
                    || self.chr == 0x0D
                    || self.chr == 0x2028
                    || self.chr == 0x2029
                    || self.chr < 0
                {
                    if quote == Rune::from(b'/') {
                        newline = true;
                        break;
                    }
                    self.scan_newline();
                } else {
                    let (l, u) = self.scan_escape(quote);
                    length += l;
                    if u {
                        is_unicode = true;
                    }
                }
                continue;
            } else if chr == Rune::from(b'[') && quote == Rune::from(b'/') {
                quote = QUOTE_NONE;
            } else if chr == Rune::from(b']') && quote == QUOTE_NONE {
                quote = Rune::from(b'/');
            }
            if chr >= RUNE_SELF_R {
                is_unicode = true;
                if chr > 0xFFFF {
                    length += 1;
                }
            }
            length += 1;
        }
        if newline {
            self.scan_newline();
            let mut err_str = "String not terminated";
            if quote == Rune::from(b'/') {
                err_str = "Invalid regular expression: missing /";
                let idx = self.idx_of(offset);
                self.error_idx(idx, err_str);
            }
            return (
                Vec::new(),
                unistring::String::default(),
                err_str.to_string(),
            );
        }
        self.read();
        let literal = self.str[offset..self.chr_offset].to_vec();
        let mut parsed = unistring::String::default();
        let mut err = String::new();
        if parse {
            let (p, e) =
                parse_string_literal(&literal[1..literal.len() - 1], length, is_unicode, false);
            parsed = p;
            err = e;
        }
        (literal, parsed, err)
    }

    fn scan_newline(&mut self) {
        if self.chr == 0x2028 || self.chr == 0x2029 {
            self.read();
            return;
        }
        if self.chr == 0x0D {
            self.read();
            if self.chr != 0x0A {
                return;
            }
        }
        self.read();
    }

    pub fn parse_template_characters(
        &mut self,
    ) -> (Vec<u8>, unistring::String, bool, String, String) {
        let offset = self.chr_offset;
        let mut length = 0usize;
        let mut is_unicode = false;
        let mut has_cr = false;
        let mut finished = false;
        let mut parse_err = String::new();
        let end = loop {
            let chr = self.chr;
            if chr < 0 {
                return (
                    Vec::new(),
                    unistring::String::default(),
                    true,
                    parse_err,
                    ERR_UNEXPECTED_END_OF_INPUT.to_string(),
                );
            }
            self.read();
            if chr == Rune::from(b'`') {
                finished = true;
                break self.chr_offset - 1;
            }
            if chr == Rune::from(b'\\') {
                if self.chr == 0x0A
                    || self.chr == 0x0D
                    || self.chr == 0x2028
                    || self.chr == 0x2029
                    || self.chr < 0
                {
                    if self.chr == 0x0D {
                        has_cr = true;
                    }
                    self.scan_newline();
                } else {
                    if (self.chr == Rune::from(b'8') || self.chr == Rune::from(b'9'))
                        && parse_err.is_empty()
                    {
                        parse_err = "\\8 and \\9 are not allowed in template strings.".to_string();
                    }
                    let (l, u) = self.scan_escape(Rune::from(b'`'));
                    length += l;
                    if u {
                        is_unicode = true;
                    }
                }
                continue;
            }
            if chr == Rune::from(b'$') && self.chr == Rune::from(b'{') {
                self.read();
                break self.chr_offset - 2;
            }
            if chr >= RUNE_SELF_R {
                is_unicode = true;
                if chr > 0xFFFF {
                    length += 1;
                }
            } else if chr == 0x0D {
                has_cr = true;
                if self.chr == 0x0A {
                    length = length.wrapping_sub(1);
                }
            }
            length = length.wrapping_add(1);
        };
        let mut literal = self.str[offset..end].to_vec();
        if has_cr {
            literal = normalise_crlf(&literal);
        }
        let mut parsed = unistring::String::default();
        if parse_err.is_empty() {
            let (p, e) = parse_string_literal(&literal, length, is_unicode, true);
            parsed = p;
            parse_err = e;
        }
        self.insert_semicolon = true;
        (literal, parsed, finished, parse_err, String::new())
    }

    pub(super) fn scan_numeric_literal(&mut self, decimal_point: bool) -> (Token, Vec<u8>) {
        let mut offset = self.chr_offset;
        let tkn = token::NUMBER;
        let mut at_end = false;
        if decimal_point {
            offset -= 1;
            self.scan_mantissa(10, true);
        } else {
            if self.chr == Rune::from(b'0') {
                self.read();
                let mut base = 0;
                match self.chr {
                    0x78 | 0x58 => base = 16,
                    0x6F | 0x4F => base = 8,
                    0x62 | 0x42 => base = 2,
                    0x2E | 0x65 | 0x45 => {}
                    _ => {
                        self.scan_mantissa(8, false);
                        at_end = true;
                    }
                }
                if !at_end && base > 0 {
                    self.read();
                    if !is_digit(self.chr, base) {
                        return (token::ILLEGAL, self.str[offset..self.chr_offset].to_vec());
                    }
                    self.scan_mantissa(base, true);
                    at_end = true;
                }
            } else {
                self.scan_mantissa(10, true);
            }
            if !at_end && self.chr == Rune::from(b'.') {
                self.read();
                self.scan_mantissa(10, true);
            }
        }
        if !at_end && (self.chr == Rune::from(b'e') || self.chr == Rune::from(b'E')) {
            self.read();
            if self.chr == Rune::from(b'-') || self.chr == Rune::from(b'+') {
                self.read();
            }
            if is_decimal_digit(self.chr) {
                self.read();
                self.scan_mantissa(10, true);
            } else {
                return (token::ILLEGAL, self.str[offset..self.chr_offset].to_vec());
            }
        }
        if self.chr == Rune::from(b'n') || self.chr == Rune::from(b'N') {
            self.read();
            return (tkn, self.str[offset..self.chr_offset].to_vec());
        }
        if is_identifier_start(self.chr) || is_decimal_digit(self.chr) {
            return (token::ILLEGAL, self.str[offset..self.chr_offset].to_vec());
        }
        (tkn, self.str[offset..self.chr_offset].to_vec())
    }
}

fn normalise_crlf(s: &[u8]) -> Vec<u8> {
    let mut buf = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        if s[i] == b'\r' {
            buf.push(b'\n');
            if i < s.len() - 1 && s[i + 1] == b'\n' {
                i += 1;
            }
        } else {
            buf.push(s[i]);
        }
        i += 1;
    }
    buf
}

fn hex2decimal(chr: u8) -> Option<Rune> {
    match chr {
        b'0'..=b'9' => Some(Rune::from(chr - b'0')),
        b'a'..=b'f' => Some(Rune::from(chr - b'a' + 10)),
        b'A'..=b'F' => Some(Rune::from(chr - b'A' + 10)),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IllegalNumericLiteral;

impl std::fmt::Display for IllegalNumericLiteral {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Illegal numeric literal")
    }
}

fn hex_step(value: f64, digit: f64) -> f64 {
    #[cfg(target_arch = "aarch64")]
    {
        value.mul_add(16.0, digit)
    }
    #[cfg(not(target_arch = "aarch64"))]
    {
        value * 16.0 + digit
    }
}

pub fn parse_number_literal(literal: &[u8]) -> Result<NumberValue, IllegalNumericLiteral> {
    let parse_int_err = match strconv::parse_int(literal, 0, 64) {
        Ok(v) => return Ok(NumberValue::Int64(v)),
        Err(e) => e,
    };
    match strconv::parse_float(literal, 64) {
        Ok(v) => return Ok(NumberValue::Float64(v)),
        Err(e) if e.kind == ErrorKind::Range => return Ok(NumberValue::Float64(e.value)),
        Err(_) => {}
    }
    if parse_int_err.kind == ErrorKind::Range
        && literal.len() > 2
        && literal[0] == b'0'
        && (literal[1] == b'X' || literal[1] == b'x')
        && literal[literal.len() - 1] != b'n'
    {
        let mut value = 0f64;
        let rest = &literal[2..];
        let mut i = 0;
        while i < rest.len() {
            let (chr, size) = utf8::decode_rune(&rest[i..]);
            let digit = digit_value(chr);
            if digit >= 16 {
                return Err(IllegalNumericLiteral);
            }
            value = hex_step(value, digit as f64);
            i += size;
        }
        return Ok(NumberValue::Float64(value));
    }
    if literal.len() > 1 && literal[literal.len() - 1] == b'n' {
        if literal[0] == b'0' && literal.len() > 2 && is_decimal_digit(Rune::from(literal[1])) {
            return Err(IllegalNumericLiteral);
        }
        return match big::Int::set_string(&literal[..literal.len() - 1], 0) {
            Some(v) => Ok(NumberValue::BigInt(Box::new(v))),
            None => Err(IllegalNumericLiteral),
        };
    }
    Err(IllegalNumericLiteral)
}

fn push_utf16(chars: &mut Vec<u16>, chr: Rune) {
    if chr <= 0xFFFF {
        chars.push(chr as u16);
    } else {
        let (first, second) = utf16::encode_rune(chr);
        chars.push(first as u16);
        chars.push(second as u16);
    }
}

fn quoted(s: &[u8]) -> Arg<'_> {
    Arg::Str(s)
}

pub fn parse_string_literal(
    literal: &[u8],
    length: usize,
    unicode: bool,
    strict: bool,
) -> (unistring::String, String) {
    let mut sb: Vec<u8> = Vec::new();
    let mut chars: Vec<u16> = Vec::new();
    if unicode {
        chars.reserve(length + 1);
        chars.push(unistring::BOM);
    } else {
        sb.reserve(length);
    }
    let mut s = literal;
    let fail = |msg: Vec<u8>| {
        (
            unistring::String::default(),
            String::from_utf8_lossy(&msg).into_owned(),
        )
    };
    while !s.is_empty() {
        let chr = s[0];
        if chr >= RUNE_SELF {
            let (r, size) = utf8::decode_rune(s);
            push_utf16(&mut chars, r);
            s = &s[size..];
            continue;
        }
        if chr != b'\\' {
            if unicode {
                chars.push(u16::from(chr));
            } else {
                sb.push(chr);
            }
            s = &s[1..];
            continue;
        }
        if s.len() <= 1 {
            panic!("len(str) <= 1");
        }
        let chr = s[1];
        let value: Rune;
        if chr >= RUNE_SELF {
            s = &s[1..];
            let (v, size) = utf8::decode_rune(s);
            s = &s[size..];
            if v == 0x2028 || v == 0x2029 {
                continue;
            }
            value = v;
        } else {
            s = &s[2..];
            let chr_s = [chr];
            match chr {
                b'b' => value = 0x08,
                b'f' => value = 0x0C,
                b'n' => value = 0x0A,
                b'r' => value = 0x0D,
                b't' => value = 0x09,
                b'v' => value = 0x0B,
                b'x' | b'u' => {
                    let mut size = 0usize;
                    match chr {
                        b'x' => size = 2,
                        _ => {
                            if s.is_empty() || s[0] != b'{' {
                                size = 4;
                            }
                        }
                    }
                    let mut v: Rune = 0;
                    if size > 0 {
                        if s.len() < size {
                            return fail(sprintf(
                                "invalid escape: \\%s: len(%q) != %d",
                                &[Arg::Str(&chr_s), quoted(s), Arg::Int(size as i64)],
                            ));
                        }
                        for &c in &s[..size] {
                            let Some(decimal) = hex2decimal(c) else {
                                return fail(sprintf(
                                    "invalid escape: \\%s: %q",
                                    &[Arg::Str(&chr_s), quoted(&s[..size])],
                                ));
                            };
                            v = (v << 4) | decimal;
                        }
                    } else {
                        s = &s[1..];
                        let mut val: Rune = 0;
                        v = -1;
                        while size < s.len() {
                            if s[size] == b'}' {
                                if size == 0 {
                                    return fail(sprintf(
                                        "invalid escape: \\%s",
                                        &[Arg::Str(&chr_s)],
                                    ));
                                }
                                size += 1;
                                v = val;
                                break;
                            }
                            let Some(decimal) = hex2decimal(s[size]) else {
                                return fail(sprintf(
                                    "invalid escape: \\%s: %q",
                                    &[Arg::Str(&chr_s), quoted(&s[..size + 1])],
                                ));
                            };
                            val = (val << 4) | decimal;
                            if val > utf8::MAX_RUNE {
                                return fail(sprintf(
                                    "undefined Unicode code-point: %q",
                                    &[quoted(&s[..size + 1])],
                                ));
                            }
                            size += 1;
                        }
                        if v == -1 {
                            return fail(sprintf("unterminated \\u{: %q", &[quoted(s)]));
                        }
                    }
                    s = &s[size..];
                    if chr != b'x' && v > utf8::MAX_RUNE {
                        panic!("value > utf8.MaxRune");
                    }
                    value = v;
                }
                b'0' if s.is_empty() || s[0] < b'0' || s[0] > b'7' => value = 0,
                b'0'..=b'7' => {
                    if strict {
                        return (
                            unistring::String::default(),
                            "Octal escape sequences are not allowed in this context".to_string(),
                        );
                    }
                    let mut v = Rune::from(chr - b'0');
                    let mut j = 0;
                    while j < 2 {
                        if s.len() < j + 1 {
                            break;
                        }
                        let c = s[j];
                        if !(b'0'..=b'7').contains(&c) {
                            break;
                        }
                        v = (v << 3) | Rune::from(c - b'0');
                        j += 1;
                    }
                    s = &s[j..];
                    value = v;
                }
                b'\\' => value = Rune::from(b'\\'),
                b'\'' | b'"' => value = Rune::from(chr),
                b'\r' | b'\n' => {
                    if chr == b'\r' && !s.is_empty() && s[0] == b'\n' {
                        s = &s[1..];
                    }
                    continue;
                }
                _ => value = Rune::from(chr),
            }
        }
        if unicode {
            push_utf16(&mut chars, value);
        } else {
            if value >= RUNE_SELF_R {
                return (
                    unistring::String::default(),
                    "Unexpected unicode character".to_string(),
                );
            }
            sb.push(value as u8);
        }
    }
    if unicode {
        if chars.len() != length + 1 {
            panic!(
                "unexpected unicode length while parsing '{}'",
                String::from_utf8_lossy(literal)
            );
        }
        return (unistring::from_utf16(&chars), String::new());
    }
    if sb.len() != length {
        panic!(
            "unexpected length while parsing '{}'",
            String::from_utf8_lossy(literal)
        );
    }
    (unistring::String::from_bytes(&sb), String::new())
}
