use crate::gocompat::utf8::Rune;

const REPLACEMENT_CHAR: Rune = 0xFFFD;
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
