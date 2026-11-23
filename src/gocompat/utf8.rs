pub type Rune = i32;

pub const RUNE_ERROR: Rune = 0xFFFD;
pub const RUNE_SELF: u8 = 0x80;
pub const MAX_RUNE: Rune = 0x10FFFF;
pub const UTF_MAX: usize = 4;

const SURROGATE_MIN: u32 = 0xD800;
const SURROGATE_MAX: u32 = 0xDFFF;
const T2: u8 = 0b1100_0000;
const T3: u8 = 0b1110_0000;
const T4: u8 = 0b1111_0000;
const TX: u8 = 0b1000_0000;
const MASKX: u8 = 0b0011_1111;
const MASK2: u8 = 0b0001_1111;
const MASK3: u8 = 0b0000_1111;
const MASK4: u8 = 0b0000_0111;
const RUNE1_MAX: u32 = (1 << 7) - 1;
const RUNE2_MAX: u32 = (1 << 11) - 1;
const RUNE3_MAX: u32 = (1 << 16) - 1;
const LOCB: u8 = 0b1000_0000;
const HICB: u8 = 0b1011_1111;

fn lead(p0: u8) -> Option<(usize, u8, u8)> {
    match p0 {
        0xC2..=0xDF => Some((2, LOCB, HICB)),
        0xE0 => Some((3, 0xA0, HICB)),
        0xE1..=0xEC | 0xEE..=0xEF => Some((3, LOCB, HICB)),
        0xED => Some((3, LOCB, 0x9F)),
        0xF0 => Some((4, 0x90, HICB)),
        0xF1..=0xF3 => Some((4, LOCB, HICB)),
        0xF4 => Some((4, LOCB, 0x8F)),
        _ => None,
    }
}

pub fn decode_rune(p: &[u8]) -> (Rune, usize) {
    let Some(&p0) = p.first() else {
        return (RUNE_ERROR, 0);
    };
    if p0 < RUNE_SELF {
        return (Rune::from(p0), 1);
    }
    let Some((sz, lo, hi)) = lead(p0) else {
        return (RUNE_ERROR, 1);
    };
    if p.len() < sz {
        return (RUNE_ERROR, 1);
    }
    let b1 = p[1];
    if b1 < lo || hi < b1 {
        return (RUNE_ERROR, 1);
    }
    if sz <= 2 {
        return ((Rune::from(p0 & MASK2) << 6) | Rune::from(b1 & MASKX), 2);
    }
    let b2 = p[2];
    if !(LOCB..=HICB).contains(&b2) {
        return (RUNE_ERROR, 1);
    }
    if sz <= 3 {
        return (
            (Rune::from(p0 & MASK3) << 12) | (Rune::from(b1 & MASKX) << 6) | Rune::from(b2 & MASKX),
            3,
        );
    }
    let b3 = p[3];
    if !(LOCB..=HICB).contains(&b3) {
        return (RUNE_ERROR, 1);
    }
    (
        (Rune::from(p0 & MASK4) << 18)
            | (Rune::from(b1 & MASKX) << 12)
            | (Rune::from(b2 & MASKX) << 6)
            | Rune::from(b3 & MASKX),
        4,
    )
}

pub fn append_rune(p: &mut Vec<u8>, r: Rune) {
    let i = r as u32;
    if i <= RUNE1_MAX {
        p.push(r as u8);
        return;
    }
    if i <= RUNE2_MAX {
        p.extend_from_slice(&[T2 | (r >> 6) as u8, TX | (r as u8 & MASKX)]);
        return;
    }
    let r = if i > MAX_RUNE as u32 || (SURROGATE_MIN..=SURROGATE_MAX).contains(&i) {
        RUNE_ERROR
    } else {
        r
    };
    if (r as u32) <= RUNE3_MAX {
        p.extend_from_slice(&[
            T3 | (r >> 12) as u8,
            TX | ((r >> 6) as u8 & MASKX),
            TX | (r as u8 & MASKX),
        ]);
        return;
    }
    p.extend_from_slice(&[
        T4 | (r >> 18) as u8,
        TX | ((r >> 12) as u8 & MASKX),
        TX | ((r >> 6) as u8 & MASKX),
        TX | (r as u8 & MASKX),
    ]);
}
