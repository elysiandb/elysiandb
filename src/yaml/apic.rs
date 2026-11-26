use super::yamlh::{Encoding, ErrorType, Mark, Parser, ParserState, Token};
use std::collections::{HashMap, VecDeque};

impl<'a> Parser<'a> {
    pub(super) fn new(input: &'a [u8]) -> Parser<'a> {
        Parser {
            error: ErrorType::None,
            problem: String::new(),
            problem_offset: 0,
            problem_value: 0,
            problem_mark: Mark::default(),
            context: "",
            context_mark: Mark::default(),
            input,
            input_pos: 0,
            eof: false,
            buffer: Vec::new(),
            buffer_pos: 0,
            unread: 0,
            raw_buffer: Vec::new(),
            raw_buffer_pos: 0,
            encoding: Encoding::Any,
            offset: 0,
            mark: Mark::default(),
            stream_start_produced: false,
            stream_end_produced: false,
            flow_level: 0,
            tokens: VecDeque::new(),
            tokens_parsed: 0,
            token_available: false,
            indent: 0,
            indents: Vec::new(),
            simple_key_allowed: false,
            simple_keys: Vec::new(),
            simple_keys_by_tok: HashMap::new(),
            state: ParserState::StreamStart,
            states: Vec::new(),
            marks: Vec::new(),
            tag_directives: Vec::new(),
        }
    }

    pub(super) fn insert_token(&mut self, pos: isize, token: Token) {
        match usize::try_from(pos) {
            Ok(p) => self.tokens.insert(p, token),
            Err(_) => self.tokens.push_back(token),
        }
    }
}
