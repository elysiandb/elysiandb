use std::collections::HashMap;
use std::sync::OnceLock;

use gocompat::sort;
use gocompat::unicode::{self, RangeTable};
use gocompat::utf8::{self, Rune};

use super::escape::escape_rune;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct CharSet {
    pub(crate) ranges: Vec<SingleRange>,
    pub(crate) categories: Vec<Category>,
    pub(crate) sub: Option<Box<CharSet>>,
    pub(crate) negate: bool,
    pub(crate) anything: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Category {
    pub(crate) negate: bool,
    pub(crate) cat: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub(crate) struct SingleRange {
    pub(crate) first: Rune,
    pub(crate) last: Rune,
}

const SPACE_CATEGORY_TEXT: &str = " ";
const WORD_CATEGORY_TEXT: &str = "W";

const ECMA_SPACE: &[Rune] = &[
    0x0009, 0x000e, 0x0020, 0x0021, 0x00a0, 0x00a1, 0x1680, 0x1681, 0x2000, 0x200b, 0x2028, 0x202a,
    0x202f, 0x2030, 0x205f, 0x2060, 0x3000, 0x3001, 0xfeff, 0xff00,
];
const ECMA_WORD: &[Rune] = &[
    0x0030, 0x003a, 0x0041, 0x005b, 0x005f, 0x0060, 0x0061, 0x007b,
];
const ECMA_DIGIT: &[Rune] = &[0x0030, 0x003a];
const RE2_SPACE: &[Rune] = &[0x0009, 0x000b, 0x000c, 0x000e, 0x0020, 0x0021];

fn template(cell: &'static OnceLock<CharSet>, build: fn() -> CharSet) -> CharSet {
    cell.get_or_init(build).clone()
}

pub fn any_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(&[0], false))
}

pub fn ecma_any_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || {
        get_char_set_from_old_string(&[0, 0x000a, 0x000b, 0x000d, 0x000e], false)
    })
}

pub fn none_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(&[], false))
}

pub fn ecma_word_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(ECMA_WORD, false))
}

pub fn not_ecma_word_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(ECMA_WORD, true))
}

pub fn ecma_space_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(ECMA_SPACE, false))
}

pub fn not_ecma_space_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(ECMA_SPACE, true))
}

pub fn ecma_digit_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(ECMA_DIGIT, false))
}

pub fn not_ecma_digit_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(ECMA_DIGIT, true))
}

pub fn word_class() -> CharSet {
    get_char_set_from_category_string(false, false, &[WORD_CATEGORY_TEXT])
}

pub fn not_word_class() -> CharSet {
    get_char_set_from_category_string(true, false, &[WORD_CATEGORY_TEXT])
}

pub fn space_class() -> CharSet {
    get_char_set_from_category_string(false, false, &[SPACE_CATEGORY_TEXT])
}

pub fn not_space_class() -> CharSet {
    get_char_set_from_category_string(true, false, &[SPACE_CATEGORY_TEXT])
}

pub fn digit_class() -> CharSet {
    get_char_set_from_category_string(false, false, &["Nd"])
}

pub fn not_digit_class() -> CharSet {
    get_char_set_from_category_string(false, true, &["Nd"])
}

pub fn re2_space_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(RE2_SPACE, false))
}

pub fn not_re2_space_class() -> CharSet {
    static C: OnceLock<CharSet> = OnceLock::new();
    template(&C, || get_char_set_from_old_string(RE2_SPACE, true))
}

pub(crate) fn unicode_categories() -> &'static HashMap<&'static str, &'static RangeTable> {
    static M: OnceLock<HashMap<&'static str, &'static RangeTable>> = OnceLock::new();
    M.get_or_init(|| {
        let mut m = HashMap::new();
        for &(k, v) in unicode::SCRIPTS {
            m.insert(k, v);
        }
        for &(k, v) in unicode::CATEGORIES {
            m.insert(k, v);
        }
        for &(k, v) in unicode::PROPERTIES {
            m.insert(k, v);
        }
        m
    })
}

