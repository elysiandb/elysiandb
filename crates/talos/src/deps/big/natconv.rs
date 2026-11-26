use super::nat::{self, Word};

pub const MAX_BASE: u32 = 10 + (b'z' - b'a' + 1) as u32 + (b'Z' - b'A' + 1) as u32;
const MAX_BASE_SMALL: u32 = 10 + (b'z' - b'a' + 1) as u32;
const DIGITS: &[u8; 62] = b"0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ";

pub struct ByteReader<'a> {
    s: &'a [u8],
    pub i: usize,
}

impl<'a> ByteReader<'a> {
    pub fn new(s: &'a [u8]) -> ByteReader<'a> {
        ByteReader { s, i: 0 }
    }

    pub fn read_byte(&mut self) -> Option<u8> {
        let b = *self.s.get(self.i)?;
        self.i += 1;
        Some(b)
    }

    pub fn unread_byte(&mut self) {
        if self.i > 0 {
            self.i -= 1;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScanError {
    NoDigits,
    InvalSep,
}

fn max_pow(b: Word) -> (Word, u32) {
    let mut p = b;
    let mut n = 1;
    let max = u64::MAX / b;
    while p <= max {
        p *= b;
        n += 1;
    }
    (p, n)
}

fn pow(x: Word, n: u32) -> Word {
    let mut p: Word = 1;
    for _ in 0..n {
        p = p.wrapping_mul(x);
    }
    p
}

pub fn scan(r: &mut ByteReader<'_>, base: u32) -> Result<Vec<Word>, ScanError> {
    let base_ok = base == 0 || (2..=MAX_BASE).contains(&base);
    if !base_ok {
        panic!("invalid number base {base}");
    }
    let mut prev = b'.';
    let mut inval_sep = false;
    let mut ch = r.read_byte();
    let (mut b, mut prefix) = (base, 0u8);
    let mut count = 0;
    if base == 0 {
        b = 10;
        if ch == Some(b'0') {
            prev = b'0';
            count = 1;
            ch = r.read_byte();
            if let Some(c) = ch {
                match c {
                    b'b' | b'B' => {
                        b = 2;
                        prefix = b'b';
                    }
                    b'o' | b'O' => {
                        b = 8;
                        prefix = b'o';
                    }
                    b'x' | b'X' => {
                        b = 16;
                        prefix = b'x';
                    }
                    _ => {
                        b = 8;
                        prefix = b'0';
                    }
                }
                if prefix != 0 {
                    count = 0;
                    if prefix != b'0' {
                        ch = r.read_byte();
                    }
                }
            }
        }
    }
    let mut z: Vec<Word> = Vec::new();
    let b1 = Word::from(b);
    let (bn, n) = max_pow(b1);
    let mut di: Word = 0;
    let mut i = 0;
    while let Some(c) = ch {
        if c == b'_' && base == 0 {
            if prev != b'0' {
                inval_sep = true;
            }
            prev = b'_';
        } else {
            let d1 = match c {
                b'0'..=b'9' => Word::from(c - b'0'),
                b'a'..=b'z' => Word::from(c - b'a' + 10),
                b'A'..=b'Z' => {
                    if b <= MAX_BASE_SMALL {
                        Word::from(c - b'A' + 10)
                    } else {
                        Word::from(c - b'A') + Word::from(MAX_BASE_SMALL)
                    }
                }
                _ => Word::from(MAX_BASE + 1),
            };
            if d1 >= b1 {
                r.unread_byte();
                break;
            }
            prev = b'0';
            count += 1;
            di = di * b1 + d1;
            i += 1;
            if i == n {
                z = nat::mul_add_ww(&z, bn, di);
                di = 0;
                i = 0;
            }
        }
        ch = r.read_byte();
    }
    let mut err = None;
    if inval_sep || prev == b'_' {
        err = Some(ScanError::InvalSep);
    }
    if count == 0 {
        if prefix == b'0' {
            return match err {
                Some(e) => Err(e),
                None => Ok(Vec::new()),
            };
        }
        err = Some(ScanError::NoDigits);
    }
    if i > 0 {
        z = nat::mul_add_ww(&z, pow(b1, i), di);
    }
    match err {
        Some(e) => Err(e),
        None => Ok(nat::norm(z)),
    }
}

pub fn itoa(x: &[Word], neg: bool, base: u32) -> Vec<u8> {
    if !(2..=MAX_BASE).contains(&base) {
        panic!("invalid base");
    }
    if x.is_empty() {
        return b"0".to_vec();
    }
    let b = Word::from(base);
    let (bb, ndigits) = max_pow(b);
    let mut out: Vec<u8> = Vec::new();
    let mut q = x.to_vec();
    while !q.is_empty() {
        let (nq, mut r) = nat::div_w(&q, bb);
        q = nq;
        if q.is_empty() {
            while r > 0 {
                out.push(DIGITS[(r % b) as usize]);
                r /= b;
            }
        } else {
            for _ in 0..ndigits {
                out.push(DIGITS[(r % b) as usize]);
                r /= b;
            }
        }
    }
    if neg {
        out.push(b'-');
    }
    out.reverse();
    out
}
