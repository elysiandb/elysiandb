use super::parse::{FOLD_CASE, Flags};
use gocompat::strconv;
use gocompat::unicode;
use gocompat::utf8::{self, Rune};
use std::fmt;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prog {
    pub inst: Vec<Inst>,
    pub start: usize,
    pub num_cap: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum InstOp {
    Alt = 0,
    AltMatch = 1,
    Capture = 2,
    EmptyWidth = 3,
    Match = 4,
    Fail = 5,
    Nop = 6,
    Rune = 7,
    Rune1 = 8,
    RuneAny = 9,
    RuneAnyNotNL = 10,
}

impl fmt::Display for InstOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            InstOp::Alt => "InstAlt",
            InstOp::AltMatch => "InstAltMatch",
            InstOp::Capture => "InstCapture",
            InstOp::EmptyWidth => "InstEmptyWidth",
            InstOp::Match => "InstMatch",
            InstOp::Fail => "InstFail",
            InstOp::Nop => "InstNop",
            InstOp::Rune => "InstRune",
            InstOp::Rune1 => "InstRune1",
            InstOp::RuneAny => "InstRuneAny",
            InstOp::RuneAnyNotNL => "InstRuneAnyNotNL",
        })
    }
}

pub type EmptyOp = u8;

pub const EMPTY_BEGIN_LINE: EmptyOp = 1;
pub const EMPTY_END_LINE: EmptyOp = 1 << 1;
pub const EMPTY_BEGIN_TEXT: EmptyOp = 1 << 2;
pub const EMPTY_END_TEXT: EmptyOp = 1 << 3;
pub const EMPTY_WORD_BOUNDARY: EmptyOp = 1 << 4;
pub const EMPTY_NO_WORD_BOUNDARY: EmptyOp = 1 << 5;

pub fn empty_op_context(r1: Rune, r2: Rune) -> EmptyOp {
    let mut op: EmptyOp = EMPTY_NO_WORD_BOUNDARY;
    let mut boundary: u8 = 0;
    if is_word_char(r1) {
        boundary = 1;
    } else if r1 == '\n' as Rune {
        op |= EMPTY_BEGIN_LINE;
    } else if r1 < 0 {
        op |= EMPTY_BEGIN_TEXT | EMPTY_BEGIN_LINE;
    }
    if is_word_char(r2) {
        boundary ^= 1;
    } else if r2 == '\n' as Rune {
        op |= EMPTY_END_LINE;
    } else if r2 < 0 {
        op |= EMPTY_END_TEXT | EMPTY_END_LINE;
    }
    if boundary != 0 {
        op ^= EMPTY_WORD_BOUNDARY | EMPTY_NO_WORD_BOUNDARY;
    }
    op
}

pub fn is_word_char(r: Rune) -> bool {
    (0x61..=0x7A).contains(&r)
        || (0x41..=0x5A).contains(&r)
        || (0x30..=0x39).contains(&r)
        || r == '_' as Rune
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Inst {
    pub op: InstOp,
    pub out: u32,
    pub arg: u32,
    pub rune: Arc<[Rune]>,
}

impl Inst {
    pub fn new(op: InstOp) -> Inst {
        Inst {
            op,
            out: 0,
            arg: 0,
            rune: Arc::from(Vec::new()),
        }
    }

    pub fn op(&self) -> InstOp {
        match self.op {
            InstOp::Rune1 | InstOp::RuneAny | InstOp::RuneAnyNotNL => InstOp::Rune,
            op => op,
        }
    }

    pub fn match_rune(&self, r: Rune) -> bool {
        self.match_rune_pos(r).is_some()
    }

    pub fn match_rune_pos(&self, r: Rune) -> Option<usize> {
        let rune = &self.rune;
        match rune.len() {
            0 => None,
            1 => {
                let r0 = rune[0];
                if r == r0 {
                    return Some(0);
                }
                if (self.arg as Flags) & FOLD_CASE != 0 {
                    let mut r1 = unicode::simple_fold(r0);
                    while r1 != r0 {
                        if r == r1 {
                            return Some(0);
                        }
                        r1 = unicode::simple_fold(r1);
                    }
                }
                None
            }
            2 => {
                if r >= rune[0] && r <= rune[1] {
                    return Some(0);
                }
                None
            }
            4 | 6 | 8 => {
                let mut j = 0;
                while j < rune.len() {
                    if r < rune[j] {
                        return None;
                    }
                    if r <= rune[j + 1] {
                        return Some(j / 2);
                    }
                    j += 2;
                }
                None
            }
            _ => {
                let mut lo = 0;
                let mut hi = rune.len() / 2;
                while lo < hi {
                    let m = lo + (hi - lo) / 2;
                    let c = rune[2 * m];
                    if c <= r {
                        if r <= rune[2 * m + 1] {
                            return Some(m);
                        }
                        lo = m + 1;
                    } else {
                        hi = m;
                    }
                }
                None
            }
        }
    }

    pub fn match_empty_width(&self, before: Rune, after: Rune) -> bool {
        match self.arg as EmptyOp {
            EMPTY_BEGIN_LINE => before == '\n' as Rune || before == -1,
            EMPTY_END_LINE => after == '\n' as Rune || after == -1,
            EMPTY_BEGIN_TEXT => before == -1,
            EMPTY_END_TEXT => after == -1,
            EMPTY_WORD_BOUNDARY => is_word_char(before) != is_word_char(after),
            EMPTY_NO_WORD_BOUNDARY => is_word_char(before) == is_word_char(after),
            _ => panic!("unknown empty-width argument"),
        }
    }
}

impl fmt::Display for Inst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = String::new();
        dump_inst(&mut b, self);
        f.write_str(&b)
    }
}

