mod tables;

pub use self::tables::*;
pub use self::tables::{
    C as OTHER, L as LETTER, LL as LOWER, LT as TITLE, LU as UPPER, M as MARK, N as NUMBER,
    ND as DIGIT, P as PUNCT, S as SYMBOL, SENTENCE_TERMINAL as STERM, Z as SPACE,
};

use crate::utf8::Rune;

pub const MAX_RUNE: Rune = 0x10FFFF;
pub const REPLACEMENT_CHAR: Rune = 0xFFFD;
pub const MAX_ASCII: Rune = 0x7F;
pub const MAX_LATIN1: Rune = 0xFF;

pub const UPPER_CASE: usize = 0;
pub const LOWER_CASE: usize = 1;
pub const TITLE_CASE: usize = 2;
pub const MAX_CASE: usize = 3;

pub const UPPER_LOWER: Rune = MAX_RUNE + 1;

const LINEAR_MAX: usize = 18;

const P_C: u8 = 1;
const P_P: u8 = 1 << 1;
const P_N: u8 = 1 << 2;
const P_S: u8 = 1 << 3;
const P_Z: u8 = 1 << 4;
const P_LU: u8 = 1 << 5;
const P_LL: u8 = 1 << 6;
const P_PRINT: u8 = 1 << 7;
const P_GRAPHIC: u8 = P_PRINT | P_Z;
const P_LO: u8 = P_LL | P_LU;
const P_LMASK: u8 = P_LO;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range16 {
    pub lo: u16,
    pub hi: u16,
    pub stride: u16,
}

