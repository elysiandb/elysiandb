use std::collections::HashMap;

use gocompat::utf8::Rune;

use super::charclass::{CharSet, char_description};
use super::escape::escape;
use super::prefix::{AnchorLoc, BmPrefix, Prefix, anchor_description};
use super::{MAX_INT32, runes_to_string};

pub type InstOp = isize;

pub const ONEREP: InstOp = 0;
pub const NOTONEREP: InstOp = 1;
pub const SETREP: InstOp = 2;
pub const ONELOOP: InstOp = 3;
pub const NOTONELOOP: InstOp = 4;
pub const SETLOOP: InstOp = 5;
pub const ONELAZY: InstOp = 6;
pub const NOTONELAZY: InstOp = 7;
pub const SETLAZY: InstOp = 8;
pub const ONE: InstOp = 9;
pub const NOTONE: InstOp = 10;
pub const SET: InstOp = 11;
pub const MULTI: InstOp = 12;
pub const REF: InstOp = 13;
pub const BOL: InstOp = 14;
pub const EOL: InstOp = 15;
pub const BOUNDARY: InstOp = 16;
pub const NONBOUNDARY: InstOp = 17;
pub const BEGINNING: InstOp = 18;
pub const START: InstOp = 19;
pub const END_Z: InstOp = 20;
pub const END: InstOp = 21;
pub const NOTHING: InstOp = 22;
pub const LAZYBRANCH: InstOp = 23;
pub const BRANCHMARK: InstOp = 24;
pub const LAZYBRANCHMARK: InstOp = 25;
pub const NULLCOUNT: InstOp = 26;
pub const SETCOUNT: InstOp = 27;
pub const BRANCHCOUNT: InstOp = 28;
pub const LAZYBRANCHCOUNT: InstOp = 29;
pub const NULLMARK: InstOp = 30;
pub const SETMARK: InstOp = 31;
pub const CAPTUREMARK: InstOp = 32;
pub const GETMARK: InstOp = 33;
pub const SETJUMP: InstOp = 34;
pub const BACKJUMP: InstOp = 35;
pub const FOREJUMP: InstOp = 36;
pub const TESTREF: InstOp = 37;
pub const GOTO: InstOp = 38;
pub const PRUNE: InstOp = 39;
pub const STOP: InstOp = 40;
pub const ECMA_BOUNDARY: InstOp = 41;
pub const NON_ECMA_BOUNDARY: InstOp = 42;
pub const MASK: InstOp = 63;
pub const RTL: InstOp = 64;
pub const BACK: InstOp = 128;
pub const BACK2: InstOp = 256;
pub const CI: InstOp = 512;

#[derive(Clone, Debug)]
pub struct Code {
    pub codes: Vec<isize>,
    pub strings: Vec<Vec<Rune>>,
    pub sets: Vec<CharSet>,
    pub track_count: isize,
    pub caps: Option<HashMap<isize, isize>>,
    pub capsize: isize,
    pub fc_prefix: Option<Prefix>,
    pub bm_prefix: Option<BmPrefix>,
    pub anchors: AnchorLoc,
    pub right_to_left: bool,
}

pub(crate) fn opcode_backtracks(op: InstOp) -> bool {
    matches!(
        op & MASK,
        ONELOOP
            | NOTONELOOP
            | SETLOOP
            | ONELAZY
            | NOTONELAZY
            | SETLAZY
            | LAZYBRANCH
            | BRANCHMARK
            | LAZYBRANCHMARK
            | NULLCOUNT
            | SETCOUNT
            | BRANCHCOUNT
            | LAZYBRANCHCOUNT
            | SETMARK
            | CAPTUREMARK
            | GETMARK
            | SETJUMP
            | BACKJUMP
            | FOREJUMP
            | GOTO
    )
}

pub(crate) fn opcode_size(op: InstOp) -> isize {
    match op & MASK {
        NOTHING | BOL | EOL | BOUNDARY | NONBOUNDARY | ECMA_BOUNDARY | NON_ECMA_BOUNDARY
        | BEGINNING | START | END_Z | END | NULLMARK | SETMARK | GETMARK | SETJUMP | BACKJUMP
        | FOREJUMP | STOP => 1,
        ONE | NOTONE | MULTI | REF | TESTREF | GOTO | NULLCOUNT | SETCOUNT | LAZYBRANCH
        | BRANCHMARK | LAZYBRANCHMARK | PRUNE | SET => 2,
        CAPTUREMARK | BRANCHCOUNT | LAZYBRANCHCOUNT | ONEREP | NOTONEREP | ONELOOP | NOTONELOOP
        | ONELAZY | NOTONELAZY | SETLAZY | SETREP | SETLOOP => 3,
        other => panic!("unexpected op code: {other}"),
    }
}

