mod charclass;
mod code;
mod escape;
mod parser;
mod prefix;
mod replacerdata;
mod tree;
mod writer;

pub use self::charclass::{
    CharSet, LOWERCASE_ADD, LOWERCASE_BAD, LOWERCASE_BOR, LOWERCASE_SET, any_class,
    char_description, digit_class, ecma_any_class, ecma_digit_class, ecma_space_class,
    ecma_word_class, is_ecma_word_char, is_word_char, none_class, not_digit_class,
    not_ecma_digit_class, not_ecma_space_class, not_ecma_word_class, not_re2_space_class,
    not_space_class, not_word_class, re2_space_class, space_class, word_class,
};
pub use self::code::{
    BACK, BACK2, BACKJUMP, BEGINNING, BOL, BOUNDARY, BRANCHCOUNT, BRANCHMARK, CAPTUREMARK, CI,
    Code, ECMA_BOUNDARY, END, END_Z, EOL, FOREJUMP, GETMARK, GOTO, InstOp, LAZYBRANCH,
    LAZYBRANCHCOUNT, LAZYBRANCHMARK, MASK, MULTI, NON_ECMA_BOUNDARY, NONBOUNDARY, NOTHING, NOTONE,
    NOTONELAZY, NOTONELOOP, NOTONEREP, NULLCOUNT, NULLMARK, ONE, ONELAZY, ONELOOP, ONEREP, PRUNE,
    REF, RTL, SET, SETCOUNT, SETJUMP, SETLAZY, SETLOOP, SETMARK, SETREP, START, STOP, TESTREF,
};
pub use self::escape::{escape, unescape};
pub use self::parser::{
    COMPILED, DEBUG, ECMASCRIPT, EXPLICIT_CAPTURE, Error, ErrorArg, ErrorCode, IGNORE_CASE,
    IGNORE_PATTERN_WHITESPACE, MULTILINE, RE2, RIGHT_TO_LEFT, RegexOptions, SINGLELINE, UNICODE,
    parse,
};
pub use self::prefix::{
    ANCHOR_BEGINNING, ANCHOR_BOL, ANCHOR_BOUNDARY, ANCHOR_ECMA_BOUNDARY, ANCHOR_END, ANCHOR_END_Z,
    ANCHOR_EOL, ANCHOR_START, AnchorLoc, BmPrefix, Prefix, anchor_description,
};
pub use self::replacerdata::{ReplacerData, new_replacer_data};
pub use self::tree::RegexTree;
pub use self::writer::{MAX_PREFIX_SIZE, write};

use gocompat::utf8::{self, Rune};

pub(crate) const MAX_INT32: isize = i32::MAX as isize;

pub(crate) fn decode_runes(s: &[u8]) -> Vec<Rune> {
    let mut out = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let (r, n) = utf8::decode_rune(&s[i..]);
        out.push(r);
        i += n;
    }
    out
}

pub(crate) fn push_rune(s: &mut String, r: Rune) {
    s.push(char::from_u32(r as u32).unwrap_or('\u{FFFD}'));
}

pub(crate) fn runes_to_string(runes: &[Rune]) -> String {
    let mut s = String::with_capacity(runes.len());
    for &r in runes {
        push_rune(&mut s, r);
    }
    s
}

pub(crate) fn rune_to_char(r: Rune) -> Option<char> {
    if r < 0 {
        return None;
    }
    char::from_u32(r as u32)
}

pub(crate) fn write_stdout(s: &str) {
    use std::io::Write;
    let _ = std::io::stdout().write_all(s.as_bytes());
}
