use super::perl_groups::{CharGroup, perl_group, posix_group};
use super::regexp::{OP_PSEUDO, Op, Regexp};
use gocompat::unicode::{self, Range16, Range32, RangeTable};
use gocompat::utf8::{self, Rune};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub code: ErrorCode,
    pub expr: Vec<u8>,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code.reason())
    }
}

impl std::error::Error for Error {}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ErrorCode {
    InternalError,
    InvalidCharClass,
    InvalidCharRange,
    InvalidEscape,
    InvalidNamedCapture,
    InvalidPerlOp,
    InvalidRepeatOp,
    InvalidRepeatSize,
    InvalidUtf8,
    MissingBracket,
    MissingParen,
    MissingRepeatArgument,
    TrailingBackslash,
    UnexpectedParen,
    NestingDepth,
    Large,
}

impl ErrorCode {
    pub fn reason(self) -> &'static str {
        match self {
            ErrorCode::InternalError => "internal error",
            ErrorCode::InvalidCharClass => "invalid character class",
            ErrorCode::InvalidCharRange => "invalid character class range",
            ErrorCode::InvalidEscape => "invalid escape sequence",
            ErrorCode::InvalidNamedCapture => "invalid named capture",
            ErrorCode::InvalidPerlOp => "invalid or unsupported Perl syntax",
            ErrorCode::InvalidRepeatOp => "invalid nested repetition operator",
            ErrorCode::InvalidRepeatSize => "invalid repeat count",
            ErrorCode::InvalidUtf8 => "invalid UTF-8",
            ErrorCode::MissingBracket => "missing closing ]",
            ErrorCode::MissingParen => "missing closing )",
            ErrorCode::MissingRepeatArgument => "missing argument to repetition operator",
            ErrorCode::TrailingBackslash => "trailing backslash at end of expression",
            ErrorCode::UnexpectedParen => "unexpected )",
            ErrorCode::NestingDepth => "expression nests too deeply",
            ErrorCode::Large => "expression too large",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.reason())
    }
}

fn error(code: ErrorCode, expr: &[u8]) -> Error {
    Error {
        code,
        expr: expr.to_vec(),
    }
}

pub type Flags = u16;

pub const FOLD_CASE: Flags = 1;
pub const LITERAL: Flags = 1 << 1;
pub const CLASS_NL: Flags = 1 << 2;
pub const DOT_NL: Flags = 1 << 3;
pub const ONE_LINE: Flags = 1 << 4;
pub const NON_GREEDY: Flags = 1 << 5;
pub const PERL_X: Flags = 1 << 6;
pub const UNICODE_GROUPS: Flags = 1 << 7;
pub const WAS_DOLLAR: Flags = 1 << 8;
pub const SIMPLE: Flags = 1 << 9;
pub const MATCH_NL: Flags = CLASS_NL | DOT_NL;
pub const PERL: Flags = CLASS_NL | ONE_LINE | PERL_X | UNICODE_GROUPS;
pub const POSIX: Flags = 0;

pub const MAX_HEIGHT: usize = 1000;
pub const MAX_SIZE: i64 = (128 << 20) / INST_SIZE;
const INST_SIZE: i64 = 5 * 8;
pub const MAX_RUNES: usize = (128 << 20) / RUNE_SIZE;
const RUNE_SIZE: usize = 4;

pub const MIN_FOLD: Rune = 0x0041;
pub const MAX_FOLD: Rune = 0x1e943;

#[derive(Clone, Debug)]
struct Node {
    op: Op,
    flags: Flags,
    sub: Vec<usize>,
    rune: Vec<Rune>,
    min: i32,
    max: i32,
    cap: i32,
    name: Vec<u8>,
}

impl Node {
    fn new(op: Op) -> Node {
        Node {
            op,
            flags: 0,
            sub: Vec::new(),
            rune: Vec::new(),
            min: 0,
            max: 0,
            cap: 0,
            name: Vec::new(),
        }
    }
}

struct Parser<'a> {
    flags: Flags,
    stack: Vec<usize>,
    free: Vec<usize>,
    nodes: Vec<Node>,
    num_cap: i32,
    whole_regexp: &'a [u8],
    tmp_class: Vec<Rune>,
    num_regexp: usize,
    num_runes: usize,
    repeats: i64,
    height: Option<Vec<i32>>,
    size: Option<Vec<i64>>,
}