impl Range16 {
    pub const fn new(lo: u16, hi: u16, stride: u16) -> Range16 {
        Range16 { lo, hi, stride }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Range32 {
    pub lo: u32,
    pub hi: u32,
    pub stride: u32,
}

impl Range32 {
    pub const fn new(lo: u32, hi: u32, stride: u32) -> Range32 {
        Range32 { lo, hi, stride }
    }
}

#[derive(Debug)]
pub struct RangeTable {
    pub r16: &'static [Range16],
    pub r32: &'static [Range32],
    pub latin_offset: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CaseRange {
    pub lo: u32,
    pub hi: u32,
    pub delta: [Rune; MAX_CASE],
}

impl CaseRange {
    pub const fn new(lo: u32, hi: u32, delta: [Rune; MAX_CASE]) -> CaseRange {
        CaseRange { lo, hi, delta }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FoldPair {
    pub from: u16,
    pub to: u16,
}

impl FoldPair {
    pub const fn new(from: u16, to: u16) -> FoldPair {
        FoldPair { from, to }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SpecialCase(pub &'static [CaseRange]);

const TURKISH_CASE_RANGES: &[CaseRange] = &[
    CaseRange::new(0x0049, 0x0049, [0, 0x131 - 0x49, 0]),
    CaseRange::new(0x0069, 0x0069, [0x130 - 0x69, 0, 0x130 - 0x69]),
    CaseRange::new(0x0130, 0x0130, [0, 0x69 - 0x130, 0]),
    CaseRange::new(0x0131, 0x0131, [0x49 - 0x131, 0, 0x49 - 0x131]),
];

pub static TURKISH_CASE: SpecialCase = SpecialCase(TURKISH_CASE_RANGES);

pub static AZERI_CASE: SpecialCase = SpecialCase(TURKISH_CASE_RANGES);

pub static GRAPHIC_RANGES: &[&RangeTable] = &[&L, &M, &N, &P, &S, &ZS];

pub static PRINT_RANGES: &[&RangeTable] = &[&L, &M, &N, &P, &S];

fn in_range16(range: &Range16, r: u16) -> bool {
    range.stride == 1 || (r - range.lo).is_multiple_of(range.stride)
}

fn in_range32(range: &Range32, r: u32) -> bool {
    range.stride == 1 || (r - range.lo).is_multiple_of(range.stride)
}

fn is16(ranges: &[Range16], r: u16) -> bool {
    if ranges.len() <= LINEAR_MAX || r <= MAX_LATIN1 as u16 {
        for range in ranges {
            if r < range.lo {
                return false;
            }
            if r <= range.hi {
                return in_range16(range, r);
            }
        }
        return false;
    }
    let mut lo = 0;
    let mut hi = ranges.len();
    while lo < hi {
        let m = (lo + hi) / 2;
        let range = &ranges[m];
        if range.lo <= r && r <= range.hi {
            return in_range16(range, r);
        }
        if r < range.lo {
            hi = m;
        } else {
            lo = m + 1;
        }
    }
    false
}

fn is32(ranges: &[Range32], r: u32) -> bool {
    if ranges.len() <= LINEAR_MAX {
        for range in ranges {
            if r < range.lo {
                return false;
            }
            if r <= range.hi {
                return in_range32(range, r);
            }
        }
        return false;
    }
    let mut lo = 0;
    let mut hi = ranges.len();
    while lo < hi {
        let m = (lo + hi) / 2;
        let range = &ranges[m];
        if range.lo <= r && r <= range.hi {
            return in_range32(range, r);
        }
        if r < range.lo {
            hi = m;
        } else {
            lo = m + 1;
        }
    }
    false
}

pub fn is(range_tab: &RangeTable, r: Rune) -> bool {
    let r16 = range_tab.r16;
    if let Some(last) = r16.last()
        && (r as u32) <= u32::from(last.hi)
    {
        return is16(r16, r as u16);
    }
    let r32 = range_tab.r32;
    if let Some(first) = r32.first()
        && r >= first.lo as Rune
    {
        return is32(r32, r as u32);
    }
    false
}

fn is_excluding_latin(range_tab: &RangeTable, r: Rune) -> bool {
    let r16 = range_tab.r16;
    let off = range_tab.latin_offset;
    if r16.len() > off && (r as u32) <= u32::from(r16[r16.len() - 1].hi) {
        return is16(&r16[off..], r as u16);
    }
    let r32 = range_tab.r32;
    if let Some(first) = r32.first()
        && r >= first.lo as Rune
    {
        return is32(r32, r as u32);
    }
    false
}

fn latin1(r: Rune) -> Option<u8> {
    if (r as u32) <= MAX_LATIN1 as u32 {
        Some(LATIN1_PROPERTIES[r as usize])
    } else {
        None
    }
}

pub fn is_upper(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_LMASK == P_LU,
        None => is_excluding_latin(&UPPER, r),
    }
}

pub fn is_lower(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_LMASK == P_LL,
        None => is_excluding_latin(&LOWER, r),
    }
}

pub fn is_title(r: Rune) -> bool {
    if r <= MAX_LATIN1 {
        return false;
    }
    is_excluding_latin(&TITLE, r)
}

pub fn is_graphic(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_GRAPHIC != 0,
        None => in_(r, GRAPHIC_RANGES),
    }
}

pub fn is_print(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_PRINT != 0,
        None => in_(r, PRINT_RANGES),
    }
}

pub fn is_one_of(ranges: &[&RangeTable], r: Rune) -> bool {
    ranges.iter().any(|inside| is(inside, r))
}

pub fn in_(r: Rune, ranges: &[&RangeTable]) -> bool {
    ranges.iter().any(|inside| is(inside, r))
}

pub fn is_control(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_C != 0,
        None => false,
    }
}

pub fn is_letter(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_LMASK != 0,
        None => is_excluding_latin(&LETTER, r),
    }
}

pub fn is_mark(r: Rune) -> bool {
    is_excluding_latin(&MARK, r)
}

pub fn is_number(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_N != 0,
        None => is_excluding_latin(&NUMBER, r),
    }
}

pub fn is_punct(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_P != 0,
        None => is(&PUNCT, r),
    }
}

pub fn is_space(r: Rune) -> bool {
    if (r as u32) <= MAX_LATIN1 as u32 {
        return matches!(r, 0x09 | 0x0A | 0x0B | 0x0C | 0x0D | 0x20 | 0x85 | 0xA0);
    }
    is_excluding_latin(&WHITE_SPACE, r)
}