impl Prog {
    pub fn skip_nop(&self, pc: u32) -> &Inst {
        let mut i = &self.inst[pc as usize];
        while i.op == InstOp::Nop || i.op == InstOp::Capture {
            i = &self.inst[i.out as usize];
        }
        i
    }

    pub fn prefix(&self) -> (Vec<u8>, bool) {
        let mut i = self.skip_nop(self.start as u32);
        if i.op() != InstOp::Rune || i.rune.len() != 1 {
            return (Vec::new(), i.op == InstOp::Match);
        }
        let mut buf = Vec::new();
        while i.op() == InstOp::Rune
            && i.rune.len() == 1
            && (i.arg as Flags) & FOLD_CASE == 0
            && i.rune[0] != utf8::RUNE_ERROR
        {
            utf8::append_rune(&mut buf, i.rune[0]);
            i = self.skip_nop(i.out);
        }
        (buf, i.op == InstOp::Match)
    }

    pub fn start_cond(&self) -> EmptyOp {
        let mut flag: EmptyOp = 0;
        let mut pc = self.start;
        loop {
            let i = &self.inst[pc];
            match i.op {
                InstOp::EmptyWidth => flag |= i.arg as EmptyOp,
                InstOp::Fail => return !0,
                InstOp::Capture | InstOp::Nop => {}
                _ => break,
            }
            pc = i.out as usize;
        }
        flag
    }
}

impl fmt::Display for Prog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut b = String::new();
        for (j, i) in self.inst.iter().enumerate() {
            let mut pc = j.to_string();
            if pc.len() < 3 {
                b.push_str(&"   "[pc.len()..]);
            }
            if j == self.start {
                pc.push('*');
            }
            b.push_str(&pc);
            b.push('\t');
            dump_inst(&mut b, i);
            b.push('\n');
        }
        f.write_str(&b)
    }
}

fn runes_string(runes: &[Rune]) -> Vec<u8> {
    let mut s = Vec::new();
    for &r in runes {
        utf8::append_rune(&mut s, r);
    }
    s
}

fn dump_inst(b: &mut String, i: &Inst) {
    match i.op {
        InstOp::Alt => b.push_str(&format!("alt -> {}, {}", i.out, i.arg)),
        InstOp::AltMatch => b.push_str(&format!("altmatch -> {}, {}", i.out, i.arg)),
        InstOp::Capture => b.push_str(&format!("cap {} -> {}", i.arg, i.out)),
        InstOp::EmptyWidth => b.push_str(&format!("empty {} -> {}", i.arg, i.out)),
        InstOp::Match => b.push_str("match"),
        InstOp::Fail => b.push_str("fail"),
        InstOp::Nop => b.push_str(&format!("nop -> {}", i.out)),
        InstOp::Rune => {
            b.push_str("rune ");
            b.push_str(&strconv::quote_to_ascii(&runes_string(&i.rune)));
            if (i.arg as Flags) & FOLD_CASE != 0 {
                b.push_str("/i");
            }
            b.push_str(&format!(" -> {}", i.out));
        }
        InstOp::Rune1 => {
            b.push_str("rune1 ");
            b.push_str(&strconv::quote_to_ascii(&runes_string(&i.rune)));
            b.push_str(&format!(" -> {}", i.out));
        }
        InstOp::RuneAny => b.push_str(&format!("any -> {}", i.out)),
        InstOp::RuneAnyNotNL => b.push_str(&format!("anynotnl -> {}", i.out)),
    }
}