impl<'a> Parser<'a> {
    fn new(flags: Flags, whole: &'a [u8]) -> Parser<'a> {
        Parser {
            flags,
            stack: Vec::new(),
            free: Vec::new(),
            nodes: Vec::new(),
            num_cap: 0,
            whole_regexp: whole,
            tmp_class: Vec::new(),
            num_regexp: 0,
            num_runes: 0,
            repeats: 0,
            height: None,
            size: None,
        }
    }

    fn new_regexp(&mut self, op: Op) -> usize {
        if let Some(id) = self.free.pop() {
            self.nodes[id] = Node::new(op);
            id
        } else {
            self.nodes.push(Node::new(op));
            self.num_regexp += 1;
            self.nodes.len() - 1
        }
    }

    fn reuse(&mut self, re: usize) {
        if let Some(height) = &mut self.height
            && re < height.len()
        {
            height[re] = 0;
        }
        self.free.push(re);
    }

    fn large(&self) -> Error {
        error(ErrorCode::Large, self.whole_regexp)
    }

    fn check_limits(&mut self, re: usize) -> Result<(), Error> {
        if self.num_runes > MAX_RUNES {
            return Err(self.large());
        }
        self.check_size(re)?;
        self.check_height(re)
    }

    fn check_size(&mut self, re: usize) -> Result<(), Error> {
        if self.size.is_none() {
            if self.repeats == 0 {
                self.repeats = 1;
            }
            if self.nodes[re].op == Op::Repeat {
                let mut n = self.nodes[re].max;
                if n == -1 {
                    n = self.nodes[re].min;
                }
                if n <= 0 {
                    n = 1;
                }
                if i64::from(n) > MAX_SIZE / self.repeats {
                    self.repeats = MAX_SIZE;
                } else {
                    self.repeats *= i64::from(n);
                }
            }
            if (self.num_regexp as i64) < MAX_SIZE / self.repeats {
                return Ok(());
            }
            self.size = Some(Vec::new());
            for k in 0..self.stack.len() {
                let id = self.stack[k];
                self.check_size(id)?;
            }
        }
        if self.calc_size(re, true) > MAX_SIZE {
            return Err(self.large());
        }
        Ok(())
    }

    fn size_memo(&self, re: usize) -> Option<i64> {
        match &self.size {
            Some(m) if re < m.len() && m[re] != 0 => Some(m[re]),
            _ => None,
        }
    }

    fn calc_size(&mut self, re: usize, force: bool) -> i64 {
        if !force && let Some(size) = self.size_memo(re) {
            return size;
        }
        let mut size: i64 = 0;
        match self.nodes[re].op {
            Op::Literal => size = self.nodes[re].rune.len() as i64,
            Op::Capture | Op::Star => {
                let sub = self.nodes[re].sub[0];
                size = 2i64.wrapping_add(self.calc_size(sub, false));
            }
            Op::Plus | Op::Quest => {
                let sub = self.nodes[re].sub[0];
                size = 1i64.wrapping_add(self.calc_size(sub, false));
            }
            Op::Concat => {
                for k in 0..self.nodes[re].sub.len() {
                    let sub = self.nodes[re].sub[k];
                    size = size.wrapping_add(self.calc_size(sub, false));
                }
            }
            Op::Alternate => {
                let n = self.nodes[re].sub.len();
                for k in 0..n {
                    let sub = self.nodes[re].sub[k];
                    size = size.wrapping_add(self.calc_size(sub, false));
                }
                if n > 1 {
                    size = size.wrapping_add(n as i64 - 1);
                }
            }
            Op::Repeat => {
                let sub = self.nodes[re].sub[0];
                let subsize = self.calc_size(sub, false);
                let (min, max) = (i64::from(self.nodes[re].min), i64::from(self.nodes[re].max));
                if max == -1 {
                    if min == 0 {
                        size = 2i64.wrapping_add(subsize);
                    } else {
                        size = 1i64.wrapping_add(min.wrapping_mul(subsize));
                    }
                } else {
                    size = max.wrapping_mul(subsize).wrapping_add(max - min);
                }
            }
            _ => {}
        }
        let size = size.max(1);
        if let Some(memo) = &mut self.size {
            if memo.len() <= re {
                memo.resize(re + 1, 0);
            }
            memo[re] = size;
        }
        size
    }

    fn check_height(&mut self, re: usize) -> Result<(), Error> {
        if self.num_regexp < MAX_HEIGHT {
            return Ok(());
        }
        if self.height.is_none() {
            self.height = Some(Vec::new());
            for k in 0..self.stack.len() {
                let id = self.stack[k];
                self.check_height(id)?;
            }
        }
        if self.calc_height(re, true) > MAX_HEIGHT as i32 {
            return Err(error(ErrorCode::NestingDepth, self.whole_regexp));
        }
        Ok(())
    }

    fn calc_height(&mut self, re: usize, force: bool) -> i32 {
        if !force
            && let Some(memo) = &self.height
            && re < memo.len()
            && memo[re] != 0
        {
            return memo[re];
        }
        let mut h = 1;
        for k in 0..self.nodes[re].sub.len() {
            let sub = self.nodes[re].sub[k];
            let hsub = self.calc_height(sub, false);
            if h < 1 + hsub {
                h = 1 + hsub;
            }
        }
        if let Some(memo) = &mut self.height {
            if memo.len() <= re {
                memo.resize(re + 1, 0);
            }
            memo[re] = h;
        }
        h
    }

    fn push(&mut self, re: usize) -> Result<Option<usize>, Error> {
        self.num_runes += self.nodes[re].rune.len();
        let node = &self.nodes[re];
        let r = &node.rune;
        if node.op == Op::CharClass && r.len() == 2 && r[0] == r[1] {
            let r0 = r[0];
            if self.maybe_concat(r0, self.flags & !FOLD_CASE) {
                return Ok(None);
            }
            let flags = self.flags & !FOLD_CASE;
            let node = &mut self.nodes[re];
            node.op = Op::Literal;
            node.rune.truncate(1);
            node.flags = flags;
        } else if node.op == Op::CharClass
            && r.len() == 4
            && r[0] == r[1]
            && r[2] == r[3]
            && unicode::simple_fold(r[0]) == r[2]
            && unicode::simple_fold(r[2]) == r[0]
            || node.op == Op::CharClass
                && r.len() == 2
                && r[0] + 1 == r[1]
                && unicode::simple_fold(r[0]) == r[1]
                && unicode::simple_fold(r[1]) == r[0]
        {
            let r0 = r[0];
            if self.maybe_concat(r0, self.flags | FOLD_CASE) {
                return Ok(None);
            }
            let flags = self.flags | FOLD_CASE;
            let node = &mut self.nodes[re];
            node.op = Op::Literal;
            node.rune.truncate(1);
            node.flags = flags;
        } else {
            self.maybe_concat(-1, 0);
        }
        self.stack.push(re);
        self.check_limits(re)?;
        Ok(Some(re))
    }

    fn maybe_concat(&mut self, r: Rune, flags: Flags) -> bool {
        let n = self.stack.len();
        if n < 2 {
            return false;
        }
        let re1 = self.stack[n - 1];
        let re2 = self.stack[n - 2];
        if self.nodes[re1].op != Op::Literal
            || self.nodes[re2].op != Op::Literal
            || self.nodes[re1].flags & FOLD_CASE != self.nodes[re2].flags & FOLD_CASE
        {
            return false;
        }
        let moved = std::mem::take(&mut self.nodes[re1].rune);
        self.nodes[re2].rune.extend_from_slice(&moved);
        if r >= 0 {
            let node = &mut self.nodes[re1];
            node.rune = moved;
            node.rune.clear();
            node.rune.push(r);
            node.flags = flags;
            return true;
        }
        self.nodes[re1].rune = moved;
        self.stack.pop();
        self.reuse(re1);
        false
    }

    fn literal(&mut self, r: Rune) -> Result<(), Error> {
        let re = self.new_regexp(Op::Literal);
        self.nodes[re].flags = self.flags;
        let r = if self.flags & FOLD_CASE != 0 {
            min_fold_rune(r)
        } else {
            r
        };
        self.nodes[re].rune.push(r);
        self.push(re)?;
        Ok(())
    }

    fn op(&mut self, op: Op) -> Result<usize, Error> {
        let re = self.new_regexp(op);
        self.nodes[re].flags = self.flags;
        Ok(self.push(re)?.unwrap_or(re))
    }

    fn repeat(
        &mut self,
        op: Op,
        min: i32,
        max: i32,
        before: usize,
        after: usize,
        last_repeat: Option<usize>,
    ) -> Result<usize, Error> {
        let s = self.whole_regexp;
        let mut after = after;
        let mut flags = self.flags;
        if self.flags & PERL_X != 0 {
            if after < s.len() && s[after] == b'?' {
                after += 1;
                flags ^= NON_GREEDY;
            }
            if let Some(last) = last_repeat {
                return Err(error(ErrorCode::InvalidRepeatOp, &s[last..after]));
            }
        }
        let n = self.stack.len();
        if n == 0 {
            return Err(error(ErrorCode::MissingRepeatArgument, &s[before..after]));
        }
        let sub = self.stack[n - 1];
        if self.nodes[sub].op >= OP_PSEUDO {
            return Err(error(ErrorCode::MissingRepeatArgument, &s[before..after]));
        }
        let re = self.new_regexp(op);
        {
            let node = &mut self.nodes[re];
            node.min = min;
            node.max = max;
            node.flags = flags;
            node.sub.push(sub);
        }
        self.stack[n - 1] = re;
        self.check_limits(re)?;
        if op == Op::Repeat && (min >= 2 || max >= 2) && !self.repeat_is_valid(re, 1000) {
            return Err(error(ErrorCode::InvalidRepeatSize, &s[before..after]));
        }
        Ok(after)
    }

    fn repeat_is_valid(&self, re: usize, n: i32) -> bool {
        let mut n = n;
        let node = &self.nodes[re];
        if node.op == Op::Repeat {
            let mut m = node.max;
            if m == 0 {
                return true;
            }
            if m < 0 {
                m = node.min;
            }
            if m > n {
                return false;
            }
            if m > 0 {
                n /= m;
            }
        }
        node.sub.iter().all(|&sub| self.repeat_is_valid(sub, n))
    }

    fn concat(&mut self) -> Result<Option<usize>, Error> {
        self.maybe_concat(-1, 0);
        let mut i = self.stack.len();
        while i > 0 && self.nodes[self.stack[i - 1]].op < OP_PSEUDO {
            i -= 1;
        }
        let subs = self.stack.split_off(i);
        if subs.is_empty() {
            let re = self.new_regexp(Op::EmptyMatch);
            return self.push(re);
        }
        let re = self.collapse(subs, Op::Concat)?;
        self.push(re)
    }

    fn alternate(&mut self) -> Result<Option<usize>, Error> {
        let mut i = self.stack.len();
        while i > 0 && self.nodes[self.stack[i - 1]].op < OP_PSEUDO {
            i -= 1;
        }
        let subs = self.stack.split_off(i);
        if let Some(&last) = subs.last() {
            self.clean_alt(last);
        }
        if subs.is_empty() {
            let re = self.new_regexp(Op::NoMatch);
            return self.push(re);
        }
        let re = self.collapse(subs, Op::Alternate)?;
        self.push(re)
    }

    fn clean_alt(&mut self, re: usize) {
        let node = &mut self.nodes[re];
        if node.op == Op::CharClass {
            clean_class(&mut node.rune);
            let r = &node.rune;
            if r.len() == 2 && r[0] == 0 && r[1] == unicode::MAX_RUNE {
                node.rune = Vec::new();
                node.op = Op::AnyChar;
                return;
            }
            if r.len() == 4
                && r[0] == 0
                && r[1] == '\n' as Rune - 1
                && r[2] == '\n' as Rune + 1
                && r[3] == unicode::MAX_RUNE
            {
                node.rune = Vec::new();
                node.op = Op::AnyCharNotNL;
            }
        }
    }

    fn collapse(&mut self, subs: Vec<usize>, op: Op) -> Result<usize, Error> {
        if subs.len() == 1 {
            return Ok(subs[0]);
        }
        let mut re = self.new_regexp(op);
        let mut list = Vec::with_capacity(subs.len());
        for sub in subs {
            if self.nodes[sub].op == op {
                list.extend_from_slice(&self.nodes[sub].sub);
                self.reuse(sub);
            } else {
                list.push(sub);
            }
        }
        if op == Op::Alternate {
            list = self.factor(list)?;
            if list.len() == 1 {
                let old = re;
                re = list[0];
                self.nodes[old].sub = list;
                self.reuse(old);
                return Ok(re);
            }
        }
        self.nodes[re].sub = list;
        Ok(re)
    }

    fn factor(&mut self, sub: Vec<usize>) -> Result<Vec<usize>, Error> {
        if sub.len() < 2 {
            return Ok(sub);
        }
        let mut sub = sub;

        let mut str_: Vec<Rune> = Vec::new();
        let mut strflags: Flags = 0;
        let mut start = 0;
        let mut out: Vec<usize> = Vec::with_capacity(sub.len());
        let mut i = 0;
        while i <= sub.len() {
            let mut istr: Vec<Rune> = Vec::new();
            let mut iflags: Flags = 0;
            if i < sub.len() {
                (istr, iflags) = self.leading_string(sub[i]);
                if iflags == strflags {
                    let mut same = 0;
                    while same < str_.len() && same < istr.len() && str_[same] == istr[same] {
                        same += 1;
                    }
                    if same > 0 {
                        str_.truncate(same);
                        i += 1;
                        continue;
                    }
                }
            }
            if i == start {
            } else if i == start + 1 {
                out.push(sub[start]);
            } else {
                let prefix = self.new_regexp(Op::Literal);
                self.nodes[prefix].flags = strflags;
                self.nodes[prefix].rune = str_.clone();
                let n = str_.len();
                for item in sub[start..i].iter_mut() {
                    *item = self.remove_leading_string(*item, n);
                    self.check_limits(*item)?;
                }
                let suffix = self.collapse(sub[start..i].to_vec(), Op::Alternate)?;
                let re = self.new_regexp(Op::Concat);
                self.nodes[re].sub = vec![prefix, suffix];
                out.push(re);
            }
            start = i;
            str_ = istr;
            strflags = iflags;
            i += 1;
        }
        sub = out;

        start = 0;
        let mut out: Vec<usize> = Vec::with_capacity(sub.len());
        let mut first: Option<usize> = None;
        let mut i = 0;
        while i <= sub.len() {
            let mut ifirst: Option<usize> = None;
            if i < sub.len() {
                ifirst = self.leading_regexp(sub[i]);
                if let Some(f) = first
                    && let Some(fi) = ifirst
                    && self.equal(f, fi)
                    && (self.is_char_class(f)
                        || (self.nodes[f].op == Op::Repeat
                            && self.nodes[f].min == self.nodes[f].max
                            && self.is_char_class(self.nodes[f].sub[0])))
                {
                    i += 1;
                    continue;
                }
            }
            if i == start {
            } else if i == start + 1 {
                out.push(sub[start]);
            } else {
                let prefix = first.expect("leading regexp");
                for (k, item) in sub[start..i].iter_mut().enumerate() {
                    let reuse = k != 0;
                    *item = self.remove_leading_regexp(*item, reuse);
                    self.check_limits(*item)?;
                }
                let suffix = self.collapse(sub[start..i].to_vec(), Op::Alternate)?;
                let re = self.new_regexp(Op::Concat);
                self.nodes[re].sub = vec![prefix, suffix];
                out.push(re);
            }
            start = i;
            first = ifirst;
            i += 1;
        }
        sub = out;

        start = 0;
        let mut out: Vec<usize> = Vec::with_capacity(sub.len());
        let mut i = 0;
        while i <= sub.len() {
            if i < sub.len() && self.is_char_class(sub[i]) {
                i += 1;
                continue;
            }
            if i == start {
            } else if i == start + 1 {
                out.push(sub[start]);
            } else {
                let mut max = start;
                for j in start + 1..i {
                    let (a, b) = (&self.nodes[sub[max]], &self.nodes[sub[j]]);
                    if a.op < b.op || a.op == b.op && a.rune.len() < b.rune.len() {
                        max = j;
                    }
                }
                sub.swap(start, max);
                for j in start + 1..i {
                    self.merge_char_class(sub[start], sub[j]);
                    self.reuse(sub[j]);
                }
                self.clean_alt(sub[start]);
                out.push(sub[start]);
            }
            if i < sub.len() {
                out.push(sub[i]);
            }
            start = i + 1;
            i += 1;
        }
        sub = out;

        let mut out: Vec<usize> = Vec::with_capacity(sub.len());
        for i in 0..sub.len() {
            if i + 1 < sub.len()
                && self.nodes[sub[i]].op == Op::EmptyMatch
                && self.nodes[sub[i + 1]].op == Op::EmptyMatch
            {
                continue;
            }
            out.push(sub[i]);
        }
        Ok(out)
    }

    fn leading_string(&self, re: usize) -> (Vec<Rune>, Flags) {
        let mut re = re;
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            re = self.nodes[re].sub[0];
        }
        if self.nodes[re].op != Op::Literal {
            return (Vec::new(), 0);
        }
        (
            self.nodes[re].rune.clone(),
            self.nodes[re].flags & FOLD_CASE,
        )
    }