fn get_char_set_from_category_string(negate_set: bool, negate_cat: bool, cats: &[&str]) -> CharSet {
    if negate_cat && negate_set {
        panic!("a constant set negates both the set and its categories");
    }
    let mut c = CharSet {
        negate: negate_set,
        ..CharSet::default()
    };
    c.categories = cats
        .iter()
        .map(|cat| Category {
            cat: (*cat).to_string(),
            negate: negate_cat,
        })
        .collect();
    c
}

fn get_char_set_from_old_string(set_text: &[Rune], negate: bool) -> CharSet {
    let mut c = CharSet::default();
    let mut set_text = set_text;
    if !set_text.is_empty() {
        let mut fill_first = false;
        let mut l = set_text.len();
        if negate {
            if set_text[0] == 0 {
                set_text = &set_text[1..];
            } else {
                l += 1;
                fill_first = true;
            }
        }
        if l.is_multiple_of(2) {
            c.ranges = vec![SingleRange::default(); l / 2];
        } else {
            c.ranges = vec![SingleRange::default(); l / 2 + 1];
        }
        let mut first = true;
        if fill_first {
            c.ranges[0] = SingleRange { first: 0, last: 0 };
            first = false;
        }
        let mut i = 0;
        for &r in set_text {
            if first {
                c.ranges[i] = SingleRange { first: r, last: 0 };
                first = false;
            } else {
                c.ranges[i].last = r - 1;
                i += 1;
                first = true;
            }
        }
        if !first {
            c.ranges[i].last = utf8::MAX_RUNE;
        }
    }
    c
}

impl CharSet {
    pub fn copy(&self) -> CharSet {
        let mut ret = CharSet {
            anything: self.anything,
            negate: self.negate,
            ..CharSet::default()
        };
        ret.ranges.extend_from_slice(&self.ranges);
        ret.categories.extend_from_slice(&self.categories);
        if let Some(sub) = &self.sub {
            ret.sub = Some(Box::new(sub.copy()));
        }
        ret
    }

    pub fn string(&self) -> String {
        let mut buf = String::from("[");
        if self.is_negated() {
            buf.push('^');
        }
        for r in &self.ranges {
            buf.push_str(&char_description(r.first));
            if r.first != r.last {
                if r.last.wrapping_sub(r.first) != 1 {
                    buf.push('-');
                }
                buf.push_str(&char_description(r.last));
            }
        }
        for c in &self.categories {
            buf.push_str(&c.string());
        }
        if let Some(sub) = &self.sub {
            buf.push('-');
            buf.push_str(&sub.string());
        }
        buf.push(']');
        buf
    }

    pub(crate) fn map_hash_fill(&self, buf: &mut Vec<u8>) {
        if self.negate {
            buf.push(0);
        } else {
            buf.push(1);
        }
        for r in &self.ranges {
            write_rune(buf, r.first);
            write_rune(buf, r.last);
        }
        for ct in &self.categories {
            buf.extend_from_slice(ct.cat.as_bytes());
            if ct.negate {
                buf.push(1);
            } else {
                buf.push(0);
            }
        }
        if let Some(sub) = &self.sub {
            sub.map_hash_fill(buf);
        }
    }

    pub fn char_in(&self, ch: Rune) -> bool {
        let mut val = false;
        for r in &self.ranges {
            if ch < r.first {
                continue;
            }
            if ch <= r.last {
                val = true;
                break;
            }
        }
        if !val && !self.categories.is_empty() {
            for ct in &self.categories {
                if ct.cat == SPACE_CATEGORY_TEXT {
                    if unicode::is_space(ch) {
                        val = !ct.negate;
                        break;
                    } else if ct.negate {
                        val = true;
                        break;
                    }
                } else if ct.cat == WORD_CATEGORY_TEXT {
                    if is_word_char(ch) {
                        val = !ct.negate;
                        break;
                    } else if ct.negate {
                        val = true;
                        break;
                    }
                } else if unicode::is(unicode_categories()[ct.cat.as_str()], ch) {
                    val = !ct.negate;
                    break;
                } else if ct.negate {
                    val = true;
                    break;
                }
            }
        }
        if self.negate {
            val = !val;
        }
        if val && let Some(sub) = &self.sub {
            val = !sub.char_in(ch);
        }
        val
    }

