use std::collections::HashMap;
use std::fmt;

use gocompat::unicode;
use gocompat::utf8::Rune;

use super::charclass::{
    CharSet, any_class, digit_class, ecma_any_class, ecma_digit_class, ecma_space_class,
    ecma_word_class, is_valid_unicode_cat, is_word_char, none_class, not_digit_class,
    not_ecma_digit_class, not_ecma_space_class, not_ecma_word_class, not_re2_space_class,
    not_space_class, not_word_class, re2_space_class, space_class, word_class,
};
use super::tree::{
    Arena, NT_ALTERNATE, NT_BEGINNING, NT_BOL, NT_BOUNDARY, NT_CAPTURE, NT_CONCATENATE,
    NT_ECMA_BOUNDARY, NT_END, NT_END_Z, NT_EOL, NT_GREEDY, NT_GROUP, NT_NON_ECMA_BOUNDARY,
    NT_NONBOUNDARY, NT_NOTHING, NT_NOTONE, NT_ONE, NT_PREVENT, NT_REF, NT_REQUIRE, NT_SET,
    NT_START, NT_TESTGROUP, NT_TESTREF, NodeId, NodeType, RegexTree,
};
use super::{MAX_INT32, decode_runes, rune_to_char, runes_to_string, write_stdout};

pub type RegexOptions = i32;

pub const IGNORE_CASE: RegexOptions = 0x0001;
pub const MULTILINE: RegexOptions = 0x0002;
pub const EXPLICIT_CAPTURE: RegexOptions = 0x0004;
pub const COMPILED: RegexOptions = 0x0008;
pub const SINGLELINE: RegexOptions = 0x0010;
pub const IGNORE_PATTERN_WHITESPACE: RegexOptions = 0x0020;
pub const RIGHT_TO_LEFT: RegexOptions = 0x0040;
pub const DEBUG: RegexOptions = 0x0080;
pub const ECMASCRIPT: RegexOptions = 0x0100;
pub const RE2: RegexOptions = 0x0200;
pub const UNICODE: RegexOptions = 0x0400;