    fn remove_leading_string(&mut self, re: usize, n: usize) -> usize {
        let mut re = re;
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            let sub = self.nodes[re].sub[0];
            let sub = self.remove_leading_string(sub, n);
            self.nodes[re].sub[0] = sub;
            if self.nodes[sub].op == Op::EmptyMatch {
                self.reuse(sub);
                match self.nodes[re].sub.len() {
                    0 | 1 => {
                        self.nodes[re].op = Op::EmptyMatch;
                        self.nodes[re].sub = Vec::new();
                    }
                    2 => {
                        let old = re;
                        re = self.nodes[re].sub[1];
                        self.reuse(old);
                    }
                    _ => {
                        self.nodes[re].sub.remove(0);
                    }
                }
            }
            return re;
        }
        if self.nodes[re].op == Op::Literal {
            let node = &mut self.nodes[re];
            node.rune.drain(..n);
            if node.rune.is_empty() {
                node.op = Op::EmptyMatch;
            }
        }
        re
    }

    fn leading_regexp(&self, re: usize) -> Option<usize> {
        if self.nodes[re].op == Op::EmptyMatch {
            return None;
        }
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            let sub = self.nodes[re].sub[0];
            if self.nodes[sub].op == Op::EmptyMatch {
                return None;
            }
            return Some(sub);
        }
        Some(re)
    }

    fn remove_leading_regexp(&mut self, re: usize, reuse: bool) -> usize {
        if self.nodes[re].op == Op::Concat && !self.nodes[re].sub.is_empty() {
            if reuse {
                let first = self.nodes[re].sub[0];
                self.reuse(first);
            }
            self.nodes[re].sub.remove(0);
            match self.nodes[re].sub.len() {
                0 => {
                    self.nodes[re].op = Op::EmptyMatch;
                    self.nodes[re].sub = Vec::new();
                }
                1 => {
                    let old = re;
                    let re = self.nodes[re].sub[0];
                    self.reuse(old);
                    return re;
                }
                _ => {}
            }
            return re;
        }
        if reuse {
            self.reuse(re);
        }
        self.new_regexp(Op::EmptyMatch)
    }

    fn equal(&self, x: usize, y: usize) -> bool {
        let (a, b) = (&self.nodes[x], &self.nodes[y]);
        if a.op != b.op {
            return false;
        }
        match a.op {
            Op::EndText => {
                if a.flags & WAS_DOLLAR != b.flags & WAS_DOLLAR {
                    return false;
                }
            }
            Op::Literal | Op::CharClass => return a.rune == b.rune,
            Op::Alternate | Op::Concat => {
                return a.sub.len() == b.sub.len()
                    && a.sub
                        .iter()
                        .zip(b.sub.iter())
                        .all(|(&p, &q)| self.equal(p, q));
            }
            Op::Star | Op::Plus | Op::Quest => {
                if a.flags & NON_GREEDY != b.flags & NON_GREEDY || !self.equal(a.sub[0], b.sub[0]) {
                    return false;
                }
            }
            Op::Repeat => {
                if a.flags & NON_GREEDY != b.flags & NON_GREEDY
                    || a.min != b.min
                    || a.max != b.max
                    || !self.equal(a.sub[0], b.sub[0])
                {
                    return false;
                }
            }
            Op::Capture => {
                if a.cap != b.cap || a.name != b.name || !self.equal(a.sub[0], b.sub[0]) {
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    fn is_char_class(&self, re: usize) -> bool {
        let node = &self.nodes[re];
        node.op == Op::Literal && node.rune.len() == 1
            || node.op == Op::CharClass
            || node.op == Op::AnyCharNotNL
            || node.op == Op::AnyChar
    }

    fn match_rune(&self, re: usize, r: Rune) -> bool {
        let node = &self.nodes[re];
        match node.op {
            Op::Literal => node.rune.len() == 1 && node.rune[0] == r,
            Op::CharClass => node.rune.chunks_exact(2).any(|p| p[0] <= r && r <= p[1]),
            Op::AnyCharNotNL => r != '\n' as Rune,
            Op::AnyChar => true,
            _ => false,
        }
    }

    fn parse_vertical_bar(&mut self) -> Result<(), Error> {
        self.concat()?;
        if !self.swap_vertical_bar() {
            self.op(Op::VerticalBar)?;
        }
        Ok(())
    }

    fn merge_char_class(&mut self, dst: usize, src: usize) {
        let src_op = self.nodes[src].op;
        let src_flags = self.nodes[src].flags;
        match self.nodes[dst].op {
            Op::AnyChar => {}
            Op::AnyCharNotNL => {
                if self.match_rune(src, '\n' as Rune) {
                    self.nodes[dst].op = Op::AnyChar;
                }
            }
            Op::CharClass => {
                if src_op == Op::Literal {
                    let r0 = self.nodes[src].rune[0];
                    append_literal(&mut self.nodes[dst].rune, r0, src_flags);
                } else {
                    let src_rune = std::mem::take(&mut self.nodes[src].rune);
                    append_class(&mut self.nodes[dst].rune, &src_rune);
                    self.nodes[src].rune = src_rune;
                }
            }
            Op::Literal => {
                let s0 = self.nodes[src].rune[0];
                let node = &mut self.nodes[dst];
                if s0 == node.rune[0] && src_flags == node.flags {
                    return;
                }
                node.op = Op::CharClass;
                let d0 = node.rune[0];
                let dflags = node.flags;
                node.rune.clear();
                append_literal(&mut node.rune, d0, dflags);
                append_literal(&mut node.rune, s0, src_flags);
            }
            _ => {}
        }
    }

    fn swap_vertical_bar(&mut self) -> bool {
        let n = self.stack.len();
        if n >= 3
            && self.nodes[self.stack[n - 2]].op == Op::VerticalBar
            && self.is_char_class(self.stack[n - 1])
            && self.is_char_class(self.stack[n - 3])
        {
            let mut re1 = self.stack[n - 1];
            let mut re3 = self.stack[n - 3];
            if self.nodes[re1].op > self.nodes[re3].op {
                std::mem::swap(&mut re1, &mut re3);
                self.stack[n - 3] = re3;
            }
            self.merge_char_class(re3, re1);
            self.reuse(re1);
            self.stack.truncate(n - 1);
            return true;
        }
        if n >= 2 {
            let re1 = self.stack[n - 1];
            let re2 = self.stack[n - 2];
            if self.nodes[re2].op == Op::VerticalBar {
                if n >= 3 {
                    self.clean_alt(self.stack[n - 3]);
                }
                self.stack[n - 2] = re1;
                self.stack[n - 1] = re2;
                return true;
            }
        }
        false
    }

    fn parse_right_paren(&mut self) -> Result<(), Error> {
        self.concat()?;
        if self.swap_vertical_bar() {
            self.stack.pop();
        }
        self.alternate()?;
        let n = self.stack.len();
        if n < 2 {
            return Err(error(ErrorCode::UnexpectedParen, self.whole_regexp));
        }
        let re1 = self.stack[n - 1];
        let re2 = self.stack[n - 2];
        self.stack.truncate(n - 2);
        if self.nodes[re2].op != Op::LeftParen {
            return Err(error(ErrorCode::UnexpectedParen, self.whole_regexp));
        }
        self.flags = self.nodes[re2].flags;
        if self.nodes[re2].cap == 0 {
            self.push(re1)?;
        } else {
            let node = &mut self.nodes[re2];
            node.op = Op::Capture;
            node.sub = vec![re1];
            self.push(re2)?;
        }
        Ok(())
    }

    fn parse_repeat(&self, start: usize) -> Option<(i32, i32, usize)> {
        let s = self.whole_regexp;
        if start >= s.len() || s[start] != b'{' {
            return None;
        }
        let mut t = start + 1;
        let (mut min, rest) = parse_int(s, t)?;
        t = rest;
        if t >= s.len() {
            return None;
        }
        let max;
        if s[t] != b',' {
            max = min;
        } else {
            t += 1;
            if t >= s.len() {
                return None;
            }
            if s[t] == b'}' {
                max = -1;
            } else {
                let (m, rest) = parse_int(s, t)?;
                t = rest;
                max = m;
                if max < 0 {
                    min = -1;
                }
            }
        }
        if t >= s.len() || s[t] != b'}' {
            return None;
        }
        Some((min, max, t + 1))
    }

    fn parse_perl_flags(&mut self, start: usize) -> Result<usize, Error> {
        let whole = self.whole_regexp;
        let s = &whole[start..];
        let t = s;
        let starts_with_p = t.len() > 4 && t[2] == b'P' && t[3] == b'<';
        let starts_with_name = t.len() > 3 && t[2] == b'<';
        if starts_with_p || starts_with_name {
            let expr_start_pos = if starts_with_name { 3 } else { 4 };
            let end = match t.iter().position(|&c| c == b'>') {
                Some(end) => end,
                None => {
                    check_utf8(t)?;
                    return Err(error(ErrorCode::InvalidNamedCapture, s));
                }
            };
            let capture = &t[..end + 1];
            let name = &t[expr_start_pos..end];
            check_utf8(name)?;
            if !is_valid_capture_name(name) {
                return Err(error(ErrorCode::InvalidNamedCapture, capture));
            }
            self.num_cap += 1;
            let re = self.op(Op::LeftParen)?;
            self.nodes[re].cap = self.num_cap;
            self.nodes[re].name = name.to_vec();
            return Ok(start + end + 1);
        }
        let mut t = 2;
        let mut flags = self.flags;
        let mut sign = 1;
        let mut saw_flag = false;
        while t < s.len() {
            let (c, size) = next_rune(&s[t..])?;
            t += size;
            match c {
                0x69 => {
                    flags |= FOLD_CASE;
                    saw_flag = true;
                }
                0x6D => {
                    flags &= !ONE_LINE;
                    saw_flag = true;
                }
                0x73 => {
                    flags |= DOT_NL;
                    saw_flag = true;
                }
                0x55 => {
                    flags |= NON_GREEDY;
                    saw_flag = true;
                }
                0x2D => {
                    if sign < 0 {
                        break;
                    }
                    sign = -1;
                    flags = !flags;
                    saw_flag = false;
                }
                0x3A | 0x29 => {
                    if sign < 0 {
                        if !saw_flag {
                            break;
                        }
                        flags = !flags;
                    }
                    if c == 0x3A {
                        self.op(Op::LeftParen)?;
                    }
                    self.flags = flags;
                    return Ok(start + t);
                }
                _ => break,
            }
        }
        Err(error(ErrorCode::InvalidPerlOp, &s[..t]))
    }

    fn parse_escape(&self, start: usize) -> Result<(Rune, usize), Error> {
        let s = &self.whole_regexp[start..];
        parse_escape(s).map(|(r, n)| (r, start + n))
    }

    fn parse_class_char(&self, start: usize, whole_class: usize) -> Result<(Rune, usize), Error> {
        let s = self.whole_regexp;
        if start >= s.len() {
            return Err(error(ErrorCode::MissingBracket, &s[whole_class..]));
        }
        if s[start] == b'\\' {
            return self.parse_escape(start);
        }
        let (c, size) = next_rune(&s[start..])?;
        Ok((c, start + size))
    }

    fn parse_perl_class_escape(&mut self, start: usize, r: &mut Vec<Rune>) -> Option<usize> {
        let s = &self.whole_regexp[start..];
        if self.flags & PERL_X == 0 || s.len() < 2 || s[0] != b'\\' {
            return None;
        }
        let g = perl_group(&s[..2])?;
        self.append_group(r, g);
        Some(start + 2)
    }

    fn parse_named_class(
        &mut self,
        start: usize,
        r: &mut Vec<Rune>,
    ) -> Result<Option<usize>, Error> {
        let s = &self.whole_regexp[start..];
        if s.len() < 2 || s[0] != b'[' || s[1] != b':' {
            return Ok(None);
        }
        let i = match s[2..].windows(2).position(|w| w == b":]") {
            Some(i) => i + 2,
            None => return Ok(None),
        };
        let name = &s[..i + 2];
        let g = match posix_group(name) {
            Some(g) => g,
            None => return Err(error(ErrorCode::InvalidCharRange, name)),
        };
        self.append_group(r, g);
        Ok(Some(start + i + 2))
    }

    fn append_group(&mut self, r: &mut Vec<Rune>, g: CharGroup) {
        if self.flags & FOLD_CASE == 0 {
            if g.sign < 0 {
                append_negated_class(r, g.class);
            } else {
                append_class(r, g.class);
            }
        } else {
            let mut tmp = std::mem::take(&mut self.tmp_class);
            tmp.clear();
            append_folded_class(&mut tmp, g.class);
            clean_class(&mut tmp);
            if g.sign < 0 {
                append_negated_class(r, &tmp);
            } else {
                append_class(r, &tmp);
            }
            self.tmp_class = tmp;
        }
    }

    fn parse_unicode_class(
        &mut self,
        start: usize,
        r: &mut Vec<Rune>,
    ) -> Result<Option<usize>, Error> {
        let whole = self.whole_regexp;
        let s = &whole[start..];
        if self.flags & UNICODE_GROUPS == 0
            || s.len() < 2
            || s[0] != b'\\'
            || s[1] != b'p' && s[1] != b'P'
        {
            return Ok(None);
        }
        let mut sign = if s[1] == b'P' { -1 } else { 1 };
        let (c, size) = next_rune(&s[2..])?;
        let t;
        let seq;
        let mut name;
        if c != '{' as Rune {
            t = 2 + size;
            seq = &s[..t];
            name = &seq[2..];
        } else {
            let end = match s.iter().position(|&b| b == b'}') {
                Some(end) => end,
                None => {
                    check_utf8(s)?;
                    return Err(error(ErrorCode::InvalidCharRange, s));
                }
            };
            seq = &s[..end + 1];
            t = end + 1;
            name = &s[3..end];
            check_utf8(name)?;
        }
        if !name.is_empty() && name[0] == b'^' {
            sign = -sign;
            name = &name[1..];
        }
        let (tab, fold) = match unicode_table(name) {
            Some(found) => found,
            None => return Err(error(ErrorCode::InvalidCharRange, seq)),
        };
        if self.flags & FOLD_CASE == 0 || fold.is_none() {
            if sign > 0 {
                append_table(r, tab);
            } else {
                append_negated_table(r, tab);
            }
        } else {
            let mut tmp = std::mem::take(&mut self.tmp_class);
            tmp.clear();
            append_table(&mut tmp, tab);
            if let Some(fold) = fold {
                append_table(&mut tmp, fold);
            }
            clean_class(&mut tmp);
            if sign > 0 {
                append_class(r, &tmp);
            } else {
                append_negated_class(r, &tmp);
            }
            self.tmp_class = tmp;
        }
        Ok(Some(start + t))
    }

    fn parse_class(&mut self, start: usize) -> Result<usize, Error> {
        let whole = self.whole_regexp;
        let mut t = start + 1;
        let re = self.new_regexp(Op::CharClass);
        self.nodes[re].flags = self.flags;
        let mut class: Vec<Rune> = Vec::new();
        let mut sign = 1;
        if t < whole.len() && whole[t] == b'^' {
            sign = -1;
            t += 1;
            if self.flags & CLASS_NL == 0 {
                class.push('\n' as Rune);
                class.push('\n' as Rune);
            }
        }
        let mut first = true;
        while t >= whole.len() || whole[t] != b']' || first {
            if t < whole.len()
                && whole[t] == b'-'
                && self.flags & PERL_X == 0
                && !first
                && (whole.len() - t == 1 || whole[t + 1] != b']')
            {
                let (_, size) = utf8::decode_rune(&whole[t + 1..]);
                return Err(error(ErrorCode::InvalidCharRange, &whole[t..t + 1 + size]));
            }
            first = false;
            if whole.len() > t + 2
                && whole[t] == b'['
                && whole[t + 1] == b':'
                && let Some(nt) = self.parse_named_class(t, &mut class)?
            {
                t = nt;
                continue;
            }
            if let Some(nt) = self.parse_unicode_class(t, &mut class)? {
                t = nt;
                continue;
            }
            if let Some(nt) = self.parse_perl_class_escape(t, &mut class) {
                t = nt;
                continue;
            }
            let rng = t;
            let (lo, nt) = self.parse_class_char(t, start)?;
            t = nt;
            let mut hi = lo;
            if whole.len() >= t + 2 && whole[t] == b'-' && whole[t + 1] != b']' {
                t += 1;
                let (h, nt) = self.parse_class_char(t, start)?;
                hi = h;
                t = nt;
                if hi < lo {
                    return Err(error(ErrorCode::InvalidCharRange, &whole[rng..t]));
                }
            }
            if self.flags & FOLD_CASE == 0 {
                append_range(&mut class, lo, hi);
            } else {
                append_folded_range(&mut class, lo, hi);
            }
        }
        t += 1;
        clean_class(&mut class);
        if sign < 0 {
            negate_class(&mut class);
        }
        self.nodes[re].rune = class;
        self.push(re)?;
        Ok(t)
    }

    fn build(&self, id: usize) -> Arc<Regexp> {
        let node = &self.nodes[id];
        Arc::new(Regexp {
            op: node.op,
            flags: node.flags,
            sub: node.sub.iter().map(|&s| self.build(s)).collect(),
            rune: Arc::from(node.rune.as_slice()),
            min: node.min,
            max: node.max,
            cap: node.cap,
            name: node.name.clone(),
        })
    }
}

pub fn min_fold_rune(r: Rune) -> Rune {
    if !(MIN_FOLD..=MAX_FOLD).contains(&r) {
        return r;
    }
    let mut m = r;
    let r0 = r;
    let mut r = unicode::simple_fold(r);
    while r != r0 {
        m = m.min(r);
        r = unicode::simple_fold(r);
    }
    m
}

fn literal_regexp(s: &[u8], flags: Flags) -> Regexp {
    let mut re = Regexp::new(Op::Literal);
    re.flags = flags;
    let mut runes = Vec::new();
    let mut i = 0;
    while i < s.len() {
        let (c, size) = utf8::decode_rune(&s[i..]);
        runes.push(c);
        i += size;
    }
    re.rune = Arc::from(runes);
    re
}

pub fn parse(s: &[u8], flags: Flags) -> Result<Arc<Regexp>, Error> {
    if flags & LITERAL != 0 {
        check_utf8(s)?;
        return Ok(Arc::new(literal_regexp(s, flags)));
    }
    let mut p = Parser::new(flags, s);
    let mut t = 0;
    let mut last_repeat: Option<usize> = None;
    while t < s.len() {
        let mut repeat: Option<usize> = None;
        'big: {
            match s[t] {
                b'(' => {
                    if p.flags & PERL_X != 0 && s.len() - t >= 2 && s[t + 1] == b'?' {
                        t = p.parse_perl_flags(t)?;
                        break 'big;
                    }
                    p.num_cap += 1;
                    let re = p.op(Op::LeftParen)?;
                    p.nodes[re].cap = p.num_cap;
                    t += 1;
                }
                b'|' => {
                    p.parse_vertical_bar()?;
                    t += 1;
                }
                b')' => {
                    p.parse_right_paren()?;
                    t += 1;
                }
                b'^' => {
                    if p.flags & ONE_LINE != 0 {
                        p.op(Op::BeginText)?;
                    } else {
                        p.op(Op::BeginLine)?;
                    }
                    t += 1;
                }
                b'$' => {
                    if p.flags & ONE_LINE != 0 {
                        let re = p.op(Op::EndText)?;
                        p.nodes[re].flags |= WAS_DOLLAR;
                    } else {
                        p.op(Op::EndLine)?;
                    }
                    t += 1;
                }
                b'.' => {
                    if p.flags & DOT_NL != 0 {
                        p.op(Op::AnyChar)?;
                    } else {
                        p.op(Op::AnyCharNotNL)?;
                    }
                    t += 1;
                }
                b'[' => {
                    t = p.parse_class(t)?;
                }
                b'*' | b'+' | b'?' => {
                    let before = t;
                    let op = match s[t] {
                        b'*' => Op::Star,
                        b'+' => Op::Plus,
                        _ => Op::Quest,
                    };
                    let after = p.repeat(op, 0, 0, before, t + 1, last_repeat)?;
                    repeat = Some(before);
                    t = after;
                }
                b'{' => {
                    let before = t;
                    let (min, max, after) = match p.parse_repeat(t) {
                        Some(parsed) => parsed,
                        None => {
                            p.literal('{' as Rune)?;
                            t += 1;
                            break 'big;
                        }
                    };
                    if !(0..=1000).contains(&min) || max > 1000 || max >= 0 && min > max {
                        return Err(error(ErrorCode::InvalidRepeatSize, &s[before..after]));
                    }
                    let after = p.repeat(Op::Repeat, min, max, before, after, last_repeat)?;
                    repeat = Some(before);
                    t = after;
                }
                b'\\' => {
                    if p.flags & PERL_X != 0 && s.len() - t >= 2 {
                        match s[t + 1] {
                            b'A' => {
                                p.op(Op::BeginText)?;
                                t += 2;
                                break 'big;
                            }
                            b'b' => {
                                p.op(Op::WordBoundary)?;
                                t += 2;
                                break 'big;
                            }
                            b'B' => {
                                p.op(Op::NoWordBoundary)?;
                                t += 2;
                                break 'big;
                            }
                            b'C' => {
                                return Err(error(ErrorCode::InvalidEscape, &s[t..t + 2]));
                            }
                            b'Q' => {
                                let rest = &s[t + 2..];
                                let (lit_end, next) =
                                    match rest.windows(2).position(|w| w == br"\E") {
                                        Some(i) => (t + 2 + i, t + 2 + i + 2),
                                        None => (s.len(), s.len()),
                                    };
                                let mut k = t + 2;
                                while k < lit_end {
                                    let (c, size) = next_rune(&s[k..lit_end])?;
                                    p.literal(c)?;
                                    k += size;
                                }
                                t = next;
                                break 'big;
                            }
                            b'z' => {
                                p.op(Op::EndText)?;
                                t += 2;
                                break 'big;
                            }
                            _ => {}
                        }
                    }
                    let re = p.new_regexp(Op::CharClass);
                    p.nodes[re].flags = p.flags;
                    if s.len() - t >= 2 && (s[t + 1] == b'p' || s[t + 1] == b'P') {
                        let mut r = Vec::new();
                        if let Some(rest) = p.parse_unicode_class(t, &mut r)? {
                            p.nodes[re].rune = r;
                            t = rest;
                            p.push(re)?;
                            break 'big;
                        }
                    }
                    let mut r = Vec::new();
                    if let Some(rest) = p.parse_perl_class_escape(t, &mut r) {
                        p.nodes[re].rune = r;
                        t = rest;
                        p.push(re)?;
                        break 'big;
                    }
                    p.reuse(re);
                    let (c, rest) = p.parse_escape(t)?;
                    t = rest;
                    p.literal(c)?;
                }
                _ => {
                    let (c, size) = next_rune(&s[t..])?;
                    t += size;
                    p.literal(c)?;
                }
            }
        }
        last_repeat = repeat;
    }
    p.concat()?;
    if p.swap_vertical_bar() {
        p.stack.pop();
    }
    p.alternate()?;
    if p.stack.len() != 1 {
        return Err(error(ErrorCode::MissingParen, s));
    }
    Ok(p.build(p.stack[0]))
}

fn parse_int(s: &[u8], start: usize) -> Option<(i32, usize)> {
    if start >= s.len() || !s[start].is_ascii_digit() {
        return None;
    }
    if s.len() - start >= 2 && s[start] == b'0' && s[start + 1].is_ascii_digit() {
        return None;
    }
    let mut end = start;
    while end < s.len() && s[end].is_ascii_digit() {
        end += 1;
    }
    let mut n: i32 = 0;
    for &c in &s[start..end] {
        if n >= 100_000_000 {
            n = -1;
            break;
        }
        n = n * 10 + i32::from(c - b'0');
    }
    Some((n, end))
}

fn is_valid_capture_name(name: &[u8]) -> bool {
    if name.is_empty() {
        return false;
    }
    let mut i = 0;
    while i < name.len() {
        let (c, size) = utf8::decode_rune(&name[i..]);
        if c != '_' as Rune && !isalnum(c) {
            return false;
        }
        i += size;
    }
    true
}

fn parse_escape(s: &[u8]) -> Result<(Rune, usize), Error> {
    let mut t = 1;
    if t >= s.len() {
        return Err(error(ErrorCode::TrailingBackslash, b""));
    }
    let (c, size) = next_rune(&s[t..])?;
    t += size;
    'sw: {
        match c {
            0x31..=0x37 => {
                if t >= s.len() || s[t] < b'0' || s[t] > b'7' {
                    break 'sw;
                }
                return Ok(octal(s, c, t));
            }
            0x30 => return Ok(octal(s, c, t)),
            0x78 => {
                if t >= s.len() {
                    break 'sw;
                }
                let (c, size) = next_rune(&s[t..])?;
                t += size;
                if c == '{' as Rune {
                    let mut nhex = 0;
                    let mut r: Rune = 0;
                    loop {
                        if t >= s.len() {
                            break 'sw;
                        }
                        let (c, size) = next_rune(&s[t..])?;
                        t += size;
                        if c == '}' as Rune {
                            break;
                        }
                        let v = unhex(c);
                        if v < 0 {
                            break 'sw;
                        }
                        r = r * 16 + v;
                        if r > unicode::MAX_RUNE {
                            break 'sw;
                        }
                        nhex += 1;
                    }
                    if nhex == 0 {
                        break 'sw;
                    }
                    return Ok((r, t));
                }
                let x = unhex(c);
                let (c, size) = next_rune(&s[t..])?;
                t += size;
                let y = unhex(c);
                if x < 0 || y < 0 {
                    break 'sw;
                }
                return Ok((x * 16 + y, t));
            }
            0x61 => return Ok((0x07, t)),
            0x66 => return Ok((0x0C, t)),
            0x6E => return Ok((0x0A, t)),
            0x72 => return Ok((0x0D, t)),
            0x74 => return Ok((0x09, t)),
            0x76 => return Ok((0x0B, t)),
            _ => {
                if c < 0x80 && !isalnum(c) {
                    return Ok((c, t));
                }
            }
        }
    }
    Err(error(ErrorCode::InvalidEscape, &s[..t]))
}

fn octal(s: &[u8], c: Rune, t: usize) -> (Rune, usize) {
    let mut t = t;
    let mut r = c - '0' as Rune;
    for _ in 1..3 {
        if t >= s.len() || s[t] < b'0' || s[t] > b'7' {
            break;
        }
        r = r * 8 + Rune::from(s[t]) - '0' as Rune;
        t += 1;
    }
    (r, t)
}

static ANY_TABLE: RangeTable = RangeTable {
    r16: &[Range16::new(0, 0xFFFF, 1)],
    r32: &[Range32::new(0x10000, 0x10FFFF, 1)],
    latin_offset: 0,
};

fn lookup(
    table: &'static [(&'static str, &'static RangeTable)],
    name: &[u8],
) -> Option<&'static RangeTable> {
    table
        .iter()
        .find(|(n, _)| n.as_bytes() == name)
        .map(|(_, t)| *t)
}