    pub fn singleton_char(&self) -> Rune {
        self.ranges[0].first
    }

    pub fn is_singleton(&self) -> bool {
        !self.negate
            && self.categories.is_empty()
            && self.ranges.len() == 1
            && self.sub.is_none()
            && self.ranges[0].first == self.ranges[0].last
    }

    pub fn is_singleton_inverse(&self) -> bool {
        self.negate
            && self.categories.is_empty()
            && self.ranges.len() == 1
            && self.sub.is_none()
            && self.ranges[0].first == self.ranges[0].last
    }

    pub fn is_mergeable(&self) -> bool {
        !self.is_negated() && !self.has_subtraction()
    }

    pub fn is_negated(&self) -> bool {
        self.negate
    }

    pub fn has_subtraction(&self) -> bool {
        self.sub.is_some()
    }

    pub fn is_empty(&self) -> bool {
        self.ranges.is_empty() && self.categories.is_empty() && self.sub.is_none()
    }

    pub(crate) fn add_digit(&mut self, ecma: bool, negate: bool) {
        if ecma {
            if negate {
                self.add_ranges(&not_ecma_digit_class().ranges);
            } else {
                self.add_ranges(&ecma_digit_class().ranges);
            }
        } else {
            self.add_categories(&[Category {
                cat: "Nd".to_string(),
                negate,
            }]);
        }
    }

    pub(crate) fn add_char(&mut self, ch: Rune) {
        self.add_range(ch, ch);
    }

    pub(crate) fn add_space(&mut self, ecma: bool, re2: bool, negate: bool) {
        if ecma {
            if negate {
                self.add_ranges(&not_ecma_space_class().ranges);
            } else {
                self.add_ranges(&ecma_space_class().ranges);
            }
        } else if re2 {
            if negate {
                self.add_ranges(&not_re2_space_class().ranges);
            } else {
                self.add_ranges(&re2_space_class().ranges);
            }
        } else {
            self.add_categories(&[Category {
                cat: SPACE_CATEGORY_TEXT.to_string(),
                negate,
            }]);
        }
    }

    pub(crate) fn add_word(&mut self, ecma: bool, negate: bool) {
        if ecma {
            if negate {
                self.add_ranges(&not_ecma_word_class().ranges);
            } else {
                self.add_ranges(&ecma_word_class().ranges);
            }
        } else {
            self.add_categories(&[Category {
                cat: WORD_CATEGORY_TEXT.to_string(),
                negate,
            }]);
        }
    }

    pub(crate) fn add_set(&mut self, set: &CharSet) {
        if self.anything {
            return;
        }
        if set.anything {
            self.make_anything();
            return;
        }
        self.ranges.extend_from_slice(&set.ranges);
        self.add_categories(&set.categories);
        self.canonicalize();
    }

    fn make_anything(&mut self) {
        self.anything = true;
        self.categories = Vec::new();
        self.ranges = any_class().ranges;
    }

    fn add_categories(&mut self, cats: &[Category]) {
        if self.anything {
            return;
        }
        for ct in cats {
            let mut found = false;
            for ct2 in &self.categories {
                if ct.cat == ct2.cat {
                    if ct.negate != ct2.negate {
                        self.make_anything();
                        return;
                    }
                    found = true;
                    break;
                }
            }
            if !found {
                self.categories.push(ct.clone());
            }
        }
    }

    pub(crate) fn add_ranges(&mut self, ranges: &[SingleRange]) {
        if self.anything {
            return;
        }
        self.ranges.extend_from_slice(ranges);
        self.canonicalize();
    }

    fn add_negative_ranges(&mut self, ranges: &[SingleRange]) {
        if self.anything {
            return;
        }
        let mut hi: Rune = 0;
        for r in ranges {
            if hi < r.first {
                self.ranges.push(SingleRange {
                    first: hi,
                    last: r.first - 1,
                });
            }
            hi = r.last + 1;
        }
        if hi < utf8::MAX_RUNE {
            self.ranges.push(SingleRange {
                first: hi,
                last: utf8::MAX_RUNE,
            });
        }
        self.canonicalize();
    }