fn option_from_code(ch: Rune) -> RegexOptions {
    match rune_to_char(ch) {
        Some('i' | 'I') => IGNORE_CASE,
        Some('r' | 'R') => RIGHT_TO_LEFT,
        Some('m' | 'M') => MULTILINE,
        Some('n' | 'N') => EXPLICIT_CAPTURE,
        Some('s' | 'S') => SINGLELINE,
        Some('x' | 'X') => IGNORE_PATTERN_WHITESPACE,
        Some('d' | 'D') => DEBUG,
        Some('e' | 'E') => ECMASCRIPT,
        Some('u' | 'U') => UNICODE,
        _ => 0,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    InternalError,
    UnterminatedComment,
    InvalidCharRange,
    InvalidRepeatSize,
    InvalidUtf8,
    CaptureGroupOutOfRange,
    UnexpectedParen,
    MissingParen,
    MissingBrace,
    InvalidRepeatOp,
    MissingRepeatArgument,
    ConditionalExpression,
    TooManyAlternates,
    UnrecognizedGrouping,
    InvalidGroupName,
    CapNumNotZero,
    UndefinedBackRef,
    UndefinedNameRef,
    AlternationCantCapture,
    AlternationCantHaveComment,
    MalformedReference,
    UndefinedReference,
    IllegalEndEscape,
    MalformedSlashP,
    IncompleteSlashP,
    UnknownSlashP,
    UnrecognizedEscape,
    MissingControl,
    UnrecognizedControl,
    TooFewHex,
    InvalidHex,
    MalformedNameRef,
    BadClassInCharRange,
    UnterminatedBracket,
    SubtractionMustBeLast,
    ReversedCharRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ErrorArg {
    Int(isize),
    Str(String),
    Rune(Rune),
}

impl ErrorArg {
    fn text(&self) -> String {
        match self {
            ErrorArg::Int(i) => i.to_string(),
            ErrorArg::Str(s) => s.clone(),
            ErrorArg::Rune(r) => runes_to_string(&[*r]),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub args: Vec<ErrorArg>,
    pub pos: usize,
}

impl Error {
    pub fn reason(&self) -> String {
        let a = |i: usize| self.args.get(i).map(ErrorArg::text).unwrap_or_default();
        match self.code {
            ErrorCode::InternalError => "internal error".to_string(),
            ErrorCode::UnterminatedComment => "unterminated comment".to_string(),
            ErrorCode::InvalidCharRange => "invalid character class range".to_string(),
            ErrorCode::InvalidRepeatSize => "invalid repeat count".to_string(),
            ErrorCode::InvalidUtf8 => "invalid UTF-8".to_string(),
            ErrorCode::CaptureGroupOutOfRange => "capture group number out of range".to_string(),
            ErrorCode::UnexpectedParen => "unexpected )".to_string(),
            ErrorCode::MissingParen => "missing closing )".to_string(),
            ErrorCode::MissingBrace => "missing closing }".to_string(),
            ErrorCode::InvalidRepeatOp => "invalid nested repetition operator".to_string(),
            ErrorCode::MissingRepeatArgument => "missing argument to repetition operator".to_string(),
            ErrorCode::ConditionalExpression => "illegal conditional (?(...)) expression".to_string(),
            ErrorCode::TooManyAlternates => "too many | in (?()|)".to_string(),
            ErrorCode::UnrecognizedGrouping => format!("unrecognized grouping construct: ({}", a(0)),
            ErrorCode::InvalidGroupName => "invalid group name: group names must begin with a word character and have a matching terminator".to_string(),
            ErrorCode::CapNumNotZero => "capture number cannot be zero".to_string(),
            ErrorCode::UndefinedBackRef => format!("reference to undefined group number {}", a(0)),
            ErrorCode::UndefinedNameRef => format!("reference to undefined group name {}", a(0)),
            ErrorCode::AlternationCantCapture => {
                "alternation conditions do not capture and cannot be named".to_string()
            }
            ErrorCode::AlternationCantHaveComment => {
                "alternation conditions cannot be comments".to_string()
            }
            ErrorCode::MalformedReference => format!("(?({}) ) malformed", a(0)),
            ErrorCode::UndefinedReference => {
                format!("(?({}) ) reference to undefined group", a(0))
            }
            ErrorCode::IllegalEndEscape => "illegal \\ at end of pattern".to_string(),
            ErrorCode::MalformedSlashP => "malformed \\p{X} character escape".to_string(),
            ErrorCode::IncompleteSlashP => "incomplete \\p{X} character escape".to_string(),
            ErrorCode::UnknownSlashP => {
                format!("unknown unicode category, script, or property '{}'", a(0))
            }
            ErrorCode::UnrecognizedEscape => format!("unrecognized escape sequence \\{}", a(0)),
            ErrorCode::MissingControl => "missing control character".to_string(),
            ErrorCode::UnrecognizedControl => "unrecognized control character".to_string(),
            ErrorCode::TooFewHex => "insufficient hexadecimal digits".to_string(),
            ErrorCode::InvalidHex => "hex values may not be larger than 0x10FFFF".to_string(),
            ErrorCode::MalformedNameRef => "malformed \\k<...> named back reference".to_string(),
            ErrorCode::BadClassInCharRange => {
                format!("cannot include class \\{} in character range", a(0))
            }
            ErrorCode::UnterminatedBracket => "unterminated [] set".to_string(),
            ErrorCode::SubtractionMustBeLast => {
                "a subtraction must be the last element in a character class".to_string()
            }
            ErrorCode::ReversedCharRange => format!("[{}-{}] range in reverse order", a(0), a(1)),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.reason())
    }
}

impl std::error::Error for Error {}

pub(crate) struct Parser {
    pub(crate) arena: Arena,
    stack: Option<NodeId>,
    group: Option<NodeId>,
    alternation: Option<NodeId>,
    pub(crate) concatenation: Option<NodeId>,
    unit: Option<NodeId>,
    pattern: Vec<Rune>,
    current_pos: isize,
    autocap: isize,
    capcount: isize,
    captop: isize,
    pub(crate) capsize: isize,
    pub(crate) caps: Option<HashMap<isize, isize>>,
    pub(crate) capnames: Option<HashMap<String, isize>>,
    capnumlist: Option<Vec<isize>>,
    capnamelist: Option<Vec<String>>,
    options: RegexOptions,
    options_stack: Vec<RegexOptions>,
    ignore_next_paren: bool,
}

const MAX_VALUE_DIV10: isize = MAX_INT32 / 10;
const MAX_VALUE_MOD10: isize = MAX_INT32 % 10;

pub fn parse<S: AsRef<[u8]> + ?Sized>(re: &S, op: RegexOptions) -> Result<RegexTree, Error> {
    let mut p = Parser::new(op);
    p.caps = Some(HashMap::new());
    p.set_pattern(re.as_ref());
    p.count_captures()?;
    p.reset(op);
    let root = p.scan_regex()?;
    let tree = RegexTree {
        nodes: p.arena.nodes,
        root,
        caps: p.caps.unwrap_or_default(),
        capnumlist: p.capnumlist,
        captop: p.captop,
        capnames: p.capnames,
        caplist: p.capnamelist,
        options: op,
    };
    if tree.options & DEBUG > 0 {
        write_stdout(&tree.dump());
    }
    Ok(tree)
}

fn is_digit(ch: Rune) -> bool {
    ('0' as Rune..='9' as Rune).contains(&ch)
}

impl Parser {
    pub(crate) fn new(options: RegexOptions) -> Parser {
        Parser {
            arena: Arena::default(),
            stack: None,
            group: None,
            alternation: None,
            concatenation: None,
            unit: None,
            pattern: Vec::new(),
            current_pos: 0,
            autocap: 0,
            capcount: 0,
            captop: 0,
            capsize: 0,
            caps: None,
            capnames: None,
            capnumlist: None,
            capnamelist: None,
            options,
            options_stack: Vec::new(),
            ignore_next_paren: false,
        }
    }

    pub(crate) fn set_pattern(&mut self, pattern: &[u8]) {
        self.pattern = decode_runes(pattern);
    }

    pub(crate) fn get_err(&self, code: ErrorCode, args: Vec<ErrorArg>) -> Error {
        Error {
            code,
            args,
            pos: self.current_pos.max(0) as usize,
        }
    }

    fn group_type(&self) -> NodeType {
        self.arena.nodes[self.group.expect("an open group")].t
    }

    fn note_capture_slot(&mut self, i: isize, pos: isize) {
        let caps = self.caps.get_or_insert_with(HashMap::new);
        if let std::collections::hash_map::Entry::Vacant(e) = caps.entry(i) {
            e.insert(pos);
            self.capcount += 1;
            if self.captop <= i {
                if i == MAX_INT32 {
                    self.captop = i;
                } else {
                    self.captop = i + 1;
                }
            }
        }
    }

    fn note_capture_name(&mut self, name: String, pos: isize) {
        let capnames = self.capnames.get_or_insert_with(HashMap::new);
        if !capnames.contains_key(&name) {
            capnames.insert(name.clone(), pos);
            self.capnamelist.get_or_insert_with(Vec::new).push(name);
        }
    }

    fn assign_name_slots(&mut self) {
        if self.capnames.is_some() {
            let names = self.capnamelist.clone().unwrap_or_default();
            for name in names {
                while self.is_capture_slot(self.autocap) {
                    self.autocap += 1;
                }
                let capnames = self.capnames.as_mut().expect("capture names");
                let pos = capnames.get(&name).copied().unwrap_or(0);
                capnames.insert(name, self.autocap);
                self.note_capture_slot(self.autocap, pos);
                self.autocap += 1;
            }
        }
        if self.capcount < self.captop {
            let mut list: Vec<isize> = self
                .caps
                .as_ref()
                .map(|c| c.keys().copied().collect())
                .unwrap_or_default();
            list.sort_unstable();
            self.capnumlist = Some(list);
        }
        if self.capnames.is_some() || self.capnumlist.is_some() {
            let oldcapnamelist: Vec<String>;
            let mut next: isize;
            let mut k = 0usize;
            if self.capnames.is_none() {
                oldcapnamelist = Vec::new();
                self.capnames = Some(HashMap::new());
                self.capnamelist = Some(Vec::new());
                next = -1;
            } else {
                oldcapnamelist = self.capnamelist.take().unwrap_or_default();
                self.capnamelist = Some(Vec::new());
                next = self.capname_slot(&oldcapnamelist[0]);
            }
            for i in 0..self.capcount {
                let j = match &self.capnumlist {
                    Some(list) => list[i as usize],
                    None => i,
                };
                if next == j {
                    let name = oldcapnamelist[k].clone();
                    self.capnamelist.as_mut().expect("name list").push(name);
                    k += 1;
                    if k == oldcapnamelist.len() {
                        next = -1;
                    } else {
                        next = self.capname_slot(&oldcapnamelist[k]);
                    }
                } else {
                    let s = j.to_string();
                    self.capnamelist
                        .as_mut()
                        .expect("name list")
                        .push(s.clone());
                    self.capnames.as_mut().expect("capture names").insert(s, j);
                }
            }
        }
    }

    fn capname_slot(&self, name: &str) -> isize {
        self.capnames
            .as_ref()
            .and_then(|m| m.get(name).copied())
            .unwrap_or(0)
    }

    fn consume_autocap(&mut self) -> isize {
        let r = self.autocap;
        self.autocap += 1;
        r
    }

    fn count_captures(&mut self) -> Result<(), Error> {
        self.note_capture_slot(0, 0);
        self.autocap = 1;
        while self.chars_right() > 0 {
            let pos = self.textpos();
            let ch = self.move_right_get_char();
            match rune_to_char(ch) {
                Some('\\') => {
                    if self.chars_right() > 0 {
                        let _ = self.scan_backslash(true);
                    }
                }
                Some('#') => {
                    if self.use_option_x() {
                        self.move_left();
                        let _ = self.scan_blank();
                    }
                }
                Some('[') => {
                    let _ = self.scan_char_set(false, true);
                }
                Some(')') => {
                    if !self.empty_options_stack() {
                        self.pop_options();
                    }
                }
                Some('(') => {
                    if self.chars_right() >= 2
                        && self.right_char(1) == '#' as Rune
                        && self.right_char(0) == '?' as Rune
                    {
                        self.move_left();
                        let _ = self.scan_blank();
                    } else {
                        self.push_options();
                        if self.chars_right() > 0 && self.right_char(0) == '?' as Rune {
                            self.move_right(1);
                            if self.chars_right() > 1
                                && (self.right_char(0) == '<' as Rune
                                    || self.right_char(0) == '\'' as Rune)
                            {
                                self.move_right(1);
                                let ch = self.right_char(0);
                                if ch != '0' as Rune && is_word_char(ch) {
                                    if ('1' as Rune..='9' as Rune).contains(&ch) {
                                        let dec = self.scan_decimal()?;
                                        self.note_capture_slot(dec, pos);
                                    } else {
                                        let name = self.scan_capname();
                                        self.note_capture_name(name, pos);
                                    }
                                }
                            } else if self.use_re2()
                                && self.chars_right() > 2
                                && (self.right_char(0) == 'P' as Rune
                                    && self.right_char(1) == '<' as Rune)
                            {
                                self.move_right(2);
                                let ch = self.right_char(0);
                                if is_word_char(ch) {
                                    let name = self.scan_capname();
                                    self.note_capture_name(name, pos);
                                }
                            } else {
                                self.scan_options();
                                if self.chars_right() > 0 {
                                    if self.right_char(0) == ')' as Rune {
                                        self.move_right(1);
                                        self.pop_keep_options();
                                    } else if self.right_char(0) == '(' as Rune {
                                        self.ignore_next_paren = true;
                                        continue;
                                    }
                                }
                            }
                        } else if !self.use_option_n() && !self.ignore_next_paren {
                            let cap = self.consume_autocap();
                            self.note_capture_slot(cap, pos);
                        }
                    }
                    self.ignore_next_paren = false;
                }
                _ => {}
            }
        }
        self.assign_name_slots();
        Ok(())
    }

    fn reset(&mut self, topopts: RegexOptions) {
        self.current_pos = 0;
        self.autocap = 1;
        self.ignore_next_paren = false;
        self.options_stack.clear();
        self.options = topopts;
        self.stack = None;
    }

    fn scan_regex(&mut self) -> Result<NodeId, Error> {
        let mut ch: Rune;
        let mut is_quant = false;
        let root = self.arena.new_node_mn(NT_CAPTURE, self.options, 0, -1);
        self.start_group(root);
        'outer: while self.chars_right() > 0 {
            let mut was_prev_quantifier = is_quant;
            is_quant = false;
            self.scan_blank()?;
            let mut startpos = self.textpos();
            if self.use_option_x() {
                while self.chars_right() > 0 {
                    ch = self.right_char(0);
                    if !(!is_stopper_x(ch) || (ch == '{' as Rune && !self.is_true_quantifier())) {
                        break;
                    }
                    self.move_right(1);
                }
            } else {
                while self.chars_right() > 0 {
                    ch = self.right_char(0);
                    if !(!is_special(ch) || ch == '{' as Rune && !self.is_true_quantifier()) {
                        break;
                    }
                    self.move_right(1);
                }
            }
            let endpos = self.textpos();
            let _ = self.scan_blank();
            if self.chars_right() == 0 {
                ch = '!' as Rune;
            } else {
                ch = self.right_char(0);
                if is_special(ch) {
                    is_quant = is_quantifier(ch);
                    self.move_right(1);
                } else {
                    ch = ' ' as Rune;
                }
            }
            if startpos < endpos {
                let mut cch_unquantified = endpos - startpos;
                if is_quant {
                    cch_unquantified -= 1;
                }
                was_prev_quantifier = false;
                if cch_unquantified > 0 {
                    self.add_to_concatenate(startpos, cch_unquantified, false);
                }
                if is_quant {
                    let c = self.char_at(endpos - 1);
                    self.add_unit_one(c);
                }
            }
            match rune_to_char(ch) {
                Some('!') => break 'outer,
                Some(' ') => continue 'outer,
                Some('[') => {
                    let ci = self.use_option_i();
                    let cc = self.scan_char_set(ci, false)?;
                    self.add_unit_set(cc.unwrap_or_default());
                }
                Some('(') => {
                    self.push_options();
                    match self.scan_group_open()? {
                        None => self.pop_keep_options(),
                        Some(grouper) => {
                            self.push_group();
                            self.start_group(grouper);
                        }
                    }
                    continue 'outer;
                }
                Some('|') => {
                    self.add_alternate();
                    continue 'outer;
                }
                Some(')') => {
                    if self.empty_stack() {
                        return Err(self.get_err(ErrorCode::UnexpectedParen, Vec::new()));
                    }
                    self.add_group()?;
                    self.pop_group()?;
                    self.pop_options();
                    if self.unit.is_none() {
                        continue 'outer;
                    }
                }
                Some('\\') => {
                    let n = self.scan_backslash(false)?;
                    self.add_unit_node(n);
                }
                Some('^') => {
                    if self.use_option_m() {
                        self.add_unit_type(NT_BOL);
                    } else {
                        self.add_unit_type(NT_BEGINNING);
                    }
                }
                Some('$') => {
                    if self.use_option_m() {
                        self.add_unit_type(NT_EOL);
                    } else {
                        self.add_unit_type(NT_END_Z);
                    }
                }
                Some('.') => {
                    if self.use_option_s() {
                        self.add_unit_set(any_class());
                    } else if self.use_option_e() {
                        self.add_unit_set(ecma_any_class());
                    } else {
                        self.add_unit_notone('\n' as Rune);
                    }
                }
                Some('{' | '*' | '+' | '?') => {
                    if self.unit.is_none() {
                        if was_prev_quantifier {
                            return Err(self.get_err(ErrorCode::InvalidRepeatOp, Vec::new()));
                        }
                        return Err(self.get_err(ErrorCode::MissingRepeatArgument, Vec::new()));
                    }
                    self.move_left();
                }
                _ => return Err(self.get_err(ErrorCode::InternalError, Vec::new())),
            }
            self.scan_blank()?;
            if self.chars_right() > 0 {
                is_quant = self.is_true_quantifier();
            }
            if self.chars_right() == 0 || !is_quant {
                self.add_concatenate();
                continue 'outer;
            }
            ch = self.move_right_get_char();
            while self.unit.is_some() {
                let min: isize;
                let mut max: isize;
                match rune_to_char(ch) {
                    Some('*') => {
                        min = 0;
                        max = MAX_INT32;
                    }
                    Some('?') => {
                        min = 0;
                        max = 1;
                    }
                    Some('+') => {
                        min = 1;
                        max = MAX_INT32;
                    }
                    Some('{') => {
                        startpos = self.textpos();
                        min = self.scan_decimal()?;
                        max = min;
                        if startpos < self.textpos()
                            && self.chars_right() > 0
                            && self.right_char(0) == ',' as Rune
                        {
                            self.move_right(1);
                            if self.chars_right() == 0 || self.right_char(0) == '}' as Rune {
                                max = MAX_INT32;
                            } else {
                                max = self.scan_decimal()?;
                            }
                        }
                        if startpos == self.textpos()
                            || self.chars_right() == 0
                            || self.move_right_get_char() != '}' as Rune
                        {
                            self.add_concatenate();
                            self.textto(startpos - 1);
                            continue 'outer;
                        }
                    }
                    _ => return Err(self.get_err(ErrorCode::InternalError, Vec::new())),
                }
                self.scan_blank()?;
                let lazy = if self.chars_right() == 0 || self.right_char(0) != '?' as Rune {
                    false
                } else {
                    self.move_right(1);
                    true
                };
                if min > max {
                    return Err(self.get_err(ErrorCode::InvalidRepeatSize, Vec::new()));
                }
                self.add_concatenate3(lazy, min, max);
            }
        }
        if !self.empty_stack() {
            return Err(self.get_err(ErrorCode::MissingParen, Vec::new()));
        }
        self.add_group()?;
        Ok(self.unit.expect("the root group"))
    }

    pub(crate) fn scan_replacement(&mut self) -> Result<NodeId, Error> {
        let concatenation = self.arena.new_node(NT_CONCATENATE, self.options);
        self.concatenation = Some(concatenation);
        loop {
            let mut c = self.chars_right();
            if c == 0 {
                break;
            }
            let startpos = self.textpos();
            while c > 0 && self.right_char(0) != '$' as Rune {
                self.move_right(1);
                c -= 1;
            }
            self.add_to_concatenate(startpos, self.textpos() - startpos, true);
            if c > 0 {
                if self.move_right_get_char() == '$' as Rune {
                    let n = self.scan_dollar()?;
                    self.add_unit_node(Some(n));
                }
                self.add_concatenate();
            }
        }
        Ok(self.concatenation.expect("the replacement concatenation"))
    }

    fn scan_dollar(&mut self) -> Result<NodeId, Error> {
        if self.chars_right() == 0 {
            return Ok(self.arena.new_node_ch(NT_ONE, self.options, '$' as Rune));
        }
        let mut ch = self.right_char(0);
        let mut angled = false;
        let backpos = self.textpos();
        let mut last_end_pos = backpos;
        if ch == '{' as Rune && self.chars_right() > 1 {
            angled = true;
            self.move_right(1);
            ch = self.right_char(0);
        }
        if is_digit(ch) {
            if !angled && self.use_option_e() {
                let mut capnum: isize = -1;
                let mut newcapnum = (ch - '0' as Rune) as isize;
                self.move_right(1);
                if self.is_capture_slot(newcapnum) {
                    capnum = newcapnum;
                    last_end_pos = self.textpos();
                }
                while self.chars_right() > 0 {
                    ch = self.right_char(0);
                    if !is_digit(ch) {
                        break;
                    }
                    let digit = (ch - '0' as Rune) as isize;
                    if newcapnum > MAX_VALUE_DIV10
                        || (newcapnum == MAX_VALUE_DIV10 && digit > MAX_VALUE_MOD10)
                    {
                        return Err(self.get_err(ErrorCode::CaptureGroupOutOfRange, Vec::new()));
                    }
                    newcapnum = newcapnum * 10 + digit;
                    self.move_right(1);
                    if self.is_capture_slot(newcapnum) {
                        capnum = newcapnum;
                        last_end_pos = self.textpos();
                    }
                }
                self.textto(last_end_pos);
                if capnum >= 0 {
                    return Ok(self.arena.new_node_m(NT_REF, self.options, capnum));
                }
            } else {
                let capnum = self.scan_decimal()?;
                if (!angled || self.chars_right() > 0 && self.move_right_get_char() == '}' as Rune)
                    && self.is_capture_slot(capnum)
                {
                    return Ok(self.arena.new_node_m(NT_REF, self.options, capnum));
                }
            }
        } else if angled && is_word_char(ch) {
            let capname = self.scan_capname();
            if self.chars_right() > 0
                && self.move_right_get_char() == '}' as Rune
                && self.is_capture_name(&capname)
            {
                let slot = self.capture_slot_from_name(&capname);
                return Ok(self.arena.new_node_m(NT_REF, self.options, slot));
            }
        } else if !angled {
            let mut capnum: isize = 1;
            match rune_to_char(ch) {
                Some('$') => {
                    self.move_right(1);
                    return Ok(self.arena.new_node_ch(NT_ONE, self.options, '$' as Rune));
                }
                Some('&') => capnum = 0,
                Some('`') => capnum = REPLACE_LEFT_PORTION,
                Some('\'') => capnum = REPLACE_RIGHT_PORTION,
                Some('+') => capnum = REPLACE_LAST_GROUP,
                Some('_') => capnum = REPLACE_WHOLE_STRING,
                _ => {}
            }
            if capnum != 1 {
                self.move_right(1);
                return Ok(self.arena.new_node_m(NT_REF, self.options, capnum));
            }
        }
        self.textto(backpos);
        Ok(self.arena.new_node_ch(NT_ONE, self.options, '$' as Rune))
    }

    fn scan_group_open(&mut self) -> Result<Option<NodeId>, Error> {
        let mut close: Rune = '>' as Rune;
        let start = self.textpos();
        if self.chars_right() == 0
            || self.right_char(0) != '?' as Rune
            || (self.right_char(0) == '?' as Rune
                && (self.chars_right() > 1 && self.right_char(1) == ')' as Rune))
        {
            if self.use_option_n() || self.ignore_next_paren {
                self.ignore_next_paren = false;
                return Ok(Some(self.arena.new_node(NT_GROUP, self.options)));
            }
            let cap = self.consume_autocap();
            return Ok(Some(self.arena.new_node_mn(
                NT_CAPTURE,
                self.options,
                cap,
                -1,
            )));
        }
        self.move_right(1);
        'recognize: {
            if self.chars_right() == 0 {
                break 'recognize;
            }
            let nt: NodeType;
            let ch = self.move_right_get_char();
            match rune_to_char(ch) {
                Some(':') => nt = NT_GROUP,
                Some('=') => {
                    self.options &= !RIGHT_TO_LEFT;
                    nt = NT_REQUIRE;
                }
                Some('!') => {
                    self.options &= !RIGHT_TO_LEFT;
                    nt = NT_PREVENT;
                }
                Some('>') => nt = NT_GREEDY,
                Some('\'' | '<') => {
                    if ch == '\'' as Rune {
                        close = '\'' as Rune;
                    }
                    if self.chars_right() == 0 {
                        break 'recognize;
                    }
                    let ch = self.move_right_get_char();
                    match rune_to_char(ch) {
                        Some('=') => {
                            if close == '\'' as Rune {
                                break 'recognize;
                            }
                            self.options |= RIGHT_TO_LEFT;
                            nt = NT_REQUIRE;
                        }
                        Some('!') => {
                            if close == '\'' as Rune {
                                break 'recognize;
                            }
                            self.options |= RIGHT_TO_LEFT;
                            nt = NT_PREVENT;
                        }
                        _ => {
                            self.move_left();
                            let mut capnum: isize = -1;
                            let mut uncapnum: isize = -1;
                            let mut proceed = false;
                            if is_digit(ch) {
                                capnum = self.scan_decimal()?;
                                if !self.is_capture_slot(capnum) {
                                    capnum = -1;
                                }
                                if self.chars_right() > 0
                                    && !(self.right_char(0) == close
                                        || self.right_char(0) == '-' as Rune)
                                {
                                    return Err(
                                        self.get_err(ErrorCode::InvalidGroupName, Vec::new())
                                    );
                                }
                                if capnum == 0 {
                                    return Err(self.get_err(ErrorCode::CapNumNotZero, Vec::new()));
                                }
                            } else if is_word_char(ch) {
                                let capname = self.scan_capname();
                                if self.is_capture_name(&capname) {
                                    capnum = self.capture_slot_from_name(&capname);
                                }
                                if self.chars_right() > 0
                                    && !(self.right_char(0) == close
                                        || self.right_char(0) == '-' as Rune)
                                {
                                    return Err(
                                        self.get_err(ErrorCode::InvalidGroupName, Vec::new())
                                    );
                                }
                            } else if ch == '-' as Rune {
                                proceed = true;
                            } else {
                                return Err(self.get_err(ErrorCode::InvalidGroupName, Vec::new()));
                            }
                            if (capnum != -1 || proceed)
                                && self.chars_right() > 0
                                && self.right_char(0) == '-' as Rune
                            {
                                self.move_right(1);
                                if self.chars_right() == 0 {
                                    return Err(
                                        self.get_err(ErrorCode::InvalidGroupName, Vec::new())
                                    );
                                }
                                let ch = self.right_char(0);
                                if is_digit(ch) {
                                    uncapnum = self.scan_decimal()?;
                                    if !self.is_capture_slot(uncapnum) {
                                        return Err(self.get_err(
                                            ErrorCode::UndefinedBackRef,
                                            vec![ErrorArg::Int(uncapnum)],
                                        ));
                                    }
                                    if self.chars_right() > 0 && self.right_char(0) != close {
                                        return Err(
                                            self.get_err(ErrorCode::InvalidGroupName, Vec::new())
                                        );
                                    }
                                } else if is_word_char(ch) {
                                    let uncapname = self.scan_capname();
                                    if !self.is_capture_name(&uncapname) {
                                        return Err(self.get_err(
                                            ErrorCode::UndefinedNameRef,
                                            vec![ErrorArg::Str(uncapname)],
                                        ));
                                    }
                                    uncapnum = self.capture_slot_from_name(&uncapname);
                                    if self.chars_right() > 0 && self.right_char(0) != close {
                                        return Err(
                                            self.get_err(ErrorCode::InvalidGroupName, Vec::new())
                                        );
                                    }
                                } else {
                                    return Err(
                                        self.get_err(ErrorCode::InvalidGroupName, Vec::new())
                                    );
                                }
                            }
                            if (capnum != -1 || uncapnum != -1)
                                && self.chars_right() > 0
                                && self.move_right_get_char() == close
                            {
                                return Ok(Some(self.arena.new_node_mn(
                                    NT_CAPTURE,
                                    self.options,
                                    capnum,
                                    uncapnum,
                                )));
                            }
                            break 'recognize;
                        }
                    }
                }
                Some('(') => {
                    let paren_pos = self.textpos();
                    if self.chars_right() > 0 {
                        let ch = self.right_char(0);
                        if is_digit(ch) {
                            let capnum = self.scan_decimal()?;
                            if self.chars_right() > 0 && self.move_right_get_char() == ')' as Rune {
                                if self.is_capture_slot(capnum) {
                                    return Ok(Some(self.arena.new_node_m(
                                        NT_TESTREF,
                                        self.options,
                                        capnum,
                                    )));
                                }
                                return Err(self.get_err(
                                    ErrorCode::UndefinedReference,
                                    vec![ErrorArg::Int(capnum)],
                                ));
                            }
                            return Err(self.get_err(
                                ErrorCode::MalformedReference,
                                vec![ErrorArg::Int(capnum)],
                            ));
                        } else if is_word_char(ch) {
                            let capname = self.scan_capname();
                            if self.is_capture_name(&capname)
                                && self.chars_right() > 0
                                && self.move_right_get_char() == ')' as Rune
                            {
                                let slot = self.capture_slot_from_name(&capname);
                                return Ok(Some(self.arena.new_node_m(
                                    NT_TESTREF,
                                    self.options,
                                    slot,
                                )));
                            }
                        }
                    }
                    nt = NT_TESTGROUP;
                    self.textto(paren_pos - 1);
                    self.ignore_next_paren = true;
                    let chars_right = self.chars_right();
                    if chars_right >= 3 && self.right_char(1) == '?' as Rune {
                        let rightchar2 = self.right_char(2);
                        if rightchar2 == '#' as Rune {
                            return Err(
                                self.get_err(ErrorCode::AlternationCantHaveComment, Vec::new())
                            );
                        }
                        if rightchar2 == '\'' as Rune {
                            return Err(self.get_err(ErrorCode::AlternationCantCapture, Vec::new()));
                        }
                        if chars_right >= 4
                            && (rightchar2 == '<' as Rune
                                && self.right_char(3) != '!' as Rune
                                && self.right_char(3) != '=' as Rune)
                        {
                            return Err(self.get_err(ErrorCode::AlternationCantCapture, Vec::new()));
                        }
                    }
                }
                Some('P') if self.use_re2() => {
                    if self.chars_right() < 3 {
                        break 'recognize;
                    }
                    let ch = self.move_right_get_char();
                    if ch != '<' as Rune {
                        break 'recognize;
                    }
                    let ch = self.move_right_get_char();
                    self.move_left();
                    if is_word_char(ch) {
                        let mut capnum: isize = -1;
                        let capname = self.scan_capname();
                        if self.is_capture_name(&capname) {
                            capnum = self.capture_slot_from_name(&capname);
                        }
                        if self.chars_right() > 0 && self.right_char(0) != '>' as Rune {
                            return Err(self.get_err(ErrorCode::InvalidGroupName, Vec::new()));
                        }
                        if capnum != -1
                            && self.chars_right() > 0
                            && self.move_right_get_char() == '>' as Rune
                        {
                            return Ok(Some(self.arena.new_node_mn(
                                NT_CAPTURE,
                                self.options,
                                capnum,
                                -1,
                            )));
                        }
                        break 'recognize;
                    }
                    return Err(self.get_err(ErrorCode::InvalidGroupName, Vec::new()));
                }
                _ => {
                    self.move_left();
                    nt = NT_GROUP;
                    if self.group_type() != NT_TESTGROUP {
                        self.scan_options();
                    }
                    if self.chars_right() == 0 {
                        break 'recognize;
                    }
                    let ch = self.move_right_get_char();
                    if ch == ')' as Rune {
                        return Ok(None);
                    }
                    if ch != ':' as Rune {
                        break 'recognize;
                    }
                }
            }
            return Ok(Some(self.arena.new_node(nt, self.options)));
        }
        let fragment = runes_to_string(&self.pattern[start as usize..self.textpos() as usize]);
        Err(self.get_err(
            ErrorCode::UnrecognizedGrouping,
            vec![ErrorArg::Str(fragment)],
        ))
    }

    fn scan_backslash(&mut self, scan_only: bool) -> Result<Option<NodeId>, Error> {
        if self.chars_right() == 0 {
            return Err(self.get_err(ErrorCode::IllegalEndEscape, Vec::new()));
        }
        let ch = self.right_char(0);
        let opts = self.options;
        let ecma_or_re2 = self.use_option_e() || self.use_re2();
        match rune_to_char(ch) {
            Some('b' | 'B' | 'A' | 'G' | 'Z' | 'z') => {
                self.move_right(1);
                let t = self.type_from_code(ch);
                Ok(Some(self.arena.new_node(t, opts)))
            }
            Some('w') => {
                self.move_right(1);
                let set = if ecma_or_re2 {
                    ecma_word_class()
                } else {
                    word_class()
                };
                Ok(Some(self.arena.new_node_set(NT_SET, opts, set)))
            }
            Some('W') => {
                self.move_right(1);
                let set = if ecma_or_re2 {
                    not_ecma_word_class()
                } else {
                    not_word_class()
                };
                Ok(Some(self.arena.new_node_set(NT_SET, opts, set)))
            }
            Some('s') => {
                self.move_right(1);
                let set = if self.use_option_e() {
                    ecma_space_class()
                } else if self.use_re2() {
                    re2_space_class()
                } else {
                    space_class()
                };
                Ok(Some(self.arena.new_node_set(NT_SET, opts, set)))
            }
            Some('S') => {
                self.move_right(1);
                let set = if self.use_option_e() {
                    not_ecma_space_class()
                } else if self.use_re2() {
                    not_re2_space_class()
                } else {
                    not_space_class()
                };
                Ok(Some(self.arena.new_node_set(NT_SET, opts, set)))
            }
            Some('d') => {
                self.move_right(1);
                let set = if ecma_or_re2 {
                    ecma_digit_class()
                } else {
                    digit_class()
                };
                Ok(Some(self.arena.new_node_set(NT_SET, opts, set)))
            }
            Some('D') => {
                self.move_right(1);
                let set = if ecma_or_re2 {
                    not_ecma_digit_class()
                } else {
                    not_digit_class()
                };
                Ok(Some(self.arena.new_node_set(NT_SET, opts, set)))
            }
            Some('p' | 'P') => {
                self.move_right(1);
                let prop = self.parse_property()?;
                let mut cc = CharSet::default();
                cc.add_category(&prop, ch != 'p' as Rune, self.use_option_i());
                if self.use_option_i() {
                    cc.add_lowercase();
                }
                Ok(Some(self.arena.new_node_set(NT_SET, opts, cc)))
            }
            _ => self.scan_basic_backslash(scan_only),
        }
    }

    fn scan_basic_backslash(&mut self, scan_only: bool) -> Result<Option<NodeId>, Error> {
        if self.chars_right() == 0 {
            return Err(self.get_err(ErrorCode::IllegalEndEscape, Vec::new()));
        }
        let mut angled = false;
        let mut k = false;
        let mut close: Rune = 0;
        let backpos = self.textpos();
        let mut ch = self.right_char(0);
        let has_names = self.capnames.as_ref().is_some_and(|m| !m.is_empty());
        if ch == 'k' as Rune && (!self.use_option_e() || has_names) {
            if self.chars_right() >= 2 {
                self.move_right(1);
                ch = self.move_right_get_char();
                if ch == '<' as Rune || (!self.use_option_e() && ch == '\'' as Rune) {
                    angled = true;
                    if ch == '\'' as Rune {
                        close = '\'' as Rune;
                    } else {
                        close = '>' as Rune;
                    }
                }
            }
            if !angled || self.chars_right() <= 0 {
                return Err(self.get_err(ErrorCode::MalformedNameRef, Vec::new()));
            }
            ch = self.right_char(0);
            k = true;
        } else if !self.use_option_e()
            && (ch == '<' as Rune || ch == '\'' as Rune)
            && self.chars_right() > 1
        {
            angled = true;
            if ch == '\'' as Rune {
                close = '\'' as Rune;
            } else {
                close = '>' as Rune;
            }
            self.move_right(1);
            ch = self.right_char(0);
        }
        if angled && is_digit(ch) {
            let capnum = self.scan_decimal()?;
            if self.chars_right() > 0 && self.move_right_get_char() == close {
                if self.is_capture_slot(capnum) {
                    return Ok(Some(self.arena.new_node_m(NT_REF, self.options, capnum)));
                }
                return Err(self.get_err(ErrorCode::UndefinedBackRef, vec![ErrorArg::Int(capnum)]));
            }
        } else if !angled && ('1' as Rune..='9' as Rune).contains(&ch) {
            let capnum = self.scan_decimal()?;
            if scan_only {
                return Ok(None);
            }
            if self.is_capture_slot(capnum) {
                return Ok(Some(self.arena.new_node_m(NT_REF, self.options, capnum)));
            }
            if capnum <= 9 && !self.use_option_e() {
                return Err(self.get_err(ErrorCode::UndefinedBackRef, vec![ErrorArg::Int(capnum)]));
            }
        } else if angled {
            let capname = self.scan_capname();
            if !capname.is_empty() && self.chars_right() > 0 && self.move_right_get_char() == close
            {
                if scan_only {
                    return Ok(None);
                }
                if self.is_capture_name(&capname) {
                    let slot = self.capture_slot_from_name(&capname);
                    return Ok(Some(self.arena.new_node_m(NT_REF, self.options, slot)));
                }
                return Err(self.get_err(ErrorCode::UndefinedNameRef, vec![ErrorArg::Str(capname)]));
            } else if k {
                return Err(self.get_err(ErrorCode::MalformedNameRef, Vec::new()));
            }
        }
        self.textto(backpos);
        let mut ch = self.scan_char_escape()?;
        if scan_only {
            return Ok(None);
        }
        if self.use_option_i() {
            ch = unicode::to_lower(ch);
        }
        Ok(Some(self.arena.new_node_ch(NT_ONE, self.options, ch)))
    }

    fn parse_property(&mut self) -> Result<String, Error> {
        if self.chars_right() >= 1 && self.right_char(0) != '{' as Rune {
            let c = self.move_right_get_char();
            let ch = runes_to_string(&[c]);
            if !is_valid_unicode_cat(&ch) {
                return Err(self.get_err(ErrorCode::UnknownSlashP, vec![ErrorArg::Str(ch)]));
            }
            return Ok(ch);
        }
        if self.chars_right() < 3 {
            return Err(self.get_err(ErrorCode::IncompleteSlashP, Vec::new()));
        }
        let ch = self.move_right_get_char();
        if ch != '{' as Rune {
            return Err(self.get_err(ErrorCode::MalformedSlashP, Vec::new()));
        }
        let startpos = self.textpos();
        while self.chars_right() > 0 {
            let ch = self.move_right_get_char();
            if !(is_word_char(ch) || ch == '-' as Rune) {
                self.move_left();
                break;
            }
        }
        let capname = runes_to_string(&self.pattern[startpos as usize..self.textpos() as usize]);
        if self.chars_right() == 0 || self.move_right_get_char() != '}' as Rune {
            return Err(self.get_err(ErrorCode::IncompleteSlashP, Vec::new()));
        }
        if !is_valid_unicode_cat(&capname) {
            return Err(self.get_err(ErrorCode::UnknownSlashP, vec![ErrorArg::Str(capname)]));
        }
        Ok(capname)
    }

    fn type_from_code(&self, ch: Rune) -> NodeType {
        match rune_to_char(ch) {
            Some('b') => {
                if self.use_option_e() {
                    NT_ECMA_BOUNDARY
                } else {
                    NT_BOUNDARY
                }
            }
            Some('B') => {
                if self.use_option_e() {
                    NT_NON_ECMA_BOUNDARY
                } else {
                    NT_NONBOUNDARY
                }
            }
            Some('A') => NT_BEGINNING,
            Some('G') => NT_START,
            Some('Z') => NT_END_Z,
            Some('z') => NT_END,
            _ => NT_NOTHING,
        }
    }

    fn scan_blank(&mut self) -> Result<(), Error> {
        if self.use_option_x() {
            loop {
                while self.chars_right() > 0 && is_space(self.right_char(0)) {
                    self.move_right(1);
                }
                if self.chars_right() == 0 {
                    break;
                }
                if self.right_char(0) == '#' as Rune {
                    while self.chars_right() > 0 && self.right_char(0) != '\n' as Rune {
                        self.move_right(1);
                    }
                } else if self.chars_right() >= 3
                    && self.right_char(2) == '#' as Rune
                    && self.right_char(1) == '?' as Rune
                    && self.right_char(0) == '(' as Rune
                {
                    while self.chars_right() > 0 && self.right_char(0) != ')' as Rune {
                        self.move_right(1);
                    }
                    if self.chars_right() == 0 {
                        return Err(self.get_err(ErrorCode::UnterminatedComment, Vec::new()));
                    }
                    self.move_right(1);
                } else {
                    break;
                }
            }
        } else {
            loop {
                if self.chars_right() < 3
                    || self.right_char(2) != '#' as Rune
                    || self.right_char(1) != '?' as Rune
                    || self.right_char(0) != '(' as Rune
                {
                    return Ok(());
                }
                while self.chars_right() > 0 && self.right_char(0) != ')' as Rune {
                    self.move_right(1);
                }
                if self.chars_right() == 0 {
                    return Err(self.get_err(ErrorCode::UnterminatedComment, Vec::new()));
                }
                self.move_right(1);
            }
        }
        Ok(())
    }

    fn scan_capname(&mut self) -> String {
        let startpos = self.textpos();
        while self.chars_right() > 0 {
            if !is_word_char(self.move_right_get_char()) {
                self.move_left();
                break;
            }
        }
        runes_to_string(&self.pattern[startpos as usize..self.textpos() as usize])
    }

    fn scan_char_set(
        &mut self,
        case_insensitive: bool,
        scan_only: bool,
    ) -> Result<Option<CharSet>, Error> {
        let mut ch: Rune;
        let mut ch_prev: Rune = 0;
        let mut in_range = false;
        let mut first_char = true;
        let mut closed = false;
        let mut cc = CharSet::default();
        if self.chars_right() > 0 && self.right_char(0) == '^' as Rune {
            self.move_right(1);
            if !scan_only {
                cc.negate = true;
            }
        }
        'scan: while self.chars_right() > 0 {
            'body: {
                let mut f_translated_char = false;
                ch = self.move_right_get_char();
                if ch == ']' as Rune {
                    if !first_char {
                        closed = true;
                        break 'scan;
                    } else if self.use_option_e() {
                        if !scan_only {
                            cc.add_ranges(&none_class().ranges);
                        }
                        closed = true;
                        break 'scan;
                    }
                } else if ch == '\\' as Rune && self.chars_right() > 0 {
                    ch = self.move_right_get_char();
                    match rune_to_char(ch) {
                        Some('D' | 'd') => {
                            if !scan_only {
                                if in_range {
                                    return Err(self.get_err(
                                        ErrorCode::BadClassInCharRange,
                                        vec![ErrorArg::Rune(ch)],
                                    ));
                                }
                                cc.add_digit(
                                    self.use_option_e() || self.use_re2(),
                                    ch == 'D' as Rune,
                                );
                            }
                            break 'body;
                        }
                        Some('S' | 's') => {
                            if !scan_only {
                                if in_range {
                                    return Err(self.get_err(
                                        ErrorCode::BadClassInCharRange,
                                        vec![ErrorArg::Rune(ch)],
                                    ));
                                }
                                cc.add_space(
                                    self.use_option_e(),
                                    self.use_re2(),
                                    ch == 'S' as Rune,
                                );
                            }
                            break 'body;
                        }
                        Some('W' | 'w') => {
                            if !scan_only {
                                if in_range {
                                    return Err(self.get_err(
                                        ErrorCode::BadClassInCharRange,
                                        vec![ErrorArg::Rune(ch)],
                                    ));
                                }
                                cc.add_word(
                                    self.use_option_e() || self.use_re2(),
                                    ch == 'W' as Rune,
                                );
                            }
                            break 'body;
                        }
                        Some('p' | 'P') => {
                            if !scan_only {
                                if in_range {
                                    return Err(self.get_err(
                                        ErrorCode::BadClassInCharRange,
                                        vec![ErrorArg::Rune(ch)],
                                    ));
                                }
                                let prop = self.parse_property()?;
                                cc.add_category(&prop, ch != 'p' as Rune, case_insensitive);
                            } else {
                                let _ = self.parse_property();
                            }
                            break 'body;
                        }
                        Some('-') => {
                            if !scan_only {
                                cc.add_range(ch, ch);
                            }
                            break 'body;
                        }
                        _ => {
                            self.move_left();
                            ch = self.scan_char_escape()?;
                            f_translated_char = true;
                        }
                    }
                } else if ch == '[' as Rune
                    && self.chars_right() > 0
                    && self.right_char(0) == ':' as Rune
                    && !in_range
                {
                    let save_pos = self.textpos();
                    self.move_right(1);
                    let mut negate = false;
                    if self.chars_right() > 1 && self.right_char(0) == '^' as Rune {
                        negate = true;
                        self.move_right(1);
                    }
                    let nm = self.scan_capname();
                    if !scan_only && self.use_re2() && !cc.add_named_ascii(&nm, negate) {
                        return Err(self.get_err(ErrorCode::InvalidCharRange, Vec::new()));
                    }
                    if self.chars_right() < 2
                        || self.move_right_get_char() != ':' as Rune
                        || self.move_right_get_char() != ']' as Rune
                    {
                        self.textto(save_pos);
                    } else if self.use_re2() {
                        break 'body;
                    }
                }
                if in_range {
                    in_range = false;
                    if !scan_only {
                        if ch == '[' as Rune && !f_translated_char && !first_char {
                            cc.add_char(ch_prev);
                            let sub = self.scan_char_set(case_insensitive, false)?;
                            cc.add_subtraction(sub.unwrap_or_default());
                            if self.chars_right() > 0 && self.right_char(0) != ']' as Rune {
                                return Err(
                                    self.get_err(ErrorCode::SubtractionMustBeLast, Vec::new())
                                );
                            }
                        } else {
                            if ch_prev > ch {
                                return Err(self.get_err(
                                    ErrorCode::ReversedCharRange,
                                    vec![ErrorArg::Rune(ch_prev), ErrorArg::Rune(ch)],
                                ));
                            }
                            cc.add_range(ch_prev, ch);
                        }
                    }
                } else if self.chars_right() >= 2
                    && self.right_char(0) == '-' as Rune
                    && self.right_char(1) != ']' as Rune
                {
                    ch_prev = ch;
                    in_range = true;
                    self.move_right(1);
                } else if self.chars_right() >= 1
                    && ch == '-' as Rune
                    && !f_translated_char
                    && self.right_char(0) == '[' as Rune
                    && !first_char
                {
                    if !scan_only {
                        self.move_right(1);
                        let sub = self.scan_char_set(case_insensitive, false)?;
                        cc.add_subtraction(sub.unwrap_or_default());
                        if self.chars_right() > 0 && self.right_char(0) != ']' as Rune {
                            return Err(self.get_err(ErrorCode::SubtractionMustBeLast, Vec::new()));
                        }
                    } else {
                        self.move_right(1);
                        let _ = self.scan_char_set(case_insensitive, true);
                    }
                } else if !scan_only {
                    cc.add_range(ch, ch);
                }
            }
            first_char = false;
        }
        if !closed {
            return Err(self.get_err(ErrorCode::UnterminatedBracket, Vec::new()));
        }
        if !scan_only && case_insensitive {
            cc.add_lowercase();
        }
        if scan_only {
            return Ok(None);
        }
        Ok(Some(cc))
    }

    fn scan_decimal(&mut self) -> Result<isize, Error> {
        let mut i: isize = 0;
        while self.chars_right() > 0 {
            let d = self.right_char(0).wrapping_sub('0' as Rune) as isize;
            if !(0..=9).contains(&d) {
                break;
            }
            self.move_right(1);
            if i > MAX_VALUE_DIV10 || (i == MAX_VALUE_DIV10 && d > MAX_VALUE_MOD10) {
                return Err(self.get_err(ErrorCode::CaptureGroupOutOfRange, Vec::new()));
            }
            i *= 10;
            i += d;
        }
        Ok(i)
    }

    fn scan_options(&mut self) {
        let mut off = false;
        while self.chars_right() > 0 {
            let ch = self.right_char(0);
            if ch == '-' as Rune {
                off = true;
            } else if ch == '+' as Rune {
                off = false;
            } else {
                let option = option_from_code(ch);
                if option == 0 || is_only_top_option(option) {
                    return;
                }
                if off {
                    self.options &= !option;
                } else {
                    self.options |= option;
                }
            }
            self.move_right(1);
        }
    }

    pub(crate) fn scan_char_escape(&mut self) -> Result<Rune, Error> {
        let ch = self.move_right_get_char();
        if ('0' as Rune..='7' as Rune).contains(&ch) {
            self.move_left();
            return Ok(self.scan_octal());
        }
        let pos = self.textpos();
        let result = match rune_to_char(ch) {
            Some('x') => {
                if self.chars_right() > 0 && self.right_char(0) == '{' as Rune {
                    if self.use_option_e() {
                        return Ok(ch);
                    }
                    self.move_right(1);
                    return self.scan_hex_until_brace();
                }
                self.scan_hex(2)
            }
            Some('u') => {
                if self.use_option_e()
                    && self.use_option_u()
                    && self.chars_right() > 0
                    && self.right_char(0) == '{' as Rune
                {
                    self.move_right(1);
                    return self.scan_hex_until_brace();
                }
                self.scan_hex(4)
            }
            Some('a') => return Ok(0x07),
            Some('b') => return Ok(0x08),
            Some('e') => return Ok(0x1B),
            Some('f') => return Ok(0x0C),
            Some('n') => return Ok(0x0A),
            Some('r') => return Ok(0x0D),
            Some('t') => return Ok(0x09),
            Some('v') => return Ok(0x0B),
            Some('c') => self.scan_control(),
            _ => {
                if !self.use_option_e() && !self.use_re2() && is_word_char(ch) {
                    return Err(self.get_err(
                        ErrorCode::UnrecognizedEscape,
                        vec![ErrorArg::Str(runes_to_string(&[ch]))],
                    ));
                }
                return Ok(ch);
            }
        };
        if result.is_err() && self.use_option_e() {
            self.textto(pos);
            return Ok(ch);
        }
        result
    }

    fn scan_control(&mut self) -> Result<Rune, Error> {
        if self.chars_right() <= 0 {
            return Err(self.get_err(ErrorCode::MissingControl, Vec::new()));
        }
        let mut ch = self.move_right_get_char();
        if ('a' as Rune..='z' as Rune).contains(&ch) {
            ch -= 'a' as Rune - 'A' as Rune;
        }
        ch = ch.wrapping_sub('@' as Rune);
        if (0..' ' as Rune).contains(&ch) {
            return Ok(ch);
        }
        Err(self.get_err(ErrorCode::UnrecognizedControl, Vec::new()))
    }

    fn scan_hex_until_brace(&mut self) -> Result<Rune, Error> {
        let mut i: i64 = 0;
        let mut has_content = false;
        while self.chars_right() > 0 {
            let ch = self.move_right_get_char();
            if ch == '}' as Rune {
                if !has_content {
                    return Err(self.get_err(ErrorCode::TooFewHex, Vec::new()));
                }
                return Ok(i as Rune);
            }
            has_content = true;
            let d = hex_digit(ch);
            if d < 0 {
                return Err(self.get_err(ErrorCode::MissingBrace, Vec::new()));
            }
            i *= 0x10;
            i += i64::from(d);
            if i > i64::from(unicode::MAX_RUNE) {
                return Err(self.get_err(ErrorCode::InvalidHex, Vec::new()));
            }
        }
        Err(self.get_err(ErrorCode::MissingBrace, Vec::new()))
    }

    fn scan_hex(&mut self, mut c: isize) -> Result<Rune, Error> {
        let mut i: i64 = 0;
        if self.chars_right() >= c {
            while c > 0 {
                let d = hex_digit(self.move_right_get_char());
                if d < 0 {
                    break;
                }
                i *= 0x10;
                i += i64::from(d);
                c -= 1;
            }
        }
        if c > 0 {
            return Err(self.get_err(ErrorCode::TooFewHex, Vec::new()));
        }
        Ok(i as Rune)
    }

    fn scan_octal(&mut self) -> Rune {
        let mut c: isize = 3;
        if c > self.chars_right() {
            c = self.chars_right();
        }
        let mut i: isize = 0;
        let mut d = self.right_char(0).wrapping_sub('0' as Rune) as isize;
        while c > 0 && (0..=7).contains(&d) {
            if i >= 0x20 && self.use_option_e() {
                break;
            }
            i *= 8;
            i += d;
            c -= 1;
            self.move_right(1);
            if !self.right_most() {
                d = self.right_char(0).wrapping_sub('0' as Rune) as isize;
            }
        }
        i &= 0xFF;
        i as Rune
    }

    fn textpos(&self) -> isize {
        self.current_pos
    }

    fn textto(&mut self, pos: isize) {
        self.current_pos = pos;
    }

    pub(crate) fn move_right_get_char(&mut self) -> Rune {
        let ch = self.pattern[self.current_pos as usize];
        self.current_pos += 1;
        ch
    }

    fn move_right(&mut self, i: isize) {
        self.current_pos += i;
    }

    fn move_left(&mut self) {
        self.current_pos -= 1;
    }

    fn char_at(&self, i: isize) -> Rune {
        self.pattern[i as usize]
    }

    fn right_char(&self, i: isize) -> Rune {
        self.pattern[(self.current_pos + i) as usize]
    }

    fn chars_right(&self) -> isize {
        self.pattern.len() as isize - self.current_pos
    }

    pub(crate) fn right_most(&self) -> bool {
        self.current_pos == self.pattern.len() as isize
    }

    fn capture_slot_from_name(&self, capname: &str) -> isize {
        self.capname_slot(capname)
    }

    pub(crate) fn is_capture_slot(&self, i: isize) -> bool {
        if let Some(caps) = &self.caps {
            return caps.contains_key(&i);
        }
        i >= 0 && i < self.capsize
    }

    fn is_capture_name(&self, capname: &str) -> bool {
        match &self.capnames {
            None => false,
            Some(m) => m.contains_key(capname),
        }
    }

    fn use_option_n(&self) -> bool {
        self.options & EXPLICIT_CAPTURE != 0
    }

    fn use_option_i(&self) -> bool {
        self.options & IGNORE_CASE != 0
    }

    fn use_option_m(&self) -> bool {
        self.options & MULTILINE != 0
    }

    fn use_option_s(&self) -> bool {
        self.options & SINGLELINE != 0
    }

    fn use_option_x(&self) -> bool {
        self.options & IGNORE_PATTERN_WHITESPACE != 0
    }

    fn use_option_e(&self) -> bool {
        self.options & ECMASCRIPT != 0
    }

    fn use_re2(&self) -> bool {
        self.options & RE2 != 0
    }

    fn use_option_u(&self) -> bool {
        self.options & UNICODE != 0
    }

    fn empty_options_stack(&self) -> bool {
        self.options_stack.is_empty()
    }

    pub(crate) fn add_concatenate(&mut self) {
        let concatenation = self.concatenation.expect("an open concatenation");
        let unit = self.unit.take().expect("a unit to concatenate");
        self.arena.add_child(concatenation, unit);
    }

    fn add_concatenate3(&mut self, lazy: bool, min: isize, max: isize) {
        let concatenation = self.concatenation.expect("an open concatenation");
        let unit = self.unit.take().expect("a unit to quantify");
        let quantified = self.arena.make_quantifier(unit, lazy, min, max);
        self.arena.add_child(concatenation, quantified);
    }

    fn add_unit_one(&mut self, mut ch: Rune) {
        if self.use_option_i() {
            ch = unicode::to_lower(ch);
        }
        self.unit = Some(self.arena.new_node_ch(NT_ONE, self.options, ch));
    }

    fn add_unit_notone(&mut self, mut ch: Rune) {
        if self.use_option_i() {
            ch = unicode::to_lower(ch);
        }
        self.unit = Some(self.arena.new_node_ch(NT_NOTONE, self.options, ch));
    }

    fn add_unit_set(&mut self, set: CharSet) {
        self.unit = Some(self.arena.new_node_set(NT_SET, self.options, set));
    }

    pub(crate) fn add_unit_node(&mut self, node: Option<NodeId>) {
        self.unit = node;
    }

    fn add_unit_type(&mut self, t: NodeType) {
        self.unit = Some(self.arena.new_node(t, self.options));
    }

    fn add_group(&mut self) -> Result<(), Error> {
        let group = self.group.expect("an open group");
        let concatenation = self.concatenation.expect("an open concatenation");
        let group_t = self.arena.nodes[group].t;
        if group_t == NT_TESTGROUP || group_t == NT_TESTREF {
            let left = self.arena.reverse_left(concatenation);
            self.arena.add_child(group, left);
            let n = self.arena.nodes[group].children.len();
            if (group_t == NT_TESTREF && n > 2) || n > 3 {
                return Err(self.get_err(ErrorCode::TooManyAlternates, Vec::new()));
            }
        } else {
            let alternation = self.alternation.expect("an open alternation");
            let left = self.arena.reverse_left(concatenation);
            self.arena.add_child(alternation, left);
            self.arena.add_child(group, alternation);
        }
        self.unit = Some(group);
        Ok(())
    }

    fn pop_keep_options(&mut self) {
        self.options_stack.pop();
    }

    fn pop_options(&mut self) {
        self.options = self.options_stack.pop().expect("saved options");
    }

    fn push_options(&mut self) {
        self.options_stack.push(self.options);
    }

    pub(crate) fn add_to_concatenate(&mut self, pos: isize, cch: isize, is_replacement: bool) {
        if cch == 0 {
            return;
        }
        let node = if cch > 1 {
            let mut str: Vec<Rune> = self.pattern[pos as usize..(pos + cch) as usize].to_vec();
            if self.use_option_i() && !is_replacement {
                for r in &mut str {
                    *r = unicode::to_lower(*r);
                }
            }
            self.arena
                .new_node_str(super::tree::NT_MULTI, self.options, str)
        } else {
            let mut ch = self.char_at(pos);
            if self.use_option_i() && !is_replacement {
                ch = unicode::to_lower(ch);
            }
            self.arena.new_node_ch(NT_ONE, self.options, ch)
        };
        let concatenation = self.concatenation.expect("an open concatenation");
        self.arena.add_child(concatenation, node);
    }

    fn push_group(&mut self) {
        let group = self.group.expect("an open group");
        let alternation = self.alternation.expect("an open alternation");
        let concatenation = self.concatenation.expect("an open concatenation");
        self.arena.nodes[group].next = self.stack;
        self.arena.nodes[alternation].next = Some(group);
        self.arena.nodes[concatenation].next = Some(alternation);
        self.stack = Some(concatenation);
    }

    fn pop_group(&mut self) -> Result<(), Error> {
        let concatenation = self.stack.expect("a pushed group");
        self.concatenation = Some(concatenation);
        let alternation = self.arena.nodes[concatenation]
            .next
            .expect("a pushed alternation");
        self.alternation = Some(alternation);
        let group = self.arena.nodes[alternation].next.expect("a pushed group");
        self.group = Some(group);
        self.stack = self.arena.nodes[group].next;
        if self.arena.nodes[group].t == NT_TESTGROUP && self.arena.nodes[group].children.is_empty()
        {
            let Some(unit) = self.unit.take() else {
                return Err(self.get_err(ErrorCode::ConditionalExpression, Vec::new()));
            };
            self.arena.add_child(group, unit);
        }
        Ok(())
    }

    fn empty_stack(&self) -> bool {
        self.stack.is_none()
    }

    fn start_group(&mut self, open_group: NodeId) {
        self.group = Some(open_group);
        self.alternation = Some(self.arena.new_node(NT_ALTERNATE, self.options));
        self.concatenation = Some(self.arena.new_node(NT_CONCATENATE, self.options));
    }

    fn add_alternate(&mut self) {
        let group = self.group.expect("an open group");
        let concatenation = self.concatenation.expect("an open concatenation");
        let group_t = self.arena.nodes[group].t;
        let left = self.arena.reverse_left(concatenation);
        if group_t == NT_TESTGROUP || group_t == NT_TESTREF {
            self.arena.add_child(group, left);
        } else {
            let alternation = self.alternation.expect("an open alternation");
            self.arena.add_child(alternation, left);
        }
        self.concatenation = Some(self.arena.new_node(NT_CONCATENATE, self.options));
    }

    fn is_true_quantifier(&self) -> bool {
        let mut n_chars = self.chars_right();
        if n_chars == 0 {
            return false;
        }
        let startpos = self.textpos();
        let mut ch = self.char_at(startpos);
        if ch != '{' as Rune {
            return ch <= '{' as Rune && CATEGORY[ch as usize] >= Q;
        }
        let mut pos = startpos;
        loop {
            n_chars -= 1;
            if n_chars <= 0 {
                break;
            }
            pos += 1;
            ch = self.char_at(pos);
            if !is_digit(ch) {
                break;
            }
        }
        if n_chars == 0 || pos - startpos == 1 {
            return false;
        }
        if ch == '}' as Rune {
            return true;
        }
        if ch != ',' as Rune {
            return false;
        }
        loop {
            n_chars -= 1;
            if n_chars <= 0 {
                break;
            }
            pos += 1;
            ch = self.char_at(pos);
            if !is_digit(ch) {
                break;
            }
        }
        n_chars > 0 && ch == '}' as Rune
    }
}