const CODE_STR: [&str; 43] = [
    "Onerep",
    "Notonerep",
    "Setrep",
    "Oneloop",
    "Notoneloop",
    "Setloop",
    "Onelazy",
    "Notonelazy",
    "Setlazy",
    "One",
    "Notone",
    "Set",
    "Multi",
    "Ref",
    "Bol",
    "Eol",
    "Boundary",
    "Nonboundary",
    "Beginning",
    "Start",
    "EndZ",
    "End",
    "Nothing",
    "Lazybranch",
    "Branchmark",
    "Lazybranchmark",
    "Nullcount",
    "Setcount",
    "Branchcount",
    "Lazybranchcount",
    "Nullmark",
    "Setmark",
    "Capturemark",
    "Getmark",
    "Setjump",
    "Backjump",
    "Forejump",
    "Testref",
    "Goto",
    "Prune",
    "Stop",
    "ECMABoundary",
    "NonECMABoundary",
];

pub(crate) fn operator_description(op: InstOp) -> String {
    let mut desc = CODE_STR[(op & MASK) as usize].to_string();
    if op & CI != 0 {
        desc.push_str("-Ci");
    }
    if op & RTL != 0 {
        desc.push_str("-Rtl");
    }
    if op & BACK != 0 {
        desc.push_str("-Back");
    }
    if op & BACK2 != 0 {
        desc.push_str("-Back2");
    }
    desc
}

impl Code {
    pub fn opcode_description(&self, offset: usize) -> String {
        let mut buf = String::new();
        let mut op = self.codes[offset];
        buf.push_str(&format!("{offset:06} "));
        if opcode_backtracks(op & MASK) {
            buf.push('*');
        } else {
            buf.push(' ');
        }
        buf.push_str(&operator_description(op));
        buf.push('(');
        op &= MASK;
        match op {
            ONE | NOTONE | ONEREP | NOTONEREP | ONELOOP | NOTONELOOP | ONELAZY | NOTONELAZY => {
                buf.push_str("Ch = ");
                buf.push_str(&char_description(self.codes[offset + 1] as Rune));
            }
            SET | SETREP | SETLOOP | SETLAZY => {
                buf.push_str("Set = ");
                buf.push_str(&self.sets[self.codes[offset + 1] as usize].string());
            }
            MULTI => {
                buf.push_str("String = ");
                buf.push_str(&runes_to_string(
                    &self.strings[self.codes[offset + 1] as usize],
                ));
            }
            REF | TESTREF => {
                buf.push_str(&format!("Index = {}", self.codes[offset + 1]));
            }
            CAPTUREMARK => {
                buf.push_str(&format!("Index = {}", self.codes[offset + 1]));
                if self.codes[offset + 2] != -1 {
                    buf.push_str(&format!(", Unindex = {}", self.codes[offset + 2]));
                }
            }
            NULLCOUNT | SETCOUNT => {
                buf.push_str(&format!("Value = {}", self.codes[offset + 1]));
            }
            GOTO | LAZYBRANCH | BRANCHMARK | LAZYBRANCHMARK | BRANCHCOUNT | LAZYBRANCHCOUNT => {
                buf.push_str(&format!("Addr = {}", self.codes[offset + 1]));
            }
            _ => {}
        }
        match op {
            ONEREP | NOTONEREP | ONELOOP | NOTONELOOP | ONELAZY | NOTONELAZY | SETREP | SETLOOP
            | SETLAZY => {
                buf.push_str(", Rep = ");
                if self.codes[offset + 2] == MAX_INT32 {
                    buf.push_str("inf");
                } else {
                    buf.push_str(&format!("{}", self.codes[offset + 2]));
                }
            }
            BRANCHCOUNT | LAZYBRANCHCOUNT => {
                buf.push_str(", Limit = ");
                if self.codes[offset + 2] == MAX_INT32 {
                    buf.push_str("inf");
                } else {
                    buf.push_str(&format!("{}", self.codes[offset + 2]));
                }
            }
            _ => {}
        }
        buf.push(')');
        buf
    }

    pub fn dump(&self) -> String {
        let mut buf = String::new();
        if self.right_to_left {
            buf.push_str("Direction:  right-to-left\n");
        } else {
            buf.push_str("Direction:  left-to-right\n");
        }
        match &self.fc_prefix {
            None => buf.push_str("Firstchars: n/a\n"),
            Some(fc) => buf.push_str(&format!("Firstchars: {}\n", fc.prefix_set.string())),
        }
        match &self.bm_prefix {
            None => buf.push_str("Prefix:     n/a\n"),
            Some(bm) => buf.push_str(&format!("Prefix:     {}\n", escape(bm.string().as_bytes()))),
        }
        buf.push_str(&format!(
            "Anchors:    {}\n",
            anchor_description(self.anchors)
        ));
        buf.push('\n');
        if let Some(bm) = &self.bm_prefix {
            buf.push_str("BoyerMoore:\n");
            buf.push_str(&bm.dump("    "));
            buf.push('\n');
        }
        let mut i = 0usize;
        while i < self.codes.len() {
            buf.push_str(&self.opcode_description(i));
            buf.push('\n');
            i += opcode_size(self.codes[i]) as usize;
        }
        buf
    }
}
