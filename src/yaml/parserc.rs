use super::yamlh::{
    ErrorType, Event, EventType, Mark, Parser, ParserState, ScalarStyle, TagDirective, Token,
    TokenType,
};

fn event(typ: EventType, start_mark: Mark) -> Event {
    Event {
        typ,
        start_mark,
        ..Event::default()
    }
}

fn default_tag_directives() -> [TagDirective; 2] {
    [
        TagDirective {
            handle: b"!".to_vec(),
            prefix: b"!".to_vec(),
        },
        TagDirective {
            handle: b"!!".to_vec(),
            prefix: b"tag:yaml.org,2002:".to_vec(),
        },
    ]
}

impl Parser<'_> {
    fn peek_token(&mut self) -> Option<Token> {
        if self.token_available || self.fetch_more_tokens() {
            return self.tokens.front().cloned();
        }
        None
    }

    fn skip_token(&mut self) {
        self.token_available = false;
        self.tokens_parsed += 1;
        if let Some(front) = self.tokens.pop_front() {
            self.stream_end_produced = front.typ == TokenType::StreamEnd;
        }
    }

    pub(super) fn parse(&mut self) -> Option<Event> {
        if self.stream_end_produced
            || self.error != ErrorType::None
            || self.state == ParserState::End
        {
            return Some(Event::default());
        }
        self.state_machine()
    }

    fn set_parser_error(&mut self, problem: &str, problem_mark: Mark) -> Option<Event> {
        self.error = ErrorType::Parser;
        self.problem = problem.to_string();
        self.problem_mark = problem_mark;
        None
    }

    fn set_parser_error_context(
        &mut self,
        context: &'static str,
        context_mark: Mark,
        problem: &str,
        problem_mark: Mark,
    ) -> Option<Event> {
        self.error = ErrorType::Parser;
        self.context = context;
        self.context_mark = context_mark;
        self.problem = problem.to_string();
        self.problem_mark = problem_mark;
        None
    }

    fn pop_state(&mut self) {
        if let Some(state) = self.states.pop() {
            self.state = state;
        }
    }

    fn state_machine(&mut self) -> Option<Event> {
        match self.state {
            ParserState::StreamStart => self.parse_stream_start(),
            ParserState::ImplicitDocumentStart => self.parse_document_start(true),
            ParserState::DocumentStart => self.parse_document_start(false),
            ParserState::DocumentContent => self.parse_document_content(),
            ParserState::DocumentEnd => self.parse_document_end(),
            ParserState::BlockNode => self.parse_node(true, false),
            ParserState::BlockSequenceFirstEntry => self.parse_block_sequence_entry(true),
            ParserState::BlockSequenceEntry => self.parse_block_sequence_entry(false),
            ParserState::IndentlessSequenceEntry => self.parse_indentless_sequence_entry(),
            ParserState::BlockMappingFirstKey => self.parse_block_mapping_key(true),
            ParserState::BlockMappingKey => self.parse_block_mapping_key(false),
            ParserState::BlockMappingValue => self.parse_block_mapping_value(),
            ParserState::FlowSequenceFirstEntry => self.parse_flow_sequence_entry(true),
            ParserState::FlowSequenceEntry => self.parse_flow_sequence_entry(false),
            ParserState::FlowSequenceEntryMappingKey => {
                self.parse_flow_sequence_entry_mapping_key()
            }
            ParserState::FlowSequenceEntryMappingValue => {
                self.parse_flow_sequence_entry_mapping_value()
            }
            ParserState::FlowSequenceEntryMappingEnd => {
                self.parse_flow_sequence_entry_mapping_end()
            }
            ParserState::FlowMappingFirstKey => self.parse_flow_mapping_key(true),
            ParserState::FlowMappingKey => self.parse_flow_mapping_key(false),
            ParserState::FlowMappingValue => self.parse_flow_mapping_value(false),
            ParserState::FlowMappingEmptyValue => self.parse_flow_mapping_value(true),
            ParserState::End => Some(Event::default()),
        }
    }

    fn parse_stream_start(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        if token.typ != TokenType::StreamStart {
            return self.set_parser_error("did not find expected <stream-start>", token.start_mark);
        }
        self.state = ParserState::ImplicitDocumentStart;
        self.skip_token();
        Some(event(EventType::StreamStart, token.start_mark))
    }

    fn parse_document_start(&mut self, implicit: bool) -> Option<Event> {
        let mut token = self.peek_token()?;
        if !implicit {
            while token.typ == TokenType::DocumentEnd {
                self.skip_token();
                token = self.peek_token()?;
            }
        }
        if implicit
            && token.typ != TokenType::VersionDirective
            && token.typ != TokenType::TagDirective
            && token.typ != TokenType::DocumentStart
            && token.typ != TokenType::StreamEnd
        {
            if !self.process_directives() {
                return None;
            }
            self.states.push(ParserState::DocumentEnd);
            self.state = ParserState::BlockNode;
            return Some(event(EventType::DocumentStart, token.start_mark));
        }
        if token.typ != TokenType::StreamEnd {
            let start_mark = token.start_mark;
            if !self.process_directives() {
                return None;
            }
            let token = self.peek_token()?;
            if token.typ != TokenType::DocumentStart {
                return self
                    .set_parser_error("did not find expected <document start>", token.start_mark);
            }
            self.states.push(ParserState::DocumentEnd);
            self.state = ParserState::DocumentContent;
            self.skip_token();
            return Some(event(EventType::DocumentStart, start_mark));
        }
        self.state = ParserState::End;
        self.skip_token();
        Some(event(EventType::StreamEnd, token.start_mark))
    }

    fn parse_document_content(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        if matches!(
            token.typ,
            TokenType::VersionDirective
                | TokenType::TagDirective
                | TokenType::DocumentStart
                | TokenType::DocumentEnd
                | TokenType::StreamEnd
        ) {
            self.pop_state();
            return Some(empty_scalar(token.start_mark));
        }
        self.parse_node(true, false)
    }

    fn parse_document_end(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        let start_mark = token.start_mark;
        if token.typ == TokenType::DocumentEnd {
            self.skip_token();
        }
        self.tag_directives.clear();
        self.state = ParserState::DocumentStart;
        Some(event(EventType::DocumentEnd, start_mark))
    }

    fn parse_node(&mut self, block: bool, indentless_sequence: bool) -> Option<Event> {
        let mut token = self.peek_token()?;
        if token.typ == TokenType::Alias {
            self.pop_state();
            self.skip_token();
            let mut e = event(EventType::Alias, token.start_mark);
            e.anchor = token.value;
            return Some(e);
        }
        let mut start_mark = token.start_mark;
        let mut tag_token = false;
        let mut tag_handle = Vec::new();
        let mut tag_suffix = Vec::new();
        let mut anchor = Vec::new();
        let mut tag_mark = Mark::default();
        if token.typ == TokenType::Anchor {
            anchor = token.value.clone();
            start_mark = token.start_mark;
            self.skip_token();
            token = self.peek_token()?;
            if token.typ == TokenType::Tag {
                tag_token = true;
                tag_handle = token.value.clone();
                tag_suffix = token.suffix.clone();
                tag_mark = token.start_mark;
                self.skip_token();
                token = self.peek_token()?;
            }
        } else if token.typ == TokenType::Tag {
            tag_token = true;
            tag_handle = token.value.clone();
            tag_suffix = token.suffix.clone();
            start_mark = token.start_mark;
            tag_mark = token.start_mark;
            self.skip_token();
            token = self.peek_token()?;
            if token.typ == TokenType::Anchor {
                anchor = token.value.clone();
                self.skip_token();
                token = self.peek_token()?;
            }
        }
        let mut tag = Vec::new();
        if tag_token {
            if tag_handle.is_empty() {
                tag = tag_suffix;
            } else {
                for directive in &self.tag_directives {
                    if directive.handle == tag_handle {
                        tag = directive.prefix.clone();
                        tag.extend_from_slice(&tag_suffix);
                        break;
                    }
                }
                if tag.is_empty() {
                    return self.set_parser_error_context(
                        "while parsing a node",
                        start_mark,
                        "found undefined tag handle",
                        tag_mark,
                    );
                }
            }
        }
        let implicit = tag.is_empty();
        if indentless_sequence && token.typ == TokenType::BlockEntry {
            self.state = ParserState::IndentlessSequenceEntry;
            return Some(collection(
                EventType::SequenceStart,
                start_mark,
                anchor,
                tag,
                implicit,
            ));
        }
        if token.typ == TokenType::Scalar {
            let plain_implicit = (tag.is_empty() && token.style == ScalarStyle::Plain)
                || (tag.len() == 1 && tag[0] == b'!');
            self.pop_state();
            self.skip_token();
            let mut e = collection(EventType::Scalar, start_mark, anchor, tag, plain_implicit);
            e.value = token.value;
            return Some(e);
        }
        if token.typ == TokenType::FlowSequenceStart {
            self.state = ParserState::FlowSequenceFirstEntry;
            return Some(collection(
                EventType::SequenceStart,
                start_mark,
                anchor,
                tag,
                implicit,
            ));
        }
        if token.typ == TokenType::FlowMappingStart {
            self.state = ParserState::FlowMappingFirstKey;
            return Some(collection(
                EventType::MappingStart,
                start_mark,
                anchor,
                tag,
                implicit,
            ));
        }
        if block && token.typ == TokenType::BlockSequenceStart {
            self.state = ParserState::BlockSequenceFirstEntry;
            return Some(collection(
                EventType::SequenceStart,
                start_mark,
                anchor,
                tag,
                implicit,
            ));
        }
        if block && token.typ == TokenType::BlockMappingStart {
            self.state = ParserState::BlockMappingFirstKey;
            return Some(collection(
                EventType::MappingStart,
                start_mark,
                anchor,
                tag,
                implicit,
            ));
        }
        if !anchor.is_empty() || !tag.is_empty() {
            self.pop_state();
            return Some(collection(
                EventType::Scalar,
                start_mark,
                anchor,
                tag,
                implicit,
            ));
        }
        let context = if block {
            "while parsing a block node"
        } else {
            "while parsing a flow node"
        };
        self.set_parser_error_context(
            context,
            start_mark,
            "did not find expected node content",
            token.start_mark,
        )
    }

    fn parse_block_sequence_entry(&mut self, first: bool) -> Option<Event> {
        if first {
            let token = self.peek_token()?;
            self.marks.push(token.start_mark);
            self.skip_token();
        }
        let token = self.peek_token()?;
        if token.typ == TokenType::BlockEntry {
            let mark = token.end_mark;
            self.skip_token();
            let token = self.peek_token()?;
            if token.typ != TokenType::BlockEntry && token.typ != TokenType::BlockEnd {
                self.states.push(ParserState::BlockSequenceEntry);
                return self.parse_node(true, false);
            }
            self.state = ParserState::BlockSequenceEntry;
            return Some(empty_scalar(mark));
        }
        if token.typ == TokenType::BlockEnd {
            self.pop_state();
            self.marks.pop();
            self.skip_token();
            return Some(event(EventType::SequenceEnd, token.start_mark));
        }
        let context_mark = self.marks.pop().unwrap_or_default();
        self.set_parser_error_context(
            "while parsing a block collection",
            context_mark,
            "did not find expected '-' indicator",
            token.start_mark,
        )
    }

    fn parse_indentless_sequence_entry(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        if token.typ == TokenType::BlockEntry {
            let mark = token.end_mark;
            self.skip_token();
            let token = self.peek_token()?;
            if !matches!(
                token.typ,
                TokenType::BlockEntry | TokenType::Key | TokenType::Value | TokenType::BlockEnd
            ) {
                self.states.push(ParserState::IndentlessSequenceEntry);
                return self.parse_node(true, false);
            }
            self.state = ParserState::IndentlessSequenceEntry;
            return Some(empty_scalar(mark));
        }
        self.pop_state();
        Some(event(EventType::SequenceEnd, token.start_mark))
    }

    fn parse_block_mapping_key(&mut self, first: bool) -> Option<Event> {
        if first {
            let token = self.peek_token()?;
            self.marks.push(token.start_mark);
            self.skip_token();
        }
        let token = self.peek_token()?;
        if token.typ == TokenType::Key {
            let mark = token.end_mark;
            self.skip_token();
            let token = self.peek_token()?;
            if !matches!(
                token.typ,
                TokenType::Key | TokenType::Value | TokenType::BlockEnd
            ) {
                self.states.push(ParserState::BlockMappingValue);
                return self.parse_node(true, true);
            }
            self.state = ParserState::BlockMappingValue;
            return Some(empty_scalar(mark));
        }
        if token.typ == TokenType::BlockEnd {
            self.pop_state();
            self.marks.pop();
            self.skip_token();
            return Some(event(EventType::MappingEnd, token.start_mark));
        }
        let context_mark = self.marks.pop().unwrap_or_default();
        self.set_parser_error_context(
            "while parsing a block mapping",
            context_mark,
            "did not find expected key",
            token.start_mark,
        )
    }

    fn parse_block_mapping_value(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        if token.typ == TokenType::Value {
            let mark = token.end_mark;
            self.skip_token();
            let token = self.peek_token()?;
            if !matches!(
                token.typ,
                TokenType::Key | TokenType::Value | TokenType::BlockEnd
            ) {
                self.states.push(ParserState::BlockMappingKey);
                return self.parse_node(true, true);
            }
            self.state = ParserState::BlockMappingKey;
            return Some(empty_scalar(mark));
        }
        self.state = ParserState::BlockMappingKey;
        Some(empty_scalar(token.start_mark))
    }

    fn parse_flow_sequence_entry(&mut self, first: bool) -> Option<Event> {
        if first {
            let token = self.peek_token()?;
            self.marks.push(token.start_mark);
            self.skip_token();
        }
        let mut token = self.peek_token()?;
        if token.typ != TokenType::FlowSequenceEnd {
            if !first {
                if token.typ == TokenType::FlowEntry {
                    self.skip_token();
                    token = self.peek_token()?;
                } else {
                    let context_mark = self.marks.pop().unwrap_or_default();
                    return self.set_parser_error_context(
                        "while parsing a flow sequence",
                        context_mark,
                        "did not find expected ',' or ']'",
                        token.start_mark,
                    );
                }
            }
            if token.typ == TokenType::Key {
                self.state = ParserState::FlowSequenceEntryMappingKey;
                self.skip_token();
                let mut e = event(EventType::MappingStart, token.start_mark);
                e.implicit = true;
                return Some(e);
            } else if token.typ != TokenType::FlowSequenceEnd {
                self.states.push(ParserState::FlowSequenceEntry);
                return self.parse_node(false, false);
            }
        }
        self.pop_state();
        self.marks.pop();
        self.skip_token();
        Some(event(EventType::SequenceEnd, token.start_mark))
    }

    fn parse_flow_sequence_entry_mapping_key(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        if !matches!(
            token.typ,
            TokenType::Value | TokenType::FlowEntry | TokenType::FlowSequenceEnd
        ) {
            self.states.push(ParserState::FlowSequenceEntryMappingValue);
            return self.parse_node(false, false);
        }
        let mark = token.end_mark;
        self.skip_token();
        self.state = ParserState::FlowSequenceEntryMappingValue;
        Some(empty_scalar(mark))
    }

    fn parse_flow_sequence_entry_mapping_value(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        if token.typ == TokenType::Value {
            self.skip_token();
            let token = self.peek_token()?;
            if token.typ != TokenType::FlowEntry && token.typ != TokenType::FlowSequenceEnd {
                self.states.push(ParserState::FlowSequenceEntryMappingEnd);
                return self.parse_node(false, false);
            }
        }
        self.state = ParserState::FlowSequenceEntryMappingEnd;
        Some(empty_scalar(token.start_mark))
    }

    fn parse_flow_sequence_entry_mapping_end(&mut self) -> Option<Event> {
        let token = self.peek_token()?;
        self.state = ParserState::FlowSequenceEntry;
        Some(event(EventType::MappingEnd, token.start_mark))
    }

    fn parse_flow_mapping_key(&mut self, first: bool) -> Option<Event> {
        if first {
            let token = self.peek_token()?;
            self.marks.push(token.start_mark);
            self.skip_token();
        }
        let mut token = self.peek_token()?;
        if token.typ != TokenType::FlowMappingEnd {
            if !first {
                if token.typ == TokenType::FlowEntry {
                    self.skip_token();
                    token = self.peek_token()?;
                } else {
                    let context_mark = self.marks.pop().unwrap_or_default();
                    return self.set_parser_error_context(
                        "while parsing a flow mapping",
                        context_mark,
                        "did not find expected ',' or '}'",
                        token.start_mark,
                    );
                }
            }
            if token.typ == TokenType::Key {
                self.skip_token();
                let token = self.peek_token()?;
                if !matches!(
                    token.typ,
                    TokenType::Value | TokenType::FlowEntry | TokenType::FlowMappingEnd
                ) {
                    self.states.push(ParserState::FlowMappingValue);
                    return self.parse_node(false, false);
                }
                self.state = ParserState::FlowMappingValue;
                return Some(empty_scalar(token.start_mark));
            } else if token.typ != TokenType::FlowMappingEnd {
                self.states.push(ParserState::FlowMappingEmptyValue);
                return self.parse_node(false, false);
            }
        }
        self.pop_state();
        self.marks.pop();
        self.skip_token();
        Some(event(EventType::MappingEnd, token.start_mark))
    }

    fn parse_flow_mapping_value(&mut self, empty: bool) -> Option<Event> {
        let token = self.peek_token()?;
        if empty {
            self.state = ParserState::FlowMappingKey;
            return Some(empty_scalar(token.start_mark));
        }
        if token.typ == TokenType::Value {
            self.skip_token();
            let token = self.peek_token()?;
            if token.typ != TokenType::FlowEntry && token.typ != TokenType::FlowMappingEnd {
                self.states.push(ParserState::FlowMappingKey);
                return self.parse_node(false, false);
            }
            self.state = ParserState::FlowMappingKey;
            return Some(empty_scalar(token.start_mark));
        }
        self.state = ParserState::FlowMappingKey;
        Some(empty_scalar(token.start_mark))
    }

    fn process_directives(&mut self) -> bool {
        let mut has_version = false;
        let mut token = match self.peek_token() {
            Some(t) => t,
            None => return false,
        };
        while token.typ == TokenType::VersionDirective || token.typ == TokenType::TagDirective {
            if token.typ == TokenType::VersionDirective {
                if has_version {
                    self.set_parser_error("found duplicate %YAML directive", token.start_mark);
                    return false;
                }
                if token.major != 1 || token.minor != 1 {
                    self.set_parser_error("found incompatible YAML document", token.start_mark);
                    return false;
                }
                has_version = true;
            } else {
                let value = TagDirective {
                    handle: token.value.clone(),
                    prefix: token.prefix.clone(),
                };
                if !self.append_tag_directive(value, false, token.start_mark) {
                    return false;
                }
            }
            self.skip_token();
            token = match self.peek_token() {
                Some(t) => t,
                None => return false,
            };
        }
        for directive in default_tag_directives() {
            if !self.append_tag_directive(directive, true, token.start_mark) {
                return false;
            }
        }
        true
    }

    fn append_tag_directive(
        &mut self,
        value: TagDirective,
        allow_duplicates: bool,
        mark: Mark,
    ) -> bool {
        if self.tag_directives.iter().any(|d| d.handle == value.handle) {
            if allow_duplicates {
                return true;
            }
            self.set_parser_error("found duplicate %TAG directive", mark);
            return false;
        }
        self.tag_directives.push(value);
        true
    }
}

fn empty_scalar(mark: Mark) -> Event {
    let mut e = event(EventType::Scalar, mark);
    e.implicit = true;
    e
}

fn collection(
    typ: EventType,
    start_mark: Mark,
    anchor: Vec<u8>,
    tag: Vec<u8>,
    implicit: bool,
) -> Event {
    Event {
        typ,
        start_mark,
        anchor,
        tag,
        value: Vec::new(),
        implicit,
    }
}
