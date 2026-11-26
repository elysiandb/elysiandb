use super::nat::{self, Word};
use super::natconv::{self, ByteReader};
use std::cmp::Ordering;
use std::fmt;

#[derive(Clone, Default, PartialEq, Eq, Hash)]
pub struct Int {
    neg: bool,
    abs: Vec<Word>,
}

fn ord(o: Ordering) -> i32 {
    match o {
        Ordering::Less => -1,
        Ordering::Equal => 0,
        Ordering::Greater => 1,
    }
}

impl Int {
    pub(super) fn from_parts(neg: bool, abs: Vec<Word>) -> Int {
        let abs = nat::norm(abs);
        Int {
            neg: neg && !abs.is_empty(),
            abs,
        }
    }

    pub(super) fn abs_words(&self) -> &[Word] {
        &self.abs
    }

    pub fn new() -> Int {
        Int::default()
    }

    pub fn new_int(x: i64) -> Int {
        Int::from_parts(x < 0, nat::from_u64(x.unsigned_abs()))
    }

    pub fn from_u64(x: u64) -> Int {
        Int::from_parts(false, nat::from_u64(x))
    }

    pub fn sign(&self) -> i32 {
        if self.abs.is_empty() {
            0
        } else if self.neg {
            -1
        } else {
            1
        }
    }

    pub fn is_neg(&self) -> bool {
        self.neg
    }

    pub fn int64(&self) -> i64 {
        let v = nat::low64(&self.abs) as i64;
        if self.neg { v.wrapping_neg() } else { v }
    }

    pub fn uint64(&self) -> u64 {
        nat::low64(&self.abs)
    }

    pub fn is_int64(&self) -> bool {
        if self.abs.len() <= 1 {
            let w = nat::low64(&self.abs) as i64;
            return w >= 0 || self.neg && w == w.wrapping_neg();
        }
        false
    }

    pub fn is_uint64(&self) -> bool {
        !self.neg && self.abs.len() <= 1
    }

    pub fn bit_len(&self) -> u64 {
        nat::bit_len(&self.abs)
    }