fn unicode_table(name: &[u8]) -> Option<(&'static RangeTable, Option<&'static RangeTable>)> {
    if name == b"Any" {
        return Some((&ANY_TABLE, Some(&ANY_TABLE)));
    }
    if let Some(t) = lookup(unicode::CATEGORIES, name) {
        return Some((t, lookup(unicode::FOLD_CATEGORY, name)));
    }
    if let Some(t) = lookup(unicode::SCRIPTS, name) {
        return Some((t, lookup(unicode::FOLD_SCRIPT, name)));
    }
    None
}

pub fn clean_class(r: &mut Vec<Rune>) {
    {
        let (pairs, _) = r.as_chunks_mut::<2>();
        pairs.sort_unstable_by(|a, b| a[0].cmp(&b[0]).then(b[1].cmp(&a[1])));
    }
    if r.len() < 2 {
        return;
    }
    let mut w = 2;
    let mut i = 2;
    while i < r.len() {
        let lo = r[i];
        let hi = r[i + 1];
        if lo <= r[w - 1] + 1 {
            if hi > r[w - 1] {
                r[w - 1] = hi;
            }
            i += 2;
            continue;
        }
        r[w] = lo;
        r[w + 1] = hi;
        w += 2;
        i += 2;
    }
    r.truncate(w);
}