pub fn is_symbol(r: Rune) -> bool {
    match latin1(r) {
        Some(p) => p & P_S != 0,
        None => is_excluding_latin(&SYMBOL, r),
    }
}

pub fn is_digit(r: Rune) -> bool {
    if r <= MAX_LATIN1 {
        return (0x30..=0x39).contains(&r);
    }
    is_excluding_latin(&DIGIT, r)
}

fn lookup_case_range(r: Rune, case_range: &[CaseRange]) -> Option<&CaseRange> {
    let mut lo = 0;
    let mut hi = case_range.len();
    while lo < hi {
        let m = (lo + hi) / 2;
        let cr = &case_range[m];
        if cr.lo as Rune <= r && r <= cr.hi as Rune {
            return Some(cr);
        }
        if r < cr.lo as Rune {
            hi = m;
        } else {
            lo = m + 1;
        }
    }
    None
}

fn convert_case(case: usize, r: Rune, cr: &CaseRange) -> Rune {
    let delta = cr.delta[case];
    if delta > MAX_RUNE {
        let lo = cr.lo as Rune;
        return lo + (((r - lo) & !1) | (case as Rune & 1));
    }
    r + delta
}

fn to_case(case: usize, r: Rune, case_range: &[CaseRange]) -> (Rune, bool) {
    if case >= MAX_CASE {
        return (REPLACEMENT_CHAR, false);
    }
    match lookup_case_range(r, case_range) {
        Some(cr) => (convert_case(case, r, cr), true),
        None => (r, false),
    }
}

pub fn to(case: usize, r: Rune) -> Rune {
    to_case(case, r, CASE_RANGES).0
}

pub fn to_upper(r: Rune) -> Rune {
    if r <= MAX_ASCII {
        if (0x61..=0x7A).contains(&r) {
            return r - 0x20;
        }
        return r;
    }
    to(UPPER_CASE, r)
}

pub fn to_lower(r: Rune) -> Rune {
    if r <= MAX_ASCII {
        if (0x41..=0x5A).contains(&r) {
            return r + 0x20;
        }
        return r;
    }
    to(LOWER_CASE, r)
}

pub fn to_title(r: Rune) -> Rune {
    if r <= MAX_ASCII {
        if (0x61..=0x7A).contains(&r) {
            return r - 0x20;
        }
        return r;
    }
    to(TITLE_CASE, r)
}

impl SpecialCase {
    pub fn to_upper(&self, r: Rune) -> Rune {
        let (r1, had_mapping) = to_case(UPPER_CASE, r, self.0);
        if r1 == r && !had_mapping {
            return to_upper(r);
        }
        r1
    }

    pub fn to_title(&self, r: Rune) -> Rune {
        let (r1, had_mapping) = to_case(TITLE_CASE, r, self.0);
        if r1 == r && !had_mapping {
            return to_title(r);
        }
        r1
    }

    pub fn to_lower(&self, r: Rune) -> Rune {
        let (r1, had_mapping) = to_case(LOWER_CASE, r, self.0);
        if r1 == r && !had_mapping {
            return to_lower(r);
        }
        r1
    }
}

pub fn simple_fold(r: Rune) -> Rune {
    if !(0..=MAX_RUNE).contains(&r) {
        return r;
    }
    if let Some(&f) = ASCII_FOLD.get(r as usize) {
        return Rune::from(f);
    }
    let mut lo = 0;
    let mut hi = CASE_ORBIT.len();
    while lo < hi {
        let m = (lo + hi) / 2;
        if Rune::from(CASE_ORBIT[m].from) < r {
            lo = m + 1;
        } else {
            hi = m;
        }
    }
    if let Some(pair) = CASE_ORBIT.get(lo)
        && Rune::from(pair.from) == r
    {
        return Rune::from(pair.to);
    }
    if let Some(cr) = lookup_case_range(r, CASE_RANGES) {
        let l = convert_case(LOWER_CASE, r, cr);
        if l != r {
            return l;
        }
        return convert_case(UPPER_CASE, r, cr);
    }
    r
}