pub(crate) const REPLACE_LEFT_PORTION: isize = -1;
pub(crate) const REPLACE_RIGHT_PORTION: isize = -2;
pub(crate) const REPLACE_LAST_GROUP: isize = -3;
pub(crate) const REPLACE_WHOLE_STRING: isize = -4;

fn is_only_top_option(option: RegexOptions) -> bool {
    option == RIGHT_TO_LEFT || option == ECMASCRIPT || option == RE2
}

fn hex_digit(ch: Rune) -> i32 {
    let d = ch.wrapping_sub('0' as Rune);
    if (0..=9).contains(&d) {
        return d;
    }
    let d = ch.wrapping_sub('a' as Rune);
    if (0..=5).contains(&d) {
        return d + 0xa;
    }
    let d = ch.wrapping_sub('A' as Rune);
    if (0..=5).contains(&d) {
        return d + 0xa;
    }
    -1
}

const Q: u8 = 5;
const S: u8 = 4;
const Z: u8 = 3;
const X: u8 = 2;

const CATEGORY: [u8; 128] = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, X, X, X, X, X, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    X, 0, 0, Z, S, 0, 0, 0, S, S, Q, Q, 0, 0, S, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, Q,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, S, S, 0, S, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, Q, S, 0, 0, 0,
];

fn is_space(ch: Rune) -> bool {
    ch <= ' ' as Rune && CATEGORY[ch as usize] == X
}

fn is_special(ch: Rune) -> bool {
    ch <= '|' as Rune && CATEGORY[ch as usize] >= S
}

fn is_stopper_x(ch: Rune) -> bool {
    ch <= '|' as Rune && CATEGORY[ch as usize] >= X
}

fn is_quantifier(ch: Rune) -> bool {
    ch <= '{' as Rune && CATEGORY[ch as usize] >= Q
}