    pub fn trailing_zero_bits(&self) -> u64 {
        nat::trailing_zero_bits(&self.abs)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn cmp(&self, y: &Int) -> i32 {
        if self.neg == y.neg {
            let r = ord(nat::cmp(&self.abs, &y.abs));
            if self.neg { -r } else { r }
        } else if self.neg {
            -1
        } else {
            1
        }
    }

    pub fn cmp_abs(&self, y: &Int) -> i32 {
        ord(nat::cmp(&self.abs, &y.abs))
    }

    pub fn neg(&self) -> Int {
        Int::from_parts(!self.neg, self.abs.clone())
    }

    pub fn abs(&self) -> Int {
        Int::from_parts(false, self.abs.clone())
    }

    pub fn add(&self, y: &Int) -> Int {
        if self.neg == y.neg {
            return Int::from_parts(self.neg, nat::add(&self.abs, &y.abs));
        }
        if nat::cmp(&self.abs, &y.abs) != Ordering::Less {
            Int::from_parts(self.neg, nat::sub(&self.abs, &y.abs))
        } else {
            Int::from_parts(!self.neg, nat::sub(&y.abs, &self.abs))
        }
    }

    pub fn sub(&self, y: &Int) -> Int {
        self.add(&y.neg())
    }

    pub fn mul(&self, y: &Int) -> Int {
        Int::from_parts(self.neg != y.neg, nat::mul(&self.abs, &y.abs))
    }

    pub fn quo_rem(&self, y: &Int) -> (Int, Int) {
        let (q, r) = nat::div(&self.abs, &y.abs);
        (
            Int::from_parts(self.neg != y.neg, q),
            Int::from_parts(self.neg, r),
        )
    }

    pub fn quo(&self, y: &Int) -> Int {
        self.quo_rem(y).0
    }

    pub fn rem(&self, y: &Int) -> Int {
        self.quo_rem(y).1
    }

    pub fn div_mod(&self, y: &Int) -> (Int, Int) {
        let (mut q, mut m) = self.quo_rem(y);
        if m.neg {
            if y.neg {
                q = q.add(&Int::new_int(1));
                m = m.sub(y);
            } else {
                q = q.sub(&Int::new_int(1));
                m = m.add(y);
            }
        }
        (q, m)
    }

    pub fn div(&self, y: &Int) -> Int {
        self.div_mod(y).0
    }

    pub fn modulo(&self, y: &Int) -> Int {
        self.div_mod(y).1
    }

    pub fn lsh(&self, n: u64) -> Int {
        Int::from_parts(self.neg, nat::shl(&self.abs, n))
    }

    pub fn rsh(&self, n: u64) -> Int {
        if self.neg {
            let t = nat::sub(&self.abs, &[1]);
            let t = nat::shr(&t, n);
            return Int::from_parts(true, nat::add(&t, &[1]));
        }
        Int::from_parts(false, nat::shr(&self.abs, n))
    }

    fn bitwise(&self, y: &Int, f: impl Fn(Word, Word) -> Word) -> Int {
        let len = self.abs.len().max(y.abs.len()) + 1;
        let a = nat::twos(&self.abs, self.neg, len);
        let b = nat::twos(&y.abs, y.neg, len);
        let z: Vec<Word> = a.iter().zip(b.iter()).map(|(&p, &q)| f(p, q)).collect();
        let (abs, neg) = nat::from_twos(z);
        Int::from_parts(neg, abs)
    }

    pub fn and(&self, y: &Int) -> Int {
        self.bitwise(y, |p, q| p & q)
    }

    pub fn or(&self, y: &Int) -> Int {
        self.bitwise(y, |p, q| p | q)
    }

    pub fn xor(&self, y: &Int) -> Int {
        self.bitwise(y, |p, q| p ^ q)
    }

    pub fn and_not(&self, y: &Int) -> Int {
        self.bitwise(y, |p, q| p & !q)
    }

    pub fn not(&self) -> Int {
        self.neg().sub(&Int::new_int(1))
    }

    pub fn mod_inverse(&self, n: &Int) -> Option<Int> {
        let n = n.abs();
        let g = if self.neg {
            self.modulo(&n)
        } else {
            self.clone()
        };
        let (d, x) = ext_gcd(&g, &n);
        if d.cmp(&Int::new_int(1)) != 0 {
            return None;
        }
        Some(x.modulo(&n))
    }

    pub fn exp(&self, y: &Int, m: Option<&Int>) -> Option<Int> {
        let mut x_words = self.abs.clone();
        if y.neg {
            match m {
                Some(m) if !m.abs.is_empty() => x_words = self.mod_inverse(m)?.abs,
                _ => return Some(Int::new_int(1)),
            }
        }
        let m_words: Vec<Word> = m.map(|m| m.abs.clone()).unwrap_or_default();
        let z = nat::exp(&x_words, &y.abs, &m_words);
        let neg = !z.is_empty() && self.neg && !y.abs.is_empty() && y.abs[0] & 1 == 1;
        if neg && !m_words.is_empty() {
            return Some(Int::from_parts(false, nat::sub(&m_words, &z)));
        }
        Some(Int::from_parts(neg, z))
    }

    pub fn bytes(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.abs.len() * 8);
        for w in self.abs.iter().rev() {
            out.extend_from_slice(&w.to_be_bytes());
        }
        let first = out.iter().position(|&b| b != 0).unwrap_or(out.len());
        out.drain(..first);
        out
    }

    pub fn set_bytes(buf: &[u8]) -> Int {
        let mut abs = Vec::with_capacity(buf.len().div_ceil(8));
        let mut i = buf.len();
        while i > 0 {
            let start = i.saturating_sub(8);
            let mut w = [0u8; 8];
            w[8 - (i - start)..].copy_from_slice(&buf[start..i]);
            abs.push(u64::from_be_bytes(w));
            i = start;
        }
        Int::from_parts(false, abs)
    }

    pub fn set_string(s: &[u8], base: u32) -> Option<Int> {
        let mut r = ByteReader::new(s);
        let first = r.read_byte()?;
        let neg = match first {
            b'-' => true,
            b'+' => false,
            _ => {
                r.unread_byte();
                false
            }
        };
        let abs = natconv::scan(&mut r, base).ok()?;
        if r.read_byte().is_some() {
            return None;
        }
        Some(Int::from_parts(neg, abs))
    }

    pub fn text(&self, base: u32) -> String {
        String::from_utf8(natconv::itoa(&self.abs, self.neg, base)).unwrap_or_default()
    }

    pub fn string(&self) -> String {
        self.text(10)
    }
}

fn ext_gcd(a: &Int, b: &Int) -> (Int, Int) {
    let (mut old_r, mut r) = (a.clone(), b.clone());
    let (mut old_s, mut s) = (Int::new_int(1), Int::new());
    while r.sign() != 0 {
        let q = old_r.quo(&r);
        let nr = old_r.sub(&q.mul(&r));
        old_r = std::mem::replace(&mut r, nr);
        let ns = old_s.sub(&q.mul(&s));
        old_s = std::mem::replace(&mut s, ns);
    }
    (old_r, old_s)
}

impl fmt::Display for Int {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}

impl fmt::Debug for Int {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.string())
    }
}