pub fn in_char_class(r: Rune, class: &[Rune]) -> bool {
    let mut lo = 0;
    let mut hi = class.len() / 2;
    while lo < hi {
        let m = lo + (hi - lo) / 2;
        let (clo, chi) = (class[2 * m], class[2 * m + 1]);
        if r > chi {
            lo = m + 1;
        } else if r < clo {
            hi = m;
        } else {
            return true;
        }
    }
    false
}

pub fn append_literal(r: &mut Vec<Rune>, x: Rune, flags: Flags) {
    if flags & FOLD_CASE != 0 {
        append_folded_range(r, x, x);
    } else {
        append_range(r, x, x);
    }
}

pub fn append_range(r: &mut Vec<Rune>, lo: Rune, hi: Rune) {
    let n = r.len();
    let mut i = 2;
    while i <= 4 {
        if n >= i {
            let rlo = r[n - i];
            let rhi = r[n - i + 1];
            if lo <= rhi + 1 && rlo <= hi + 1 {
                if lo < rlo {
                    r[n - i] = lo;
                }
                if hi > rhi {
                    r[n - i + 1] = hi;
                }
                return;
            }
        }
        i += 2;
    }
    r.push(lo);
    r.push(hi);
}

pub fn append_folded_range(r: &mut Vec<Rune>, lo: Rune, hi: Rune) {
    let (mut lo, mut hi) = (lo, hi);
    if lo <= MIN_FOLD && hi >= MAX_FOLD {
        append_range(r, lo, hi);
        return;
    }
    if hi < MIN_FOLD || lo > MAX_FOLD {
        append_range(r, lo, hi);
        return;
    }
    if lo < MIN_FOLD {
        append_range(r, lo, MIN_FOLD - 1);
        lo = MIN_FOLD;
    }
    if hi > MAX_FOLD {
        append_range(r, MAX_FOLD + 1, hi);
        hi = MAX_FOLD;
    }
    let mut c = lo;
    while c <= hi {
        append_range(r, c, c);
        let mut f = unicode::simple_fold(c);
        while f != c {
            append_range(r, f, f);
            f = unicode::simple_fold(f);
        }
        c += 1;
    }
}

