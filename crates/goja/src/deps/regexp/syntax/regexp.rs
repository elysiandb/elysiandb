use super::parse::{FOLD_CASE, Flags, MAX_FOLD, MIN_FOLD, NON_GREEDY, WAS_DOLLAR, in_char_class};
use gocompat::strconv;
use gocompat::unicode;
use gocompat::utf8::{self, Rune};
use std::collections::HashMap;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Op {
    NoMatch = 1,
    EmptyMatch = 2,
    Literal = 3,
    CharClass = 4,
    AnyCharNotNL = 5,
    AnyChar = 6,
    BeginLine = 7,
    EndLine = 8,
    BeginText = 9,
    EndText = 10,
    WordBoundary = 11,
    NoWordBoundary = 12,
    Capture = 13,
    Star = 14,
    Plus = 15,
    Quest = 16,
    Repeat = 17,
    Concat = 18,
    Alternate = 19,
    LeftParen = 128,
    VerticalBar = 129,
}

pub const OP_PSEUDO: Op = Op::LeftParen;

#[derive(Clone, Debug)]
pub struct Regexp {
    pub op: Op,
    pub flags: Flags,
    pub sub: Vec<Arc<Regexp>>,
    pub rune: Arc<[Rune]>,
    pub min: i32,
    pub max: i32,
    pub cap: i32,
    pub name: Vec<u8>,
}

impl Regexp {
    pub fn new(op: Op) -> Regexp {
        Regexp {
            op,
            flags: 0,
            sub: Vec::new(),
            rune: Arc::from(Vec::new()),
            min: 0,
            max: 0,
            cap: 0,
            name: Vec::new(),
        }
    }

    pub fn equal(&self, y: &Regexp) -> bool {
        if self.op != y.op {
            return false;
        }
        match self.op {
            Op::EndText => {
                if self.flags & WAS_DOLLAR != y.flags & WAS_DOLLAR {
                    return false;
                }
            }
            Op::Literal | Op::CharClass => return self.rune[..] == y.rune[..],
            Op::Alternate | Op::Concat => {
                return self.sub.len() == y.sub.len()
                    && self.sub.iter().zip(y.sub.iter()).all(|(a, b)| a.equal(b));
            }
            Op::Star | Op::Plus | Op::Quest => {
                if self.flags & NON_GREEDY != y.flags & NON_GREEDY || !self.sub[0].equal(&y.sub[0])
                {
                    return false;
                }
            }
            Op::Repeat => {
                if self.flags & NON_GREEDY != y.flags & NON_GREEDY
                    || self.min != y.min
                    || self.max != y.max
                    || !self.sub[0].equal(&y.sub[0])
                {
                    return false;
                }
            }
            Op::Capture => {
                if self.cap != y.cap || self.name != y.name || !self.sub[0].equal(&y.sub[0]) {
                    return false;
                }
            }
            _ => {}
        }
        true
    }

    pub fn string(&self) -> Vec<u8> {
        let mut b = Vec::new();
        let mut flags: HashMap<*const Regexp, PrintFlags> = HashMap::new();
        let (mut must, cant) = calc_flags(self, &mut flags);
        must |= (cant & !FLAG_I) << NEG_SHIFT;
        if must != 0 {
            must |= FLAG_OFF;
        }
        write_regexp(&mut b, self, must, &flags);
        b
    }

    pub fn max_cap(&self) -> i32 {
        let mut m = 0;
        if self.op == Op::Capture {
            m = self.cap;
        }
        for sub in &self.sub {
            let n = sub.max_cap();
            if m < n {
                m = n;
            }
        }
        m
    }

    pub fn cap_names(&self) -> Vec<Vec<u8>> {
        let mut names = vec![Vec::new(); self.max_cap() as usize + 1];
        self.cap_names_into(&mut names);
        names
    }

    fn cap_names_into(&self, names: &mut [Vec<u8>]) {
        if self.op == Op::Capture {
            names[self.cap as usize] = self.name.clone();
        }
        for sub in &self.sub {
            sub.cap_names_into(names);
        }
    }
}

type PrintFlags = u8;

const FLAG_I: PrintFlags = 1;
const FLAG_M: PrintFlags = 1 << 1;
const FLAG_S: PrintFlags = 1 << 2;
const FLAG_OFF: PrintFlags = 1 << 3;
const FLAG_PREC: PrintFlags = 1 << 4;
const NEG_SHIFT: u32 = 5;

fn key(re: &Regexp) -> *const Regexp {
    re as *const Regexp
}

fn add_span(
    start: &Regexp,
    last: &Regexp,
    f: PrintFlags,
    flags: &mut HashMap<*const Regexp, PrintFlags>,
) {
    flags.insert(key(start), f);
    *flags.entry(key(last)).or_insert(0) |= FLAG_OFF;
}