    pub(crate) fn add_category(
        &mut self,
        category_name: &str,
        negate: bool,
        case_insensitive: bool,
    ) {
        if !is_valid_unicode_cat(category_name) {
            panic!("unknown unicode category, script, or property '{category_name}'");
        }
        if case_insensitive
            && (category_name == "Ll" || category_name == "Lu" || category_name == "Lt")
        {
            self.add_categories(&[
                Category {
                    cat: "Ll".to_string(),
                    negate,
                },
                Category {
                    cat: "Lu".to_string(),
                    negate,
                },
                Category {
                    cat: "Lt".to_string(),
                    negate,
                },
            ]);
        }
        self.add_categories(&[Category {
            cat: category_name.to_string(),
            negate,
        }]);
    }

    pub(crate) fn add_subtraction(&mut self, sub: CharSet) {
        self.sub = Some(Box::new(sub));
    }

    pub(crate) fn add_range(&mut self, ch_min: Rune, ch_max: Rune) {
        self.ranges.push(SingleRange {
            first: ch_min,
            last: ch_max,
        });
        self.canonicalize();
    }

    pub(crate) fn add_named_ascii(&mut self, name: &str, negate: bool) -> bool {
        let rs: Vec<SingleRange> = match name {
            "alnum" => vec![sr('0', '9'), sr('A', 'Z'), sr('a', 'z')],
            "alpha" => vec![sr('A', 'Z'), sr('a', 'z')],
            "ascii" => vec![SingleRange {
                first: 0,
                last: 0x7f,
            }],
            "blank" => vec![sr('\t', '\t'), sr(' ', ' ')],
            "cntrl" => vec![
                SingleRange {
                    first: 0,
                    last: 0x1f,
                },
                SingleRange {
                    first: 0x7f,
                    last: 0x7f,
                },
            ],
            "digit" => {
                self.add_digit(false, negate);
                Vec::new()
            }
            "graph" => vec![sr('!', '~')],
            "lower" => vec![sr('a', 'z')],
            "print" => vec![sr(' ', '~')],
            "punct" => vec![sr('!', '/'), sr(':', '@'), sr('[', '`'), sr('{', '~')],
            "space" => {
                self.add_space(true, false, negate);
                Vec::new()
            }
            "upper" => vec![sr('A', 'Z')],
            "word" => {
                self.add_word(true, negate);
                Vec::new()
            }
            "xdigit" => vec![sr('0', '9'), sr('A', 'F'), sr('a', 'f')],
            _ => return false,
        };
        if !rs.is_empty() {
            if negate {
                self.add_negative_ranges(&rs);
            } else {
                self.add_ranges(&rs);
            }
        }
        true
    }

    pub(crate) fn canonicalize(&mut self) {
        if self.ranges.len() > 1 {
            sort::slice(&mut self.ranges, |a, b| a.first < b.first);
            let ranges = &mut self.ranges;
            let mut i = 1usize;
            let mut j = 0usize;
            loop {
                let mut last = ranges[j].last;
                let mut done = false;
                loop {
                    if i == ranges.len() || last == utf8::MAX_RUNE {
                        done = true;
                        break;
                    }
                    let current = ranges[i];
                    if current.first > last.wrapping_add(1) {
                        break;
                    }
                    if last < current.last {
                        last = current.last;
                    }
                    i += 1;
                }
                ranges[j] = SingleRange {
                    first: ranges[j].first,
                    last,
                };
                j += 1;
                if done {
                    break;
                }
                if j < i {
                    ranges[j] = ranges[i];
                }
                i += 1;
            }
            ranges.truncate(j);
        }
    }

