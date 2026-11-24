use std::collections::{HashMap, VecDeque};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Mark {
    pub index: usize,
    pub line: usize,
    pub column: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Encoding {
    Any,
    Utf8,
    Utf16Le,
    Utf16Be,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ErrorType {
    None,
    Reader,
    Scanner,
    Parser,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum ScalarStyle {
    #[default]
    Any,
    Plain,
    SingleQuoted,
    DoubleQuoted,
    Literal,
    Folded,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum TokenType {
    #[default]
    NoToken,
    StreamStart,
    StreamEnd,
    VersionDirective,
    TagDirective,
    DocumentStart,
    DocumentEnd,
    BlockSequenceStart,
    BlockMappingStart,
    BlockEnd,
    FlowSequenceStart,
    FlowSequenceEnd,
    FlowMappingStart,
    FlowMappingEnd,
    BlockEntry,
    FlowEntry,
    Key,
    Value,
    Alias,
    Anchor,
    Tag,
    Scalar,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Token {
    pub(super) typ: TokenType,
    pub(super) start_mark: Mark,
    pub(super) end_mark: Mark,
    pub(super) value: Vec<u8>,
    pub(super) suffix: Vec<u8>,
    pub(super) prefix: Vec<u8>,
    pub(super) style: ScalarStyle,
    pub(super) major: i8,
    pub(super) minor: i8,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) enum EventType {
    #[default]
    NoEvent,
    StreamStart,
    StreamEnd,
    DocumentStart,
    DocumentEnd,
    Alias,
    Scalar,
    SequenceStart,
    SequenceEnd,
    MappingStart,
    MappingEnd,
}

#[derive(Clone, Debug, Default)]
pub(super) struct Event {
    pub(super) typ: EventType,
    pub(super) start_mark: Mark,
    pub(super) anchor: Vec<u8>,
    pub(super) tag: Vec<u8>,
    pub(super) value: Vec<u8>,
    pub(super) implicit: bool,
}

pub(super) const NULL_TAG: &str = "tag:yaml.org,2002:null";
pub(super) const BOOL_TAG: &str = "tag:yaml.org,2002:bool";
pub(super) const STR_TAG: &str = "tag:yaml.org,2002:str";
pub(super) const INT_TAG: &str = "tag:yaml.org,2002:int";
pub(super) const FLOAT_TAG: &str = "tag:yaml.org,2002:float";
pub(super) const TIMESTAMP_TAG: &str = "tag:yaml.org,2002:timestamp";
pub(super) const BINARY_TAG: &str = "tag:yaml.org,2002:binary";
pub(super) const MERGE_TAG: &str = "tag:yaml.org,2002:merge";

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct SimpleKey {
    pub(super) possible: bool,
    pub(super) required: bool,
    pub(super) token_number: usize,
    pub(super) mark: Mark,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ParserState {
    StreamStart,
    ImplicitDocumentStart,
    DocumentStart,
    DocumentContent,
    DocumentEnd,
    BlockNode,
    BlockSequenceFirstEntry,
    BlockSequenceEntry,
    IndentlessSequenceEntry,
    BlockMappingFirstKey,
    BlockMappingKey,
    BlockMappingValue,
    FlowSequenceFirstEntry,
    FlowSequenceEntry,
    FlowSequenceEntryMappingKey,
    FlowSequenceEntryMappingValue,
    FlowSequenceEntryMappingEnd,
    FlowMappingFirstKey,
    FlowMappingKey,
    FlowMappingValue,
    FlowMappingEmptyValue,
    End,
}

#[derive(Clone, Debug)]
pub(super) struct TagDirective {
    pub(super) handle: Vec<u8>,
    pub(super) prefix: Vec<u8>,
}

pub(super) struct Parser<'a> {
    pub(super) error: ErrorType,
    pub(super) problem: String,
    pub(super) problem_offset: usize,
    pub(super) problem_value: i64,
    pub(super) problem_mark: Mark,
    pub(super) context: &'static str,
    pub(super) context_mark: Mark,
    pub(super) input: &'a [u8],
    pub(super) input_pos: usize,
    pub(super) eof: bool,
    pub(super) buffer: Vec<u8>,
    pub(super) buffer_pos: usize,
    pub(super) unread: usize,
    pub(super) raw_buffer: Vec<u8>,
    pub(super) raw_buffer_pos: usize,
    pub(super) encoding: Encoding,
    pub(super) offset: usize,
    pub(super) mark: Mark,
    pub(super) stream_start_produced: bool,
    pub(super) stream_end_produced: bool,
    pub(super) flow_level: usize,
    pub(super) tokens: VecDeque<Token>,
    pub(super) tokens_parsed: usize,
    pub(super) token_available: bool,
    pub(super) indent: isize,
    pub(super) indents: Vec<isize>,
    pub(super) simple_key_allowed: bool,
    pub(super) simple_keys: Vec<SimpleKey>,
    pub(super) simple_keys_by_tok: HashMap<usize, usize>,
    pub(super) state: ParserState,
    pub(super) states: Vec<ParserState>,
    pub(super) marks: Vec<Mark>,
    pub(super) tag_directives: Vec<TagDirective>,
}