fn calc_flags(
    re: &Regexp,
    flags: &mut HashMap<*const Regexp, PrintFlags>,
) -> (PrintFlags, PrintFlags) {
    match re.op {
        Op::Literal => {
            for &r in re.rune.iter() {
                if (MIN_FOLD..=MAX_FOLD).contains(&r) && unicode::simple_fold(r) != r {
                    if re.flags & FOLD_CASE != 0 {
                        return (FLAG_I, 0);
                    } else {
                        return (0, FLAG_I);
                    }
                }
            }
            (0, 0)
        }
        Op::CharClass => {
            let mut i = 0;
            while i < re.rune.len() {
                let lo = MIN_FOLD.max(re.rune[i]);
                let hi = MAX_FOLD.min(re.rune[i + 1]);
                let mut r = lo;
                while r <= hi {
                    let mut f = unicode::simple_fold(r);
                    while f != r {
                        if !((lo..=hi).contains(&f) || in_char_class(f, &re.rune)) {
                            return (0, FLAG_I);
                        }
                        f = unicode::simple_fold(f);
                    }
                    r += 1;
                }
                i += 2;
            }
            (0, 0)
        }
        Op::AnyCharNotNL => (0, FLAG_S),
        Op::AnyChar => (FLAG_S, 0),
        Op::BeginLine | Op::EndLine => (FLAG_M, 0),
        Op::EndText => {
            if re.flags & WAS_DOLLAR != 0 {
                return (0, FLAG_M);
            }
            (0, 0)
        }
        Op::Capture | Op::Star | Op::Plus | Op::Quest | Op::Repeat => calc_flags(&re.sub[0], flags),
        Op::Concat | Op::Alternate => {
            let mut must: PrintFlags = 0;
            let mut cant: PrintFlags = 0;
            let mut all_cant: PrintFlags = 0;
            let mut start = 0;
            let mut last = 0;
            let mut did = false;
            for (i, sub) in re.sub.iter().enumerate() {
                let (sub_must, sub_cant) = calc_flags(sub, flags);
                if must & sub_cant != 0 || sub_must & cant != 0 {
                    if must != 0 {
                        add_span(&re.sub[start], &re.sub[last], must, flags);
                    }
                    must = 0;
                    cant = 0;
                    start = i;
                    did = true;
                }
                must |= sub_must;
                cant |= sub_cant;
                all_cant |= sub_cant;
                if sub_must != 0 {
                    last = i;
                }
                if must == 0 && start == i {
                    start += 1;
                }
            }
            if !did {
                return (must, cant);
            }
            if must != 0 {
                add_span(&re.sub[start], &re.sub[last], must, flags);
            }
            (0, all_cant)
        }
        _ => (0, 0),
    }
}