pub fn append_class(r: &mut Vec<Rune>, x: &[Rune]) {
    for p in x.chunks_exact(2) {
        append_range(r, p[0], p[1]);
    }
}

pub fn append_folded_class(r: &mut Vec<Rune>, x: &[Rune]) {
    for p in x.chunks_exact(2) {
        append_folded_range(r, p[0], p[1]);
    }
}

pub fn append_negated_class(r: &mut Vec<Rune>, x: &[Rune]) {
    let mut next_lo: Rune = 0;
    for p in x.chunks_exact(2) {
        let (lo, hi) = (p[0], p[1]);
        if next_lo < lo {
            append_range(r, next_lo, lo - 1);
        }
        next_lo = hi + 1;
    }
    if next_lo <= unicode::MAX_RUNE {
        append_range(r, next_lo, unicode::MAX_RUNE);
    }
}

pub fn append_table(r: &mut Vec<Rune>, x: &RangeTable) {
    for xr in x.r16 {
        let (lo, hi, stride) = (Rune::from(xr.lo), Rune::from(xr.hi), Rune::from(xr.stride));
        if stride == 1 {
            append_range(r, lo, hi);
            continue;
        }
        let mut c = lo;
        while c <= hi {
            append_range(r, c, c);
            c += stride;
        }
    }
    for xr in x.r32 {
        let (lo, hi, stride) = (xr.lo as Rune, xr.hi as Rune, xr.stride as Rune);
        if stride == 1 {
            append_range(r, lo, hi);
            continue;
        }
        let mut c = lo;
        while c <= hi {
            append_range(r, c, c);
            c += stride;
        }
    }
}

