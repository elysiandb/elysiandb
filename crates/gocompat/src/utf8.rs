pub type Rune = i32;

pub const RUNE_ERROR: Rune = 0xFFFD;
pub const RUNE_SELF: u8 = 0x80;
pub const MAX_RUNE: Rune = 0x10FFFF;
pub const UTF_MAX: usize = 4;

const SURROGATE_MIN: u32 = 0xD800;
const SURROGATE_MAX: u32 = 0xDFFF;

const TX: u8 = 0b1000_0000;
const T2: u8 = 0b1100_0000;
const T3: u8 = 0b1110_0000;
const T4: u8 = 0b1111_0000;

const MASKX: u8 = 0b0011_1111;
const MASK2: u8 = 0b0001_1111;
const MASK3: u8 = 0b0000_1111;
const MASK4: u8 = 0b0000_0111;

const RUNE1_MAX: u32 = (1 << 7) - 1;
const RUNE2_MAX: u32 = (1 << 11) - 1;
const RUNE3_MAX: u32 = (1 << 16) - 1;

const LOCB: u8 = 0b1000_0000;
const HICB: u8 = 0b1011_1111;

const XX: u8 = 0xF1;
const AS: u8 = 0xF0;
const S1: u8 = 0x02;
const S2: u8 = 0x13;
const S3: u8 = 0x03;
const S4: u8 = 0x23;
const S5: u8 = 0x34;
const S6: u8 = 0x04;
const S7: u8 = 0x44;

const RUNE_ERROR_BYTES: [u8; 3] = [0xEF, 0xBF, 0xBD];

const fn first_class(b: u8) -> u8 {
    match b {
        0x00..=0x7F => AS,
        0xC2..=0xDF => S1,
        0xE0 => S2,
        0xE1..=0xEC | 0xEE..=0xEF => S3,
        0xED => S4,
        0xF0 => S5,
        0xF1..=0xF3 => S6,
        0xF4 => S7,
        _ => XX,
    }
}

static FIRST: [u8; 256] = {
    let mut t = [0u8; 256];
    let mut i = 0;
    while i < 256 {
        t[i] = first_class(i as u8);
        i += 1;
    }
    t
};

#[derive(Clone, Copy)]
struct AcceptRange {
    lo: u8,
    hi: u8,
}

static ACCEPT_RANGES: [AcceptRange; 16] = {
    let mut t = [AcceptRange { lo: 0, hi: 0 }; 16];
    t[0] = AcceptRange { lo: LOCB, hi: HICB };
    t[1] = AcceptRange { lo: 0xA0, hi: HICB };
    t[2] = AcceptRange { lo: LOCB, hi: 0x9F };
    t[3] = AcceptRange { lo: 0x90, hi: HICB };
    t[4] = AcceptRange { lo: LOCB, hi: 0x8F };
    t
};

fn outside(b: u8, lo: u8, hi: u8) -> bool {
    b < lo || hi < b
}

pub fn full_rune(p: &[u8]) -> bool {
    let n = p.len();
    if n == 0 {
        return false;
    }
    let x = FIRST[p[0] as usize];
    if n >= usize::from(x & 7) {
        return true;
    }
    let accept = ACCEPT_RANGES[usize::from(x >> 4)];
    if n > 1 && outside(p[1], accept.lo, accept.hi) {
        return true;
    }
    n > 2 && outside(p[2], LOCB, HICB)
}