fn write_regexp(
    b: &mut Vec<u8>,
    re: &Regexp,
    f: PrintFlags,
    flags: &HashMap<*const Regexp, PrintFlags>,
) {
    let mut f = f | flags.get(&key(re)).copied().unwrap_or(0);
    if f & FLAG_PREC != 0 && f & !(FLAG_OFF | FLAG_PREC) != 0 && f & FLAG_OFF != 0 {
        f &= !FLAG_PREC;
    }
    if f & !(FLAG_OFF | FLAG_PREC) != 0 {
        b.extend_from_slice(b"(?");
        if f & FLAG_I != 0 {
            b.push(b'i');
        }
        if f & FLAG_M != 0 {
            b.push(b'm');
        }
        if f & FLAG_S != 0 {
            b.push(b's');
        }
        if f & ((FLAG_M | FLAG_S) << NEG_SHIFT) != 0 {
            b.push(b'-');
            if f & (FLAG_M << NEG_SHIFT) != 0 {
                b.push(b'm');
            }
            if f & (FLAG_S << NEG_SHIFT) != 0 {
                b.push(b's');
            }
        }
        b.push(b':');
    }
    let mut closers = 0;
    if f & FLAG_OFF != 0 {
        closers += 1;
    }
    if f & FLAG_PREC != 0 {
        b.extend_from_slice(b"(?:");
        closers += 1;
    }

    match re.op {
        Op::NoMatch => b.extend_from_slice(br"[^\x00-\x{10FFFF}]"),
        Op::EmptyMatch => b.extend_from_slice(b"(?:)"),
        Op::Literal => {
            for &r in re.rune.iter() {
                escape(b, r, false);
            }
        }
        Op::CharClass => {
            let rune = &re.rune;
            if !rune.len().is_multiple_of(2) {
                b.extend_from_slice(b"[invalid char class]");
            } else {
                b.push(b'[');
                if rune.is_empty() {
                    b.extend_from_slice(br"^\x00-\x{10FFFF}");
                } else if rune[0] == 0
                    && rune[rune.len() - 1] == unicode::MAX_RUNE
                    && rune.len() > 2
                {
                    b.push(b'^');
                    let mut i = 1;
                    while i < rune.len() - 1 {
                        let lo = rune[i] + 1;
                        let hi = rune[i + 1] - 1;
                        escape(b, lo, lo == '-' as Rune);
                        if lo != hi {
                            if hi != lo + 1 {
                                b.push(b'-');
                            }
                            escape(b, hi, hi == '-' as Rune);
                        }
                        i += 2;
                    }
                } else {
                    let mut i = 0;
                    while i < rune.len() {
                        let lo = rune[i];
                        let hi = rune[i + 1];
                        escape(b, lo, lo == '-' as Rune);
                        if lo != hi {
                            if hi != lo + 1 {
                                b.push(b'-');
                            }
                            escape(b, hi, hi == '-' as Rune);
                        }
                        i += 2;
                    }
                }
                b.push(b']');
            }
        }
        Op::AnyCharNotNL | Op::AnyChar => b.push(b'.'),
        Op::BeginLine => b.push(b'^'),
        Op::EndLine => b.push(b'$'),
        Op::BeginText => b.extend_from_slice(br"\A"),
        Op::EndText => {
            if re.flags & WAS_DOLLAR != 0 {
                b.push(b'$');
            } else {
                b.extend_from_slice(br"\z");
            }
        }
        Op::WordBoundary => b.extend_from_slice(br"\b"),
        Op::NoWordBoundary => b.extend_from_slice(br"\B"),
        Op::Capture => {
            if !re.name.is_empty() {
                b.extend_from_slice(b"(?P<");
                b.extend_from_slice(&re.name);
                b.push(b'>');
            } else {
                b.push(b'(');
            }
            if re.sub[0].op != Op::EmptyMatch {
                let sf = flags.get(&key(&re.sub[0])).copied().unwrap_or(0);
                write_regexp(b, &re.sub[0], sf, flags);
            }
            b.push(b')');
        }
        Op::Star | Op::Plus | Op::Quest | Op::Repeat => {
            let mut p: PrintFlags = 0;
            let sub = &re.sub[0];
            if sub.op > Op::Capture || sub.op == Op::Literal && sub.rune.len() > 1 {
                p = FLAG_PREC;
            }
            write_regexp(b, sub, p, flags);
            match re.op {
                Op::Star => b.push(b'*'),
                Op::Plus => b.push(b'+'),
                Op::Quest => b.push(b'?'),
                _ => {
                    b.push(b'{');
                    b.extend_from_slice(strconv::itoa(i64::from(re.min)).as_bytes());
                    if re.max != re.min {
                        b.push(b',');
                        if re.max >= 0 {
                            b.extend_from_slice(strconv::itoa(i64::from(re.max)).as_bytes());
                        }
                    }
                    b.push(b'}');
                }
            }
            if re.flags & NON_GREEDY != 0 {
                b.push(b'?');
            }
        }
        Op::Concat => {
            for sub in &re.sub {
                let mut p: PrintFlags = 0;
                if sub.op == Op::Alternate {
                    p = FLAG_PREC;
                }
                write_regexp(b, sub, p, flags);
            }
        }
        Op::Alternate => {
            for (i, sub) in re.sub.iter().enumerate() {
                if i > 0 {
                    b.push(b'|');
                }
                write_regexp(b, sub, 0, flags);
            }
        }
        _ => {
            b.extend_from_slice(b"<invalid op");
            b.extend_from_slice(strconv::itoa(i64::from(re.op as u8)).as_bytes());
            b.push(b'>');
        }
    }
    for _ in 0..closers {
        b.push(b')');
    }
}

const META: &[u8] = br"\.+*?()|[]{}^$";

fn escape(b: &mut Vec<u8>, r: Rune, force: bool) {
    if unicode::is_print(r) {
        if ((0..0x80).contains(&r) && META.contains(&(r as u8))) || force {
            b.push(b'\\');
        }
        utf8::append_rune(b, r);
        return;
    }
    match r {
        0x07 => b.extend_from_slice(br"\a"),
        0x0C => b.extend_from_slice(br"\f"),
        0x0A => b.extend_from_slice(br"\n"),
        0x0D => b.extend_from_slice(br"\r"),
        0x09 => b.extend_from_slice(br"\t"),
        0x0B => b.extend_from_slice(br"\v"),
        _ => {
            if r < 0x100 {
                b.extend_from_slice(br"\x");
                let s = strconv::format_int(i64::from(r), 16);
                if s.len() == 1 {
                    b.push(b'0');
                }
                b.extend_from_slice(s.as_bytes());
                return;
            }
            b.extend_from_slice(br"\x{");
            b.extend_from_slice(strconv::format_int(i64::from(r), 16).as_bytes());
            b.push(b'}');
        }
    }
}
