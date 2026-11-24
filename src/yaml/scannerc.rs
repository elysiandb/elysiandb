use super::yamlh::{ErrorType, Mark, Parser, ScalarStyle, SimpleKey, Token, TokenType};
use super::yamlprivateh::{
    as_digit, as_hex, is_alpha, is_blank, is_blankz, is_bom, is_break, is_breakz, is_crlf,
    is_digit, is_hex, is_space, is_tab, is_z, width,
};
use std::collections::HashMap;

const MAX_FLOW_LEVEL: usize = 10000;
const MAX_INDENTS: usize = 10000;
const MAX_NUMBER_LENGTH: i8 = 2;

fn token(typ: TokenType, start_mark: Mark, end_mark: Mark) -> Token {
    Token {
        typ,
        start_mark,
        end_mark,
        ..Token::default()
    }
}

impl Parser<'_> {
    fn cache(&mut self, length: usize) -> bool {
        self.unread >= length || self.update_buffer(length)
    }

    fn at(&self, offset: usize) -> u8 {
        self.buffer[self.buffer_pos + offset]
    }

    fn skip(&mut self) {
        self.mark.index += 1;
        self.mark.column += 1;
        self.unread -= 1;
        self.buffer_pos += width(self.buffer[self.buffer_pos]);
    }

    fn skip_line(&mut self) {
        if is_crlf(&self.buffer, self.buffer_pos) {
            self.mark.index += 2;
            self.mark.column = 0;
            self.mark.line += 1;
            self.unread -= 2;
            self.buffer_pos += 2;
        } else if is_break(&self.buffer, self.buffer_pos) {
            self.mark.index += 1;
            self.mark.column = 0;
            self.mark.line += 1;
            self.unread -= 1;
            self.buffer_pos += width(self.buffer[self.buffer_pos]);
        }
    }

    fn read(&mut self, s: &mut Vec<u8>) {
        let w = width(self.buffer[self.buffer_pos]).max(1);
        s.extend_from_slice(&self.buffer[self.buffer_pos..self.buffer_pos + w]);
        self.buffer_pos += w;
        self.mark.index += 1;
        self.mark.column += 1;
        self.unread -= 1;
    }

    fn read_line(&mut self, s: &mut Vec<u8>) {
        let pos = self.buffer_pos;
        let buf = &self.buffer;
        if buf[pos] == b'\r' && buf[pos + 1] == b'\n' {
            s.push(b'\n');
            self.buffer_pos += 2;
            self.mark.index += 1;
            self.unread -= 1;
        } else if buf[pos] == b'\r' || buf[pos] == b'\n' {
            s.push(b'\n');
            self.buffer_pos += 1;
        } else if buf[pos] == 0xC2 && buf[pos + 1] == 0x85 {
            s.push(b'\n');
            self.buffer_pos += 2;
        } else if buf[pos] == 0xE2
            && buf[pos + 1] == 0x80
            && (buf[pos + 2] == 0xA8 || buf[pos + 2] == 0xA9)
        {
            s.extend_from_slice(&buf[pos..pos + 3]);
            self.buffer_pos += 3;
        } else {
            return;
        }
        self.mark.index += 1;
        self.mark.column = 0;
        self.mark.line += 1;
        self.unread -= 1;
    }

    fn set_scanner_error(
        &mut self,
        context: &'static str,
        context_mark: Mark,
        problem: &str,
    ) -> bool {
        self.error = ErrorType::Scanner;
        self.context = context;
        self.context_mark = context_mark;
        self.problem = problem.to_string();
        self.problem_mark = self.mark;
        false
    }

    fn set_scanner_tag_error(
        &mut self,
        directive: bool,
        context_mark: Mark,
        problem: &str,
    ) -> bool {
        let context = if directive {
            "while parsing a %TAG directive"
        } else {
            "while parsing a tag"
        };
        self.set_scanner_error(context, context_mark, problem)
    }

    pub(super) fn fetch_more_tokens(&mut self) -> bool {
        loop {
            if !self.tokens.is_empty() {
                match self.simple_keys_by_tok.get(&self.tokens_parsed).copied() {
                    None => break,
                    Some(idx) => match self.simple_key_is_valid(idx) {
                        None => return false,
                        Some(false) => break,
                        Some(true) => {}
                    },
                }
            }
            if !self.fetch_next_token() {
                return false;
            }
        }
        self.token_available = true;
        true
    }

    fn fetch_next_token(&mut self) -> bool {
        if !self.cache(1) {
            return false;
        }
        if !self.stream_start_produced {
            return self.fetch_stream_start();
        }
        if !self.scan_to_next_token() {
            return false;
        }
        if !self.unroll_indent(self.mark.column as isize) {
            return false;
        }
        if !self.cache(4) {
            return false;
        }
        if is_z(&self.buffer, self.buffer_pos) {
            return self.fetch_stream_end();
        }
        if self.mark.column == 0 && self.at(0) == b'%' {
            return self.fetch_directive();
        }
        let buf = &self.buffer;
        let pos = self.buffer_pos;
        if self.mark.column == 0
            && buf[pos] == b'-'
            && buf[pos + 1] == b'-'
            && buf[pos + 2] == b'-'
            && is_blankz(buf, pos + 3)
        {
            return self.fetch_document_indicator(TokenType::DocumentStart);
        }
        if self.mark.column == 0
            && buf[pos] == b'.'
            && buf[pos + 1] == b'.'
            && buf[pos + 2] == b'.'
            && is_blankz(buf, pos + 3)
        {
            return self.fetch_document_indicator(TokenType::DocumentEnd);
        }
        let c = buf[pos];
        if c == b'[' {
            return self.fetch_flow_collection_start(TokenType::FlowSequenceStart);
        }
        if c == b'{' {
            return self.fetch_flow_collection_start(TokenType::FlowMappingStart);
        }
        if c == b']' {
            return self.fetch_flow_collection_end(TokenType::FlowSequenceEnd);
        }
        if c == b'}' {
            return self.fetch_flow_collection_end(TokenType::FlowMappingEnd);
        }
        if c == b',' {
            return self.fetch_flow_entry();
        }
        if c == b'-' && is_blankz(buf, pos + 1) {
            return self.fetch_block_entry();
        }
        if c == b'?' && (self.flow_level > 0 || is_blankz(buf, pos + 1)) {
            return self.fetch_key();
        }
        if c == b':' && (self.flow_level > 0 || is_blankz(buf, pos + 1)) {
            return self.fetch_value();
        }
        if c == b'*' {
            return self.fetch_anchor(TokenType::Alias);
        }
        if c == b'&' {
            return self.fetch_anchor(TokenType::Anchor);
        }
        if c == b'!' {
            return self.fetch_tag();
        }
        if c == b'|' && self.flow_level == 0 {
            return self.fetch_block_scalar(true);
        }
        if c == b'>' && self.flow_level == 0 {
            return self.fetch_block_scalar(false);
        }
        if c == b'\'' {
            return self.fetch_flow_scalar(true);
        }
        if c == b'"' {
            return self.fetch_flow_scalar(false);
        }
        let indicator = is_blankz(buf, pos)
            || matches!(
                c,
                b'-' | b'?'
                    | b':'
                    | b','
                    | b'['
                    | b']'
                    | b'{'
                    | b'}'
                    | b'#'
                    | b'&'
                    | b'*'
                    | b'!'
                    | b'|'
                    | b'>'
                    | b'\''
                    | b'"'
                    | b'%'
                    | b'@'
                    | b'`'
            );
        if !indicator
            || (c == b'-' && !is_blank(buf, pos + 1))
            || (self.flow_level == 0 && (c == b'?' || c == b':') && !is_blankz(buf, pos + 1))
        {
            return self.fetch_plain_scalar();
        }
        let mark = self.mark;
        self.set_scanner_error(
            "while scanning for the next token",
            mark,
            "found character that cannot start any token",
        )
    }

    fn simple_key_is_valid(&mut self, idx: usize) -> Option<bool> {
        let key = self.simple_keys[idx];
        if !key.possible {
            return Some(false);
        }
        if key.mark.line < self.mark.line || key.mark.index + 1024 < self.mark.index {
            if key.required {
                self.set_scanner_error(
                    "while scanning a simple key",
                    key.mark,
                    "could not find expected ':'",
                );
                return None;
            }
            self.simple_keys[idx].possible = false;
            return Some(false);
        }
        Some(true)
    }

    fn save_simple_key(&mut self) -> bool {
        let required = self.flow_level == 0 && self.indent == self.mark.column as isize;
        if self.simple_key_allowed {
            let key = SimpleKey {
                possible: true,
                required,
                token_number: self.tokens_parsed + self.tokens.len(),
                mark: self.mark,
            };
            if !self.remove_simple_key() {
                return false;
            }
            let last = self.simple_keys.len() - 1;
            self.simple_keys[last] = key;
            self.simple_keys_by_tok.insert(key.token_number, last);
        }
        true
    }

    fn remove_simple_key(&mut self) -> bool {
        let i = self.simple_keys.len() - 1;
        if self.simple_keys[i].possible {
            if self.simple_keys[i].required {
                let mark = self.simple_keys[i].mark;
                return self.set_scanner_error(
                    "while scanning a simple key",
                    mark,
                    "could not find expected ':'",
                );
            }
            self.simple_keys[i].possible = false;
            let number = self.simple_keys[i].token_number;
            self.simple_keys_by_tok.remove(&number);
        }
        true
    }

    fn increase_flow_level(&mut self) -> bool {
        self.simple_keys.push(SimpleKey {
            possible: false,
            required: false,
            token_number: self.tokens_parsed + self.tokens.len(),
            mark: self.mark,
        });
        self.flow_level += 1;
        if self.flow_level > MAX_FLOW_LEVEL {
            let mark = self.simple_keys[self.simple_keys.len() - 1].mark;
            return self.set_scanner_error(
                "while increasing flow level",
                mark,
                &format!("exceeded max depth of {MAX_FLOW_LEVEL}"),
            );
        }
        true
    }

    fn decrease_flow_level(&mut self) -> bool {
        if self.flow_level > 0 {
            self.flow_level -= 1;
            let last = self.simple_keys.len() - 1;
            let number = self.simple_keys[last].token_number;
            self.simple_keys_by_tok.remove(&number);
            self.simple_keys.truncate(last);
        }
        true
    }

    fn roll_indent(&mut self, column: usize, number: isize, typ: TokenType, mark: Mark) -> bool {
        if self.flow_level > 0 {
            return true;
        }
        let column = column as isize;
        if self.indent < column {
            self.indents.push(self.indent);
            self.indent = column;
            if self.indents.len() > MAX_INDENTS {
                let context_mark = self.simple_keys[self.simple_keys.len() - 1].mark;
                return self.set_scanner_error(
                    "while increasing indent level",
                    context_mark,
                    &format!("exceeded max depth of {MAX_INDENTS}"),
                );
            }
            let mut number = number;
            if number > -1 {
                number -= self.tokens_parsed as isize;
            }
            self.insert_token(number, token(typ, mark, mark));
        }
        true
    }

    fn unroll_indent(&mut self, column: isize) -> bool {
        if self.flow_level > 0 {
            return true;
        }
        while self.indent > column {
            let mark = self.mark;
            self.insert_token(-1, token(TokenType::BlockEnd, mark, mark));
            self.indent = self.indents.pop().unwrap_or(-1);
        }
        true
    }

    fn fetch_stream_start(&mut self) -> bool {
        self.indent = -1;
        self.simple_keys.push(SimpleKey::default());
        self.simple_keys_by_tok = HashMap::new();
        self.simple_key_allowed = true;
        self.stream_start_produced = true;
        let mark = self.mark;
        self.insert_token(-1, token(TokenType::StreamStart, mark, mark));
        true
    }

    fn fetch_stream_end(&mut self) -> bool {
        if self.mark.column != 0 {
            self.mark.column = 0;
            self.mark.line += 1;
        }
        if !self.unroll_indent(-1) {
            return false;
        }
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let mark = self.mark;
        self.insert_token(-1, token(TokenType::StreamEnd, mark, mark));
        true
    }

    fn fetch_directive(&mut self) -> bool {
        if !self.unroll_indent(-1) {
            return false;
        }
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let mut tok = Token::default();
        if !self.scan_directive(&mut tok) {
            return false;
        }
        self.insert_token(-1, tok);
        true
    }

    fn fetch_document_indicator(&mut self, typ: TokenType) -> bool {
        if !self.unroll_indent(-1) {
            return false;
        }
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let start_mark = self.mark;
        self.skip();
        self.skip();
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(typ, start_mark, end_mark));
        true
    }

    fn fetch_flow_collection_start(&mut self, typ: TokenType) -> bool {
        if !self.save_simple_key() {
            return false;
        }
        if !self.increase_flow_level() {
            return false;
        }
        self.simple_key_allowed = true;
        let start_mark = self.mark;
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(typ, start_mark, end_mark));
        true
    }

    fn fetch_flow_collection_end(&mut self, typ: TokenType) -> bool {
        if !self.remove_simple_key() {
            return false;
        }
        if !self.decrease_flow_level() {
            return false;
        }
        self.simple_key_allowed = false;
        let start_mark = self.mark;
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(typ, start_mark, end_mark));
        true
    }

    fn fetch_flow_entry(&mut self) -> bool {
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = true;
        let start_mark = self.mark;
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(TokenType::FlowEntry, start_mark, end_mark));
        true
    }

    fn fetch_block_entry(&mut self) -> bool {
        if self.flow_level == 0 {
            if !self.simple_key_allowed {
                let mark = self.mark;
                return self.set_scanner_error(
                    "",
                    mark,
                    "block sequence entries are not allowed in this context",
                );
            }
            let mark = self.mark;
            if !self.roll_indent(mark.column, -1, TokenType::BlockSequenceStart, mark) {
                return false;
            }
        }
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = true;
        let start_mark = self.mark;
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(TokenType::BlockEntry, start_mark, end_mark));
        true
    }

    fn fetch_key(&mut self) -> bool {
        if self.flow_level == 0 {
            if !self.simple_key_allowed {
                let mark = self.mark;
                return self.set_scanner_error(
                    "",
                    mark,
                    "mapping keys are not allowed in this context",
                );
            }
            let mark = self.mark;
            if !self.roll_indent(mark.column, -1, TokenType::BlockMappingStart, mark) {
                return false;
            }
        }
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = self.flow_level == 0;
        let start_mark = self.mark;
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(TokenType::Key, start_mark, end_mark));
        true
    }

    fn fetch_value(&mut self) -> bool {
        let idx = self.simple_keys.len() - 1;
        match self.simple_key_is_valid(idx) {
            None => return false,
            Some(true) => {
                let key = self.simple_keys[idx];
                self.insert_token(
                    key.token_number as isize - self.tokens_parsed as isize,
                    token(TokenType::Key, key.mark, key.mark),
                );
                if !self.roll_indent(
                    key.mark.column,
                    key.token_number as isize,
                    TokenType::BlockMappingStart,
                    key.mark,
                ) {
                    return false;
                }
                self.simple_keys[idx].possible = false;
                self.simple_keys_by_tok.remove(&key.token_number);
                self.simple_key_allowed = false;
            }
            Some(false) => {
                if self.flow_level == 0 {
                    if !self.simple_key_allowed {
                        let mark = self.mark;
                        return self.set_scanner_error(
                            "",
                            mark,
                            "mapping values are not allowed in this context",
                        );
                    }
                    let mark = self.mark;
                    if !self.roll_indent(mark.column, -1, TokenType::BlockMappingStart, mark) {
                        return false;
                    }
                }
                self.simple_key_allowed = self.flow_level == 0;
            }
        }
        let start_mark = self.mark;
        self.skip();
        let end_mark = self.mark;
        self.insert_token(-1, token(TokenType::Value, start_mark, end_mark));
        true
    }

    fn fetch_anchor(&mut self, typ: TokenType) -> bool {
        if !self.save_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let mut tok = Token::default();
        if !self.scan_anchor(&mut tok, typ) {
            return false;
        }
        self.insert_token(-1, tok);
        true
    }

    fn fetch_tag(&mut self) -> bool {
        if !self.save_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let mut tok = Token::default();
        if !self.scan_tag(&mut tok) {
            return false;
        }
        self.insert_token(-1, tok);
        true
    }

    fn fetch_block_scalar(&mut self, literal: bool) -> bool {
        if !self.remove_simple_key() {
            return false;
        }
        self.simple_key_allowed = true;
        let mut tok = Token::default();
        if !self.scan_block_scalar(&mut tok, literal) {
            return false;
        }
        self.insert_token(-1, tok);
        true
    }

    fn fetch_flow_scalar(&mut self, single: bool) -> bool {
        if !self.save_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let mut tok = Token::default();
        if !self.scan_flow_scalar(&mut tok, single) {
            return false;
        }
        self.insert_token(-1, tok);
        true
    }

    fn fetch_plain_scalar(&mut self) -> bool {
        if !self.save_simple_key() {
            return false;
        }
        self.simple_key_allowed = false;
        let mut tok = Token::default();
        if !self.scan_plain_scalar(&mut tok) {
            return false;
        }
        self.insert_token(-1, tok);
        true
    }

    fn scan_to_next_token(&mut self) -> bool {
        loop {
            if !self.cache(1) {
                return false;
            }
            if self.mark.column == 0 && is_bom(&self.buffer) {
                self.skip();
            }
            if !self.cache(1) {
                return false;
            }
            while self.at(0) == b' '
                || ((self.flow_level > 0 || !self.simple_key_allowed) && self.at(0) == b'\t')
            {
                self.skip();
                if !self.cache(1) {
                    return false;
                }
            }
            if self.at(0) == b'#' {
                while !is_breakz(&self.buffer, self.buffer_pos) {
                    self.skip();
                    if !self.cache(1) {
                        return false;
                    }
                }
            }
            if is_break(&self.buffer, self.buffer_pos) {
                if !self.cache(2) {
                    return false;
                }
                self.skip_line();
                if self.flow_level == 0 {
                    self.simple_key_allowed = true;
                }
            } else {
                break;
            }
        }
        true
    }

    fn scan_directive(&mut self, tok: &mut Token) -> bool {
        let start_mark = self.mark;
        self.skip();
        let mut name = Vec::new();
        if !self.scan_directive_name(start_mark, &mut name) {
            return false;
        }
        if name == b"YAML" {
            let mut major = 0;
            let mut minor = 0;
            if !self.scan_version_directive_value(start_mark, &mut major, &mut minor) {
                return false;
            }
            let end_mark = self.mark;
            *tok = Token {
                typ: TokenType::VersionDirective,
                start_mark,
                end_mark,
                major,
                minor,
                ..Token::default()
            };
        } else if name == b"TAG" {
            let mut handle = Vec::new();
            let mut prefix = Vec::new();
            if !self.scan_tag_directive_value(start_mark, &mut handle, &mut prefix) {
                return false;
            }
            let end_mark = self.mark;
            *tok = Token {
                typ: TokenType::TagDirective,
                start_mark,
                end_mark,
                value: handle,
                prefix,
                ..Token::default()
            };
        } else {
            self.set_scanner_error(
                "while scanning a directive",
                start_mark,
                "found unknown directive name",
            );
            return false;
        }
        if !self.cache(1) {
            return false;
        }
        while is_blank(&self.buffer, self.buffer_pos) {
            self.skip();
            if !self.cache(1) {
                return false;
            }
        }
        if self.at(0) == b'#' {
            while !is_breakz(&self.buffer, self.buffer_pos) {
                self.skip();
                if !self.cache(1) {
                    return false;
                }
            }
        }
        if !is_breakz(&self.buffer, self.buffer_pos) {
            self.set_scanner_error(
                "while scanning a directive",
                start_mark,
                "did not find expected comment or line break",
            );
            return false;
        }
        if is_break(&self.buffer, self.buffer_pos) {
            if !self.cache(2) {
                return false;
            }
            self.skip_line();
        }
        true
    }

    fn scan_directive_name(&mut self, start_mark: Mark, name: &mut Vec<u8>) -> bool {
        if !self.cache(1) {
            return false;
        }
        let mut s = Vec::new();
        while is_alpha(&self.buffer, self.buffer_pos) {
            self.read(&mut s);
            if !self.cache(1) {
                return false;
            }
        }
        if s.is_empty() {
            self.set_scanner_error(
                "while scanning a directive",
                start_mark,
                "could not find expected directive name",
            );
            return false;
        }
        if !is_blankz(&self.buffer, self.buffer_pos) {
            self.set_scanner_error(
                "while scanning a directive",
                start_mark,
                "found unexpected non-alphabetical character",
            );
            return false;
        }
        *name = s;
        true
    }

    fn scan_version_directive_value(
        &mut self,
        start_mark: Mark,
        major: &mut i8,
        minor: &mut i8,
    ) -> bool {
        if !self.cache(1) {
            return false;
        }
        while is_blank(&self.buffer, self.buffer_pos) {
            self.skip();
            if !self.cache(1) {
                return false;
            }
        }
        if !self.scan_version_directive_number(start_mark, major) {
            return false;
        }
        if self.at(0) != b'.' {
            return self.set_scanner_error(
                "while scanning a %YAML directive",
                start_mark,
                "did not find expected digit or '.' character",
            );
        }
        self.skip();
        self.scan_version_directive_number(start_mark, minor)
    }

    fn scan_version_directive_number(&mut self, start_mark: Mark, number: &mut i8) -> bool {
        if !self.cache(1) {
            return false;
        }
        let mut value: i8 = 0;
        let mut length: i8 = 0;
        while is_digit(&self.buffer, self.buffer_pos) {
            length += 1;
            if length > MAX_NUMBER_LENGTH {
                return self.set_scanner_error(
                    "while scanning a %YAML directive",
                    start_mark,
                    "found extremely long version number",
                );
            }
            value = value * 10 + as_digit(&self.buffer, self.buffer_pos) as i8;
            self.skip();
            if !self.cache(1) {
                return false;
            }
        }
        if length == 0 {
            return self.set_scanner_error(
                "while scanning a %YAML directive",
                start_mark,
                "did not find expected version number",
            );
        }
        *number = value;
        true
    }

    fn scan_tag_directive_value(
        &mut self,
        start_mark: Mark,
        handle: &mut Vec<u8>,
        prefix: &mut Vec<u8>,
    ) -> bool {
        if !self.cache(1) {
            return false;
        }
        while is_blank(&self.buffer, self.buffer_pos) {
            self.skip();
            if !self.cache(1) {
                return false;
            }
        }
        let mut handle_value = Vec::new();
        if !self.scan_tag_handle(true, start_mark, &mut handle_value) {
            return false;
        }
        if !self.cache(1) {
            return false;
        }
        if !is_blank(&self.buffer, self.buffer_pos) {
            self.set_scanner_error(
                "while scanning a %TAG directive",
                start_mark,
                "did not find expected whitespace",
            );
            return false;
        }
        while is_blank(&self.buffer, self.buffer_pos) {
            self.skip();
            if !self.cache(1) {
                return false;
            }
        }
        let mut prefix_value = Vec::new();
        if !self.scan_tag_uri(true, &[], start_mark, &mut prefix_value) {
            return false;
        }
        if !self.cache(1) {
            return false;
        }
        if !is_blankz(&self.buffer, self.buffer_pos) {
            self.set_scanner_error(
                "while scanning a %TAG directive",
                start_mark,
                "did not find expected whitespace or line break",
            );
            return false;
        }
        *handle = handle_value;
        *prefix = prefix_value;
        true
    }

    fn scan_anchor(&mut self, tok: &mut Token, typ: TokenType) -> bool {
        let mut s = Vec::new();
        let start_mark = self.mark;
        self.skip();
        if !self.cache(1) {
            return false;
        }
        while is_alpha(&self.buffer, self.buffer_pos) {
            self.read(&mut s);
            if !self.cache(1) {
                return false;
            }
        }
        let end_mark = self.mark;
        let c = self.at(0);
        if s.is_empty()
            || !(is_blankz(&self.buffer, self.buffer_pos)
                || matches!(c, b'?' | b':' | b',' | b']' | b'}' | b'%' | b'@' | b'`'))
        {
            let context = if typ == TokenType::Anchor {
                "while scanning an anchor"
            } else {
                "while scanning an alias"
            };
            self.set_scanner_error(
                context,
                start_mark,
                "did not find expected alphabetic or numeric character",
            );
            return false;
        }
        *tok = Token {
            typ,
            start_mark,
            end_mark,
            value: s,
            ..Token::default()
        };
        true
    }

    fn scan_tag(&mut self, tok: &mut Token) -> bool {
        let mut handle = Vec::new();
        let mut suffix = Vec::new();
        let start_mark = self.mark;
        if !self.cache(2) {
            return false;
        }
        if self.at(1) == b'<' {
            self.skip();
            self.skip();
            if !self.scan_tag_uri(false, &[], start_mark, &mut suffix) {
                return false;
            }
            if self.at(0) != b'>' {
                self.set_scanner_error(
                    "while scanning a tag",
                    start_mark,
                    "did not find the expected '>'",
                );
                return false;
            }
            self.skip();
        } else {
            if !self.scan_tag_handle(false, start_mark, &mut handle) {
                return false;
            }
            if handle[0] == b'!' && handle.len() > 1 && handle[handle.len() - 1] == b'!' {
                if !self.scan_tag_uri(false, &[], start_mark, &mut suffix) {
                    return false;
                }
            } else {
                let head = std::mem::take(&mut handle);
                if !self.scan_tag_uri(false, &head, start_mark, &mut suffix) {
                    return false;
                }
                handle = vec![b'!'];
                if suffix.is_empty() {
                    std::mem::swap(&mut handle, &mut suffix);
                }
            }
        }
        if !self.cache(1) {
            return false;
        }
        if !is_blankz(&self.buffer, self.buffer_pos) {
            self.set_scanner_error(
                "while scanning a tag",
                start_mark,
                "did not find expected whitespace or line break",
            );
            return false;
        }
        let end_mark = self.mark;
        *tok = Token {
            typ: TokenType::Tag,
            start_mark,
            end_mark,
            value: handle,
            suffix,
            ..Token::default()
        };
        true
    }

    fn scan_tag_handle(&mut self, directive: bool, start_mark: Mark, handle: &mut Vec<u8>) -> bool {
        if !self.cache(1) {
            return false;
        }
        if self.at(0) != b'!' {
            self.set_scanner_tag_error(directive, start_mark, "did not find expected '!'");
            return false;
        }
        let mut s = Vec::new();
        self.read(&mut s);
        if !self.cache(1) {
            return false;
        }
        while is_alpha(&self.buffer, self.buffer_pos) {
            self.read(&mut s);
            if !self.cache(1) {
                return false;
            }
        }
        if self.at(0) == b'!' {
            self.read(&mut s);
        } else if directive && s != b"!" {
            self.set_scanner_tag_error(directive, start_mark, "did not find expected '!'");
            return false;
        }
        *handle = s;
        true
    }

    fn scan_tag_uri(
        &mut self,
        directive: bool,
        head: &[u8],
        start_mark: Mark,
        uri: &mut Vec<u8>,
    ) -> bool {
        let mut s = Vec::new();
        let mut has_tag = !head.is_empty();
        if head.len() > 1 {
            s.extend_from_slice(&head[1..]);
        }
        if !self.cache(1) {
            return false;
        }
        while is_alpha(&self.buffer, self.buffer_pos)
            || matches!(
                self.at(0),
                b';' | b'/'
                    | b'?'
                    | b':'
                    | b'@'
                    | b'&'
                    | b'='
                    | b'+'
                    | b'$'
                    | b','
                    | b'.'
                    | b'!'
                    | b'~'
                    | b'*'
                    | b'\''
                    | b'('
                    | b')'
                    | b'['
                    | b']'
                    | b'%'
            )
        {
            if self.at(0) == b'%' {
                if !self.scan_uri_escapes(directive, start_mark, &mut s) {
                    return false;
                }
            } else {
                self.read(&mut s);
            }
            if !self.cache(1) {
                return false;
            }
            has_tag = true;
        }
        if !has_tag {
            self.set_scanner_tag_error(directive, start_mark, "did not find expected tag URI");
            return false;
        }
        *uri = s;
        true
    }

    fn scan_uri_escapes(&mut self, directive: bool, start_mark: Mark, s: &mut Vec<u8>) -> bool {
        let mut w: usize = 1024;
        while w > 0 {
            if !self.cache(3) {
                return false;
            }
            if !(self.at(0) == b'%'
                && is_hex(&self.buffer, self.buffer_pos + 1)
                && is_hex(&self.buffer, self.buffer_pos + 2))
            {
                return self.set_scanner_tag_error(
                    directive,
                    start_mark,
                    "did not find URI escaped octet",
                );
            }
            let octet = ((as_hex(&self.buffer, self.buffer_pos + 1) << 4)
                + as_hex(&self.buffer, self.buffer_pos + 2)) as u8;
            if w == 1024 {
                w = width(octet);
                if w == 0 {
                    return self.set_scanner_tag_error(
                        directive,
                        start_mark,
                        "found an incorrect leading UTF-8 octet",
                    );
                }
            } else if octet & 0xC0 != 0x80 {
                return self.set_scanner_tag_error(
                    directive,
                    start_mark,
                    "found an incorrect trailing UTF-8 octet",
                );
            }
            s.push(octet);
            self.skip();
            self.skip();
            self.skip();
            w -= 1;
        }
        true
    }

    fn scan_block_scalar(&mut self, tok: &mut Token, literal: bool) -> bool {
        let start_mark = self.mark;
        self.skip();
        if !self.cache(1) {
            return false;
        }
        let mut chomping: i8 = 0;
        let mut increment: isize = 0;
        if self.at(0) == b'+' || self.at(0) == b'-' {
            chomping = if self.at(0) == b'+' { 1 } else { -1 };
            self.skip();
            if !self.cache(1) {
                return false;
            }
            if is_digit(&self.buffer, self.buffer_pos) {
                if self.at(0) == b'0' {
                    self.set_scanner_error(
                        "while scanning a block scalar",
                        start_mark,
                        "found an indentation indicator equal to 0",
                    );
                    return false;
                }
                increment = as_digit(&self.buffer, self.buffer_pos);
                self.skip();
            }
        } else if is_digit(&self.buffer, self.buffer_pos) {
            if self.at(0) == b'0' {
                self.set_scanner_error(
                    "while scanning a block scalar",
                    start_mark,
                    "found an indentation indicator equal to 0",
                );
                return false;
            }
            increment = as_digit(&self.buffer, self.buffer_pos);
            self.skip();
            if !self.cache(1) {
                return false;
            }
            if self.at(0) == b'+' || self.at(0) == b'-' {
                chomping = if self.at(0) == b'+' { 1 } else { -1 };
                self.skip();
            }
        }
        if !self.cache(1) {
            return false;
        }
        while is_blank(&self.buffer, self.buffer_pos) {
            self.skip();
            if !self.cache(1) {
                return false;
            }
        }
        if self.at(0) == b'#' {
            while !is_breakz(&self.buffer, self.buffer_pos) {
                self.skip();
                if !self.cache(1) {
                    return false;
                }
            }
        }
        if !is_breakz(&self.buffer, self.buffer_pos) {
            self.set_scanner_error(
                "while scanning a block scalar",
                start_mark,
                "did not find expected comment or line break",
            );
            return false;
        }
        if is_break(&self.buffer, self.buffer_pos) {
            if !self.cache(2) {
                return false;
            }
            self.skip_line();
        }
        let mut end_mark = self.mark;
        let mut indent: isize = 0;
        if increment > 0 {
            indent = if self.indent >= 0 {
                self.indent + increment
            } else {
                increment
            };
        }
        let mut s = Vec::new();
        let mut leading_break = Vec::new();
        let mut trailing_breaks = Vec::new();
        if !self.scan_block_scalar_breaks(
            &mut indent,
            &mut trailing_breaks,
            start_mark,
            &mut end_mark,
        ) {
            return false;
        }
        if !self.cache(1) {
            return false;
        }
        let mut leading_blank = false;
        while self.mark.column as isize == indent && !is_z(&self.buffer, self.buffer_pos) {
            let trailing_blank = is_blank(&self.buffer, self.buffer_pos);
            if !literal
                && !leading_blank
                && !trailing_blank
                && !leading_break.is_empty()
                && leading_break[0] == b'\n'
            {
                if trailing_breaks.is_empty() {
                    s.push(b' ');
                }
            } else {
                s.extend_from_slice(&leading_break);
            }
            leading_break.clear();
            s.extend_from_slice(&trailing_breaks);
            trailing_breaks.clear();
            leading_blank = is_blank(&self.buffer, self.buffer_pos);
            while !is_breakz(&self.buffer, self.buffer_pos) {
                self.read(&mut s);
                if !self.cache(1) {
                    return false;
                }
            }
            if !self.cache(2) {
                return false;
            }
            self.read_line(&mut leading_break);
            if !self.scan_block_scalar_breaks(
                &mut indent,
                &mut trailing_breaks,
                start_mark,
                &mut end_mark,
            ) {
                return false;
            }
        }
        if chomping != -1 {
            s.extend_from_slice(&leading_break);
        }
        if chomping == 1 {
            s.extend_from_slice(&trailing_breaks);
        }
        *tok = Token {
            typ: TokenType::Scalar,
            start_mark,
            end_mark,
            value: s,
            style: if literal {
                ScalarStyle::Literal
            } else {
                ScalarStyle::Folded
            },
            ..Token::default()
        };
        true
    }

    fn scan_block_scalar_breaks(
        &mut self,
        indent: &mut isize,
        breaks: &mut Vec<u8>,
        start_mark: Mark,
        end_mark: &mut Mark,
    ) -> bool {
        *end_mark = self.mark;
        let mut max_indent: isize = 0;
        loop {
            if !self.cache(1) {
                return false;
            }
            while (*indent == 0 || (self.mark.column as isize) < *indent)
                && is_space(&self.buffer, self.buffer_pos)
            {
                self.skip();
                if !self.cache(1) {
                    return false;
                }
            }
            if self.mark.column as isize > max_indent {
                max_indent = self.mark.column as isize;
            }
            if (*indent == 0 || (self.mark.column as isize) < *indent)
                && is_tab(&self.buffer, self.buffer_pos)
            {
                return self.set_scanner_error(
                    "while scanning a block scalar",
                    start_mark,
                    "found a tab character where an indentation space is expected",
                );
            }
            if !is_break(&self.buffer, self.buffer_pos) {
                break;
            }
            if !self.cache(2) {
                return false;
            }
            self.read_line(breaks);
            *end_mark = self.mark;
        }
        if *indent == 0 {
            *indent = max_indent;
            if *indent < self.indent + 1 {
                *indent = self.indent + 1;
            }
            if *indent < 1 {
                *indent = 1;
            }
        }
        true
    }

    fn scan_flow_scalar(&mut self, tok: &mut Token, single: bool) -> bool {
        let start_mark = self.mark;
        self.skip();
        let mut s = Vec::new();
        let mut leading_break = Vec::new();
        let mut trailing_breaks = Vec::new();
        let mut whitespaces = Vec::new();
        loop {
            if !self.cache(4) {
                return false;
            }
            if self.mark.column == 0
                && ((self.at(0) == b'-' && self.at(1) == b'-' && self.at(2) == b'-')
                    || (self.at(0) == b'.' && self.at(1) == b'.' && self.at(2) == b'.'))
                && is_blankz(&self.buffer, self.buffer_pos + 3)
            {
                self.set_scanner_error(
                    "while scanning a quoted scalar",
                    start_mark,
                    "found unexpected document indicator",
                );
                return false;
            }
            if is_z(&self.buffer, self.buffer_pos) {
                self.set_scanner_error(
                    "while scanning a quoted scalar",
                    start_mark,
                    "found unexpected end of stream",
                );
                return false;
            }
            let mut leading_blanks = false;
            while !is_blankz(&self.buffer, self.buffer_pos) {
                if single && self.at(0) == b'\'' && self.at(1) == b'\'' {
                    s.push(b'\'');
                    self.skip();
                    self.skip();
                } else if (single && self.at(0) == b'\'') || (!single && self.at(0) == b'"') {
                    break;
                } else if !single
                    && self.at(0) == b'\\'
                    && is_break(&self.buffer, self.buffer_pos + 1)
                {
                    if !self.cache(3) {
                        return false;
                    }
                    self.skip();
                    self.skip_line();
                    leading_blanks = true;
                    break;
                } else if !single && self.at(0) == b'\\' {
                    let mut code_length = 0;
                    match self.at(1) {
                        b'0' => s.push(0),
                        b'a' => s.push(0x07),
                        b'b' => s.push(0x08),
                        b't' | b'\t' => s.push(0x09),
                        b'n' => s.push(0x0A),
                        b'v' => s.push(0x0B),
                        b'f' => s.push(0x0C),
                        b'r' => s.push(0x0D),
                        b'e' => s.push(0x1B),
                        b' ' => s.push(0x20),
                        b'"' => s.push(b'"'),
                        b'\'' => s.push(b'\''),
                        b'\\' => s.push(b'\\'),
                        b'N' => s.extend_from_slice(b"\xC2\x85"),
                        b'_' => s.extend_from_slice(b"\xC2\xA0"),
                        b'L' => s.extend_from_slice(b"\xE2\x80\xA8"),
                        b'P' => s.extend_from_slice(b"\xE2\x80\xA9"),
                        b'x' => code_length = 2,
                        b'u' => code_length = 4,
                        b'U' => code_length = 8,
                        _ => {
                            self.set_scanner_error(
                                "while parsing a quoted scalar",
                                start_mark,
                                "found unknown escape character",
                            );
                            return false;
                        }
                    }
                    self.skip();
                    self.skip();
                    if code_length > 0 {
                        let mut value: u32 = 0;
                        if !self.cache(code_length) {
                            return false;
                        }
                        for k in 0..code_length {
                            if !is_hex(&self.buffer, self.buffer_pos + k) {
                                self.set_scanner_error(
                                    "while parsing a quoted scalar",
                                    start_mark,
                                    "did not find expected hexdecimal number",
                                );
                                return false;
                            }
                            value = (value << 4) + as_hex(&self.buffer, self.buffer_pos + k);
                        }
                        if (0xD800..=0xDFFF).contains(&value) || value > 0x10FFFF {
                            self.set_scanner_error(
                                "while parsing a quoted scalar",
                                start_mark,
                                "found invalid Unicode character escape code",
                            );
                            return false;
                        }
                        if value <= 0x7F {
                            s.push(value as u8);
                        } else if value <= 0x7FF {
                            s.push((0xC0 + (value >> 6)) as u8);
                            s.push((0x80 + (value & 0x3F)) as u8);
                        } else if value <= 0xFFFF {
                            s.push((0xE0 + (value >> 12)) as u8);
                            s.push((0x80 + ((value >> 6) & 0x3F)) as u8);
                            s.push((0x80 + (value & 0x3F)) as u8);
                        } else {
                            s.push((0xF0 + (value >> 18)) as u8);
                            s.push((0x80 + ((value >> 12) & 0x3F)) as u8);
                            s.push((0x80 + ((value >> 6) & 0x3F)) as u8);
                            s.push((0x80 + (value & 0x3F)) as u8);
                        }
                        for _ in 0..code_length {
                            self.skip();
                        }
                    }
                } else {
                    self.read(&mut s);
                }
                if !self.cache(2) {
                    return false;
                }
            }
            if !self.cache(1) {
                return false;
            }
            if single {
                if self.at(0) == b'\'' {
                    break;
                }
            } else if self.at(0) == b'"' {
                break;
            }
            while is_blank(&self.buffer, self.buffer_pos) || is_break(&self.buffer, self.buffer_pos)
            {
                if is_blank(&self.buffer, self.buffer_pos) {
                    if !leading_blanks {
                        self.read(&mut whitespaces);
                    } else {
                        self.skip();
                    }
                } else {
                    if !self.cache(2) {
                        return false;
                    }
                    if !leading_blanks {
                        whitespaces.clear();
                        self.read_line(&mut leading_break);
                        leading_blanks = true;
                    } else {
                        self.read_line(&mut trailing_breaks);
                    }
                }
                if !self.cache(1) {
                    return false;
                }
            }
            if leading_blanks {
                if !leading_break.is_empty() && leading_break[0] == b'\n' {
                    if trailing_breaks.is_empty() {
                        s.push(b' ');
                    } else {
                        s.extend_from_slice(&trailing_breaks);
                    }
                } else {
                    s.extend_from_slice(&leading_break);
                    s.extend_from_slice(&trailing_breaks);
                }
                trailing_breaks.clear();
                leading_break.clear();
            } else {
                s.extend_from_slice(&whitespaces);
                whitespaces.clear();
            }
        }
        self.skip();
        let end_mark = self.mark;
        *tok = Token {
            typ: TokenType::Scalar,
            start_mark,
            end_mark,
            value: s,
            style: if single {
                ScalarStyle::SingleQuoted
            } else {
                ScalarStyle::DoubleQuoted
            },
            ..Token::default()
        };
        true
    }

    fn scan_plain_scalar(&mut self, tok: &mut Token) -> bool {
        let mut s = Vec::new();
        let mut leading_break = Vec::new();
        let mut trailing_breaks = Vec::new();
        let mut whitespaces = Vec::new();
        let mut leading_blanks = false;
        let indent = self.indent + 1;
        let start_mark = self.mark;
        let mut end_mark = self.mark;
        loop {
            if !self.cache(4) {
                return false;
            }
            if self.mark.column == 0
                && ((self.at(0) == b'-' && self.at(1) == b'-' && self.at(2) == b'-')
                    || (self.at(0) == b'.' && self.at(1) == b'.' && self.at(2) == b'.'))
                && is_blankz(&self.buffer, self.buffer_pos + 3)
            {
                break;
            }
            if self.at(0) == b'#' {
                break;
            }
            while !is_blankz(&self.buffer, self.buffer_pos) {
                let c = self.at(0);
                if (c == b':' && is_blankz(&self.buffer, self.buffer_pos + 1))
                    || (self.flow_level > 0 && matches!(c, b',' | b'?' | b'[' | b']' | b'{' | b'}'))
                {
                    break;
                }
                if leading_blanks || !whitespaces.is_empty() {
                    if leading_blanks {
                        if leading_break[0] == b'\n' {
                            if trailing_breaks.is_empty() {
                                s.push(b' ');
                            } else {
                                s.extend_from_slice(&trailing_breaks);
                            }
                        } else {
                            s.extend_from_slice(&leading_break);
                            s.extend_from_slice(&trailing_breaks);
                        }
                        trailing_breaks.clear();
                        leading_break.clear();
                        leading_blanks = false;
                    } else {
                        s.extend_from_slice(&whitespaces);
                        whitespaces.clear();
                    }
                }
                self.read(&mut s);
                end_mark = self.mark;
                if !self.cache(2) {
                    return false;
                }
            }
            if !(is_blank(&self.buffer, self.buffer_pos) || is_break(&self.buffer, self.buffer_pos))
            {
                break;
            }
            if !self.cache(1) {
                return false;
            }
            while is_blank(&self.buffer, self.buffer_pos) || is_break(&self.buffer, self.buffer_pos)
            {
                if is_blank(&self.buffer, self.buffer_pos) {
                    if leading_blanks
                        && (self.mark.column as isize) < indent
                        && is_tab(&self.buffer, self.buffer_pos)
                    {
                        self.set_scanner_error(
                            "while scanning a plain scalar",
                            start_mark,
                            "found a tab character that violates indentation",
                        );
                        return false;
                    }
                    if !leading_blanks {
                        self.read(&mut whitespaces);
                    } else {
                        self.skip();
                    }
                } else {
                    if !self.cache(2) {
                        return false;
                    }
                    if !leading_blanks {
                        whitespaces.clear();
                        self.read_line(&mut leading_break);
                        leading_blanks = true;
                    } else {
                        self.read_line(&mut trailing_breaks);
                    }
                }
                if !self.cache(1) {
                    return false;
                }
            }
            if self.flow_level == 0 && (self.mark.column as isize) < indent {
                break;
            }
        }
        *tok = Token {
            typ: TokenType::Scalar,
            start_mark,
            end_mark,
            value: s,
            style: ScalarStyle::Plain,
            ..Token::default()
        };
        if leading_blanks {
            self.simple_key_allowed = true;
        }
        true
    }
}