    pub(crate) fn add_lowercase(&mut self) {
        if self.anything {
            return;
        }
        let mut to_add = Vec::new();
        for i in 0..self.ranges.len() {
            let r = self.ranges[i];
            if r.first == r.last {
                let lower = unicode::to_lower(r.first);
                self.ranges[i] = SingleRange {
                    first: lower,
                    last: lower,
                };
            } else {
                to_add.push(r);
            }
        }
        for r in to_add {
            self.add_lowercase_range(r.first, r.last);
        }
        self.canonicalize();
    }

    fn add_lowercase_range(&mut self, ch_min: Rune, ch_max: Rune) {
        let mut i = 0usize;
        let mut i_max = LC_TABLE.len();
        while i < i_max {
            let i_mid = (i + i_max) / 2;
            if LC_TABLE[i_mid].ch_max < ch_min {
                i = i_mid + 1;
            } else {
                i_max = i_mid;
            }
        }
        while i < LC_TABLE.len() {
            let lc = &LC_TABLE[i];
            if lc.ch_min > ch_max {
                return;
            }
            let mut ch_min_t = lc.ch_min;
            if ch_min_t < ch_min {
                ch_min_t = ch_min;
            }
            let mut ch_max_t = lc.ch_max;
            if ch_max_t > ch_max {
                ch_max_t = ch_max;
            }
            match lc.op {
                LOWERCASE_SET => {
                    ch_min_t = lc.data;
                    ch_max_t = lc.data;
                }
                LOWERCASE_ADD => {
                    ch_min_t += lc.data;
                    ch_max_t += lc.data;
                }
                LOWERCASE_BOR => {
                    ch_min_t |= 1;
                    ch_max_t |= 1;
                }
                LOWERCASE_BAD => {
                    ch_min_t += ch_min_t & 1;
                    ch_max_t += ch_max_t & 1;
                }
                _ => {}
            }
            if ch_min_t < ch_min || ch_max_t > ch_max {
                self.add_range(ch_min_t, ch_max_t);
            }
            i += 1;
        }
    }
}

fn sr(a: char, b: char) -> SingleRange {
    SingleRange {
        first: a as Rune,
        last: b as Rune,
    }
}

fn write_rune(buf: &mut Vec<u8>, r: Rune) {
    if (r as u32) < u32::from(utf8::RUNE_SELF) {
        buf.push(r as u8);
    } else {
        utf8::append_rune(buf, r);
    }
}

impl Category {
    pub(crate) fn string(&self) -> String {
        match self.cat.as_str() {
            SPACE_CATEGORY_TEXT => {
                if self.negate {
                    return "\\S".to_string();
                }
                return "\\s".to_string();
            }
            WORD_CATEGORY_TEXT => {
                if self.negate {
                    return "\\W".to_string();
                }
                return "\\w".to_string();
            }
            _ => {}
        }
        if unicode_categories().contains_key(self.cat.as_str()) {
            if self.negate {
                return format!("\\P{{{}}}", self.cat);
            }
            return format!("\\p{{{}}}", self.cat);
        }
        format!("Unknown category: {}", self.cat)
    }
}

pub fn char_description(ch: Rune) -> String {
    let mut b = String::new();
    escape_rune(&mut b, ch, false);
    b
}

pub fn is_word_char(r: Rune) -> bool {
    unicode::in_(r, &[&unicode::L, &unicode::MN, &unicode::ND, &unicode::PC])
        || r == 0x200D
        || r == 0x200C
}

pub fn is_ecma_word_char(r: Rune) -> bool {
    unicode::in_(r, &[&unicode::L, &unicode::MN, &unicode::ND, &unicode::PC])
}

pub(crate) fn is_valid_unicode_cat(cat_name: &str) -> bool {
    unicode_categories().contains_key(cat_name)
}

pub const LOWERCASE_SET: i32 = 0;
pub const LOWERCASE_ADD: i32 = 1;
pub const LOWERCASE_BOR: i32 = 2;
pub const LOWERCASE_BAD: i32 = 3;

struct LcMap {
    ch_min: Rune,
    ch_max: Rune,
    op: i32,
    data: i32,
}

const fn lc(ch_min: Rune, ch_max: Rune, op: i32, data: i32) -> LcMap {
    LcMap {
        ch_min,
        ch_max,
        op,
        data,
    }
}