pub fn append_negated_table(r: &mut Vec<Rune>, x: &RangeTable) {
    let mut next_lo: Rune = 0;
    let ranges = x
        .r16
        .iter()
        .map(|xr| (Rune::from(xr.lo), Rune::from(xr.hi), Rune::from(xr.stride)))
        .chain(
            x.r32
                .iter()
                .map(|xr| (xr.lo as Rune, xr.hi as Rune, xr.stride as Rune)),
        );
    for (lo, hi, stride) in ranges {
        if stride == 1 {
            if next_lo < lo {
                append_range(r, next_lo, lo - 1);
            }
            next_lo = hi + 1;
            continue;
        }
        let mut c = lo;
        while c <= hi {
            if next_lo < c {
                append_range(r, next_lo, c - 1);
            }
            next_lo = c + 1;
            c += stride;
        }
    }
    if next_lo <= unicode::MAX_RUNE {
        append_range(r, next_lo, unicode::MAX_RUNE);
    }
}

pub fn negate_class(r: &mut Vec<Rune>) {
    let mut next_lo: Rune = 0;
    let mut w = 0;
    let mut i = 0;
    while i < r.len() {
        let (lo, hi) = (r[i], r[i + 1]);
        if next_lo < lo {
            r[w] = next_lo;
            r[w + 1] = lo - 1;
            w += 2;
        }
        next_lo = hi + 1;
        i += 2;
    }
    r.truncate(w);
    if next_lo <= unicode::MAX_RUNE {
        r.push(next_lo);
        r.push(unicode::MAX_RUNE);
    }
}

pub fn check_utf8(s: &[u8]) -> Result<(), Error> {
    let mut i = 0;
    while i < s.len() {
        let (r, size) = utf8::decode_rune(&s[i..]);
        if r == utf8::RUNE_ERROR && size == 1 {
            return Err(error(ErrorCode::InvalidUtf8, &s[i..]));
        }
        i += size;
    }
    Ok(())
}

pub fn next_rune(s: &[u8]) -> Result<(Rune, usize), Error> {
    let (c, size) = utf8::decode_rune(s);
    if c == utf8::RUNE_ERROR && size == 1 {
        return Err(error(ErrorCode::InvalidUtf8, s));
    }
    Ok((c, size))
}

fn isalnum(c: Rune) -> bool {
    (0x30..=0x39).contains(&c) || (0x41..=0x5A).contains(&c) || (0x61..=0x7A).contains(&c)
}

fn unhex(c: Rune) -> Rune {
    if (0x30..=0x39).contains(&c) {
        return c - 0x30;
    }
    if (0x61..=0x66).contains(&c) {
        return c - 0x61 + 10;
    }
    if (0x41..=0x46).contains(&c) {
        return c - 0x41 + 10;
    }
    -1
}