pub fn decode_rune(p: &[u8]) -> (Rune, usize) {
    let n = p.len();
    if n < 1 {
        return (RUNE_ERROR, 0);
    }
    let p0 = p[0];
    let x = FIRST[p0 as usize];
    if x >= AS {
        if x == AS {
            return (Rune::from(p0), 1);
        }
        return (RUNE_ERROR, 1);
    }
    let sz = usize::from(x & 7);
    let accept = ACCEPT_RANGES[usize::from(x >> 4)];
    if n < sz {
        return (RUNE_ERROR, 1);
    }
    let b1 = p[1];
    if outside(b1, accept.lo, accept.hi) {
        return (RUNE_ERROR, 1);
    }
    if sz <= 2 {
        return ((Rune::from(p0 & MASK2) << 6) | Rune::from(b1 & MASKX), 2);
    }
    let b2 = p[2];
    if outside(b2, LOCB, HICB) {
        return (RUNE_ERROR, 1);
    }
    if sz <= 3 {
        return (
            (Rune::from(p0 & MASK3) << 12) | (Rune::from(b1 & MASKX) << 6) | Rune::from(b2 & MASKX),
            3,
        );
    }
    let b3 = p[3];
    if outside(b3, LOCB, HICB) {
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

pub fn decode_last_rune(p: &[u8]) -> (Rune, usize) {
    let end = p.len();
    if end == 0 {
        return (RUNE_ERROR, 0);
    }
    let last = p[end - 1];
    if last < RUNE_SELF {
        return (Rune::from(last), 1);
    }
    let lim = end.saturating_sub(UTF_MAX) as isize;
    let mut start = end as isize - 2;
    while start >= lim {
        if rune_start(p[start as usize]) {
            break;
        }
        start -= 1;
    }
    let start = start.max(0) as usize;
    let (r, size) = decode_rune(&p[start..end]);
    if start + size != end {
        return (RUNE_ERROR, 1);
    }
    (r, size)
}

pub fn rune_len(r: Rune) -> Option<usize> {
    if r < 0 {
        return None;
    }
    let i = r as u32;
    if i <= RUNE1_MAX {
        Some(1)
    } else if i <= RUNE2_MAX {
        Some(2)
    } else if (SURROGATE_MIN..=SURROGATE_MAX).contains(&i) {
        None
    } else if i <= RUNE3_MAX {
        Some(3)
    } else if i <= MAX_RUNE as u32 {
        Some(4)
    } else {
        None
    }
}

fn encoded(r: Rune) -> ([u8; UTF_MAX], usize) {
    let i = r as u32;
    if i <= RUNE1_MAX {
        return ([r as u8, 0, 0, 0], 1);
    }
    if i <= RUNE2_MAX {
        return ([T2 | (r >> 6) as u8, TX | (r as u8 & MASKX), 0, 0], 2);
    }
    if i < SURROGATE_MIN || (SURROGATE_MAX < i && i <= RUNE3_MAX) {
        return (
            [
                T3 | (r >> 12) as u8,
                TX | ((r >> 6) as u8 & MASKX),
                TX | (r as u8 & MASKX),
                0,
            ],
            3,
        );
    }
    if i > RUNE3_MAX && i <= MAX_RUNE as u32 {
        return (
            [
                T4 | (r >> 18) as u8,
                TX | ((r >> 12) as u8 & MASKX),
                TX | ((r >> 6) as u8 & MASKX),
                TX | (r as u8 & MASKX),
            ],
            4,
        );
    }
    (
        [
            RUNE_ERROR_BYTES[0],
            RUNE_ERROR_BYTES[1],
            RUNE_ERROR_BYTES[2],
            0,
        ],
        3,
    )
}

pub fn encode_rune(p: &mut [u8], r: Rune) -> usize {
    let (b, n) = encoded(r);
    p[..n].copy_from_slice(&b[..n]);
    n
}

pub fn append_rune(p: &mut Vec<u8>, r: Rune) {
    let (b, n) = encoded(r);
    p.extend_from_slice(&b[..n]);
}

pub fn rune_count(p: &[u8]) -> usize {
    let mut n = 0;
    let mut i = 0;
    while i < p.len() {
        if p[i] < RUNE_SELF {
            i += 1;
        } else {
            i += decode_rune(&p[i..]).1;
        }
        n += 1;
    }
    n
}

pub fn rune_start(b: u8) -> bool {
    b & 0xC0 != 0x80
}

pub fn valid(p: &[u8]) -> bool {
    let n = p.len();
    let mut i = 0;
    while i < n {
        let pi = p[i];
        if pi < RUNE_SELF {
            i += 1;
            continue;
        }
        let x = FIRST[pi as usize];
        if x == XX {
            return false;
        }
        let size = usize::from(x & 7);
        if i + size > n {
            return false;
        }
        let accept = ACCEPT_RANGES[usize::from(x >> 4)];
        if outside(p[i + 1], accept.lo, accept.hi) {
            return false;
        }
        if size > 2 && outside(p[i + 2], LOCB, HICB) {
            return false;
        }
        if size > 3 && outside(p[i + 3], LOCB, HICB) {
            return false;
        }
        i += size;
    }
    true
}

pub fn valid_rune(r: Rune) -> bool {
    (0..SURROGATE_MIN as Rune).contains(&r) || (SURROGATE_MAX as Rune + 1..=MAX_RUNE).contains(&r)
}