static LC_TABLE: &[LcMap] = &[
    lc(0x0041, 0x005A, LOWERCASE_ADD, 32),
    lc(0x00C0, 0x00DE, LOWERCASE_ADD, 32),
    lc(0x0100, 0x012E, LOWERCASE_BOR, 0),
    lc(0x0130, 0x0130, LOWERCASE_SET, 0x0069),
    lc(0x0132, 0x0136, LOWERCASE_BOR, 0),
    lc(0x0139, 0x0147, LOWERCASE_BAD, 0),
    lc(0x014A, 0x0176, LOWERCASE_BOR, 0),
    lc(0x0178, 0x0178, LOWERCASE_SET, 0x00FF),
    lc(0x0179, 0x017D, LOWERCASE_BAD, 0),
    lc(0x0181, 0x0181, LOWERCASE_SET, 0x0253),
    lc(0x0182, 0x0184, LOWERCASE_BOR, 0),
    lc(0x0186, 0x0186, LOWERCASE_SET, 0x0254),
    lc(0x0187, 0x0187, LOWERCASE_SET, 0x0188),
    lc(0x0189, 0x018A, LOWERCASE_ADD, 205),
    lc(0x018B, 0x018B, LOWERCASE_SET, 0x018C),
    lc(0x018E, 0x018E, LOWERCASE_SET, 0x01DD),
    lc(0x018F, 0x018F, LOWERCASE_SET, 0x0259),
    lc(0x0190, 0x0190, LOWERCASE_SET, 0x025B),
    lc(0x0191, 0x0191, LOWERCASE_SET, 0x0192),
    lc(0x0193, 0x0193, LOWERCASE_SET, 0x0260),
    lc(0x0194, 0x0194, LOWERCASE_SET, 0x0263),
    lc(0x0196, 0x0196, LOWERCASE_SET, 0x0269),
    lc(0x0197, 0x0197, LOWERCASE_SET, 0x0268),
    lc(0x0198, 0x0198, LOWERCASE_SET, 0x0199),
    lc(0x019C, 0x019C, LOWERCASE_SET, 0x026F),
    lc(0x019D, 0x019D, LOWERCASE_SET, 0x0272),
    lc(0x019F, 0x019F, LOWERCASE_SET, 0x0275),
    lc(0x01A0, 0x01A4, LOWERCASE_BOR, 0),
    lc(0x01A7, 0x01A7, LOWERCASE_SET, 0x01A8),
    lc(0x01A9, 0x01A9, LOWERCASE_SET, 0x0283),
    lc(0x01AC, 0x01AC, LOWERCASE_SET, 0x01AD),
    lc(0x01AE, 0x01AE, LOWERCASE_SET, 0x0288),
    lc(0x01AF, 0x01AF, LOWERCASE_SET, 0x01B0),
    lc(0x01B1, 0x01B2, LOWERCASE_ADD, 217),
    lc(0x01B3, 0x01B5, LOWERCASE_BAD, 0),
    lc(0x01B7, 0x01B7, LOWERCASE_SET, 0x0292),
    lc(0x01B8, 0x01B8, LOWERCASE_SET, 0x01B9),
    lc(0x01BC, 0x01BC, LOWERCASE_SET, 0x01BD),
    lc(0x01C4, 0x01C5, LOWERCASE_SET, 0x01C6),
    lc(0x01C7, 0x01C8, LOWERCASE_SET, 0x01C9),
    lc(0x01CA, 0x01CB, LOWERCASE_SET, 0x01CC),
    lc(0x01CD, 0x01DB, LOWERCASE_BAD, 0),
    lc(0x01DE, 0x01EE, LOWERCASE_BOR, 0),
    lc(0x01F1, 0x01F2, LOWERCASE_SET, 0x01F3),
    lc(0x01F4, 0x01F4, LOWERCASE_SET, 0x01F5),
    lc(0x01FA, 0x0216, LOWERCASE_BOR, 0),
    lc(0x0386, 0x0386, LOWERCASE_SET, 0x03AC),
    lc(0x0388, 0x038A, LOWERCASE_ADD, 37),
    lc(0x038C, 0x038C, LOWERCASE_SET, 0x03CC),
    lc(0x038E, 0x038F, LOWERCASE_ADD, 63),
    lc(0x0391, 0x03AB, LOWERCASE_ADD, 32),
    lc(0x03E2, 0x03EE, LOWERCASE_BOR, 0),
    lc(0x0401, 0x040F, LOWERCASE_ADD, 80),
    lc(0x0410, 0x042F, LOWERCASE_ADD, 32),
    lc(0x0460, 0x0480, LOWERCASE_BOR, 0),
    lc(0x0490, 0x04BE, LOWERCASE_BOR, 0),
    lc(0x04C1, 0x04C3, LOWERCASE_BAD, 0),
    lc(0x04C7, 0x04C7, LOWERCASE_SET, 0x04C8),
    lc(0x04CB, 0x04CB, LOWERCASE_SET, 0x04CC),
    lc(0x04D0, 0x04EA, LOWERCASE_BOR, 0),
    lc(0x04EE, 0x04F4, LOWERCASE_BOR, 0),
    lc(0x04F8, 0x04F8, LOWERCASE_SET, 0x04F9),
    lc(0x0531, 0x0556, LOWERCASE_ADD, 48),
    lc(0x10A0, 0x10C5, LOWERCASE_ADD, 48),
    lc(0x1E00, 0x1EF8, LOWERCASE_BOR, 0),
    lc(0x1F08, 0x1F0F, LOWERCASE_ADD, -8),
    lc(0x1F18, 0x1F1F, LOWERCASE_ADD, -8),
    lc(0x1F28, 0x1F2F, LOWERCASE_ADD, -8),
    lc(0x1F38, 0x1F3F, LOWERCASE_ADD, -8),
    lc(0x1F48, 0x1F4D, LOWERCASE_ADD, -8),
    lc(0x1F59, 0x1F59, LOWERCASE_SET, 0x1F51),
    lc(0x1F5B, 0x1F5B, LOWERCASE_SET, 0x1F53),
    lc(0x1F5D, 0x1F5D, LOWERCASE_SET, 0x1F55),
    lc(0x1F5F, 0x1F5F, LOWERCASE_SET, 0x1F57),
    lc(0x1F68, 0x1F6F, LOWERCASE_ADD, -8),
    lc(0x1F88, 0x1F8F, LOWERCASE_ADD, -8),
    lc(0x1F98, 0x1F9F, LOWERCASE_ADD, -8),
    lc(0x1FA8, 0x1FAF, LOWERCASE_ADD, -8),
    lc(0x1FB8, 0x1FB9, LOWERCASE_ADD, -8),
    lc(0x1FBA, 0x1FBB, LOWERCASE_ADD, -74),
    lc(0x1FBC, 0x1FBC, LOWERCASE_SET, 0x1FB3),
    lc(0x1FC8, 0x1FCB, LOWERCASE_ADD, -86),
    lc(0x1FCC, 0x1FCC, LOWERCASE_SET, 0x1FC3),
    lc(0x1FD8, 0x1FD9, LOWERCASE_ADD, -8),
    lc(0x1FDA, 0x1FDB, LOWERCASE_ADD, -100),
    lc(0x1FE8, 0x1FE9, LOWERCASE_ADD, -8),
    lc(0x1FEA, 0x1FEB, LOWERCASE_ADD, -112),
    lc(0x1FEC, 0x1FEC, LOWERCASE_SET, 0x1FE5),
    lc(0x1FF8, 0x1FF9, LOWERCASE_ADD, -128),
    lc(0x1FFA, 0x1FFB, LOWERCASE_ADD, -126),
    lc(0x1FFC, 0x1FFC, LOWERCASE_SET, 0x1FF3),
    lc(0x2160, 0x216F, LOWERCASE_ADD, 16),
    lc(0x24B6, 0x24D0, LOWERCASE_ADD, 26),
    lc(0xFF21, 0xFF3A, LOWERCASE_ADD, 32),
];
