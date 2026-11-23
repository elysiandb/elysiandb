use crate::utf8::Rune;

const REPLACEMENT_CHAR: Rune = 0xFFFD;
const MAX_RUNE: Rune = 0x10FFFF;
const SURR1: Rune = 0xd800;
const SURR2: Rune = 0xdc00;
const SURR3: Rune = 0xe000;
const SURR_SELF: Rune = 0x10000;

pub fn is_surrogate(r: Rune) -> bool {
    (SURR1..SURR3).contains(&r)
}

pub fn decode_rune(r1: Rune, r2: Rune) -> Rune {
    if (SURR1..SURR2).contains(&r1) && (SURR2..SURR3).contains(&r2) {
        return (((r1 - SURR1) << 10) | (r2 - SURR2)) + SURR_SELF;
    }
    REPLACEMENT_CHAR
}

pub fn encode_rune(r: Rune) -> (Rune, Rune) {
    if !(SURR_SELF..=MAX_RUNE).contains(&r) {
        return (REPLACEMENT_CHAR, REPLACEMENT_CHAR);
    }
    let r = r - SURR_SELF;
    (SURR1 + ((r >> 10) & 0x3ff), SURR2 + (r & 0x3ff))
}

pub fn rune_len(r: Rune) -> Option<usize> {
    if (0..SURR1).contains(&r) || (SURR3..SURR_SELF).contains(&r) {
        Some(1)
    } else if (SURR_SELF..=MAX_RUNE).contains(&r) {
        Some(2)
    } else {
        None
    }
}

pub fn encode(s: &[Rune]) -> Vec<u16> {
    let mut a = Vec::with_capacity(s.len() + s.iter().filter(|&&v| v >= SURR_SELF).count());
    for &v in s {
        append_rune(&mut a, v);
    }
    a
}

pub fn append_rune(a: &mut Vec<u16>, r: Rune) {
    match rune_len(r) {
        Some(1) => a.push(r as u16),
        Some(2) => {
            let (r1, r2) = encode_rune(r);
            a.push(r1 as u16);
            a.push(r2 as u16);
        }
        _ => a.push(REPLACEMENT_CHAR as u16),
    }
}

pub fn decode(s: &[u16]) -> Vec<Rune> {
    let mut buf = Vec::with_capacity(s.len());
    let mut i = 0;
    while i < s.len() {
        let r = Rune::from(s[i]);
        let ar = if !(SURR1..SURR3).contains(&r) {
            r
        } else if (SURR1..SURR2).contains(&r)
            && i + 1 < s.len()
            && (SURR2..SURR3).contains(&Rune::from(s[i + 1]))
        {
            i += 1;
            decode_rune(r, Rune::from(s[i]))
        } else {
            REPLACEMENT_CHAR
        };
        buf.push(ar);
        i += 1;
    }
    buf
}
