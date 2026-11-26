const ENCODE_STD: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

const VLQ_BASE_SHIFT: u32 = 5;
const VLQ_BASE: i32 = 1 << VLQ_BASE_SHIFT;
const VLQ_BASE_MASK: i32 = VLQ_BASE - 1;
const VLQ_SIGN_BIT: i32 = 1;
const VLQ_CONTINUATION_BIT: i32 = VLQ_BASE;

static DECODE_MAP: [u8; 256] = {
    let mut m = [0u8; 256];
    let mut i = 0;
    while i < ENCODE_STD.len() {
        m[ENCODE_STD[i] as usize] = i as u8;
        i += 1;
    }
    m
};

fn shl(v: i32, shift: u32) -> i32 {
    if shift >= 32 {
        0
    } else {
        v.wrapping_shl(shift)
    }
}

pub fn to_vlq_signed(n: i32) -> i32 {
    if n < 0 {
        shl(n.wrapping_neg(), 1).wrapping_add(1)
    } else {
        shl(n, 1)
    }
}

pub fn from_vlq_signed(n: i32) -> i32 {
    let is_neg = n & VLQ_SIGN_BIT != 0;
    let n = n >> 1;
    if is_neg { n.wrapping_neg() } else { n }
}

pub struct Reader<'a> {
    s: &'a [u8],
    i: usize,
}

impl<'a> Reader<'a> {
    pub fn new(s: &'a [u8]) -> Reader<'a> {
        Reader { s, i: 0 }
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        if self.i >= self.s.len() {
            return None;
        }
        let b = self.s[self.i];
        self.i += 1;
        Some(b)
    }

    pub fn unread_byte(&mut self) -> bool {
        if self.i == 0 {
            return false;
        }
        self.i -= 1;
        true
    }
}

pub fn encode(out: &mut Vec<u8>, n: i32) {
    let mut n = to_vlq_signed(n);
    let mut digit = VLQ_CONTINUATION_BIT;
    while digit & VLQ_CONTINUATION_BIT != 0 {
        digit = n & VLQ_BASE_MASK;
        n >>= VLQ_BASE_SHIFT;
        if n > 0 {
            digit |= VLQ_CONTINUATION_BIT;
        }
        out.push(ENCODE_STD[digit as usize]);
    }
}

pub fn decode(r: &mut Reader<'_>) -> Option<i32> {
    let mut n: i32 = 0;
    let mut shift: u32 = 0;
    let mut continuation = true;
    while continuation {
        let c = r.read_byte()?;
        let c = i32::from(DECODE_MAP[usize::from(c)]);
        continuation = c & VLQ_CONTINUATION_BIT != 0;
        n = n.wrapping_add(shl(c & VLQ_BASE_MASK, shift));
        shift = shift.wrapping_add(VLQ_BASE_SHIFT);
    }
    Some(from_vlq_signed(n))
}
