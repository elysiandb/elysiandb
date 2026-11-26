use super::common::_dcheck;

pub(super) const DIY_FP_K_SIGNIFICAND_SIZE: i64 = 64;
const K_SIGNIFICAND_SIZE: i64 = 53;
const K_UINT64_MSB: u64 = 1 << 63;

const K_SIGNIFICAND_MASK: u64 = 0x000F_FFFF_FFFF_FFFF;
const K_HIDDEN_BIT: u64 = 0x0010_0000_0000_0000;
const K_EXPONENT_MASK: u64 = 0x7FF0_0000_0000_0000;

const K_PHYSICAL_SIGNIFICAND_SIZE: i64 = 52;
const K_EXPONENT_BIAS: i64 = 0x3FF + K_PHYSICAL_SIGNIFICAND_SIZE;
const K_DENORMAL_EXPONENT: i64 = -K_EXPONENT_BIAS + 1;

#[derive(Clone, Copy)]
pub(super) struct Double(pub(super) f64);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct Diyfp {
    pub(super) f: u64,
    pub(super) e: i64,
}

impl Diyfp {
    fn subtract(&mut self, o: Diyfp) {
        _dcheck(self.e == o.e);
        _dcheck(self.f >= o.f);
        self.f = self.f.wrapping_sub(o.f);
    }

    pub(super) fn minus(self, o: Diyfp) -> Diyfp {
        let mut res = self;
        res.subtract(o);
        res
    }

    fn mul(&mut self, o: Diyfp) {
        const K_M32: u64 = 0xFFFF_FFFF;
        let a = self.f >> 32;
        let b = self.f & K_M32;
        let c = o.f >> 32;
        let d = o.f & K_M32;
        let ac = a.wrapping_mul(c);
        let bc = b.wrapping_mul(c);
        let ad = a.wrapping_mul(d);
        let bd = b.wrapping_mul(d);
        let mut tmp = (bd >> 32).wrapping_add(ad & K_M32).wrapping_add(bc & K_M32);
        tmp = tmp.wrapping_add(1 << 31);
        let result_f = ac
            .wrapping_add(ad >> 32)
            .wrapping_add(bc >> 32)
            .wrapping_add(tmp >> 32);
        self.e += o.e + 64;
        self.f = result_f;
    }

    pub(super) fn times(self, o: Diyfp) -> Diyfp {
        let mut res = self;
        res.mul(o);
        res
    }

    fn _normalize(&mut self) {
        let (mut f, mut e) = (self.f, self.e);
        const K10_MS_BITS: u64 = 0x3FF << 54;
        while f & K10_MS_BITS == 0 {
            f <<= 10;
            e -= 10;
        }
        while f & K_UINT64_MSB == 0 {
            f <<= 1;
            e -= 1;
        }
        self.f = f;
        self.e = e;
    }
}

pub(super) fn normalize_diyfp(f: Diyfp) -> Diyfp {
    let mut res = f;
    res._normalize();
    res
}

impl Double {
    pub(super) fn to_normalized_diyfp(self) -> Diyfp {
        let (mut f, mut e) = self.sig_exp();
        while f & K_HIDDEN_BIT == 0 {
            f <<= 1;
            e -= 1;
        }
        f <<= DIY_FP_K_SIGNIFICAND_SIZE - K_SIGNIFICAND_SIZE;
        e -= DIY_FP_K_SIGNIFICAND_SIZE - K_SIGNIFICAND_SIZE;
        Diyfp { f, e }
    }

    pub(super) fn normalized_boundaries(self) -> (Diyfp, Diyfp) {
        let v = self.to_diy_fp();
        let significand_is_zero = v.f == K_HIDDEN_BIT;
        let m_plus = normalize_diyfp(Diyfp {
            f: (v.f << 1).wrapping_add(1),
            e: v.e - 1,
        });
        let mut m_minus = if significand_is_zero && v.e != K_DENORMAL_EXPONENT {
            Diyfp {
                f: (v.f << 2).wrapping_sub(1),
                e: v.e - 2,
            }
        } else {
            Diyfp {
                f: (v.f << 1).wrapping_sub(1),
                e: v.e - 1,
            }
        };
        m_minus.f = shl64(m_minus.f, m_minus.e - m_plus.e);
        m_minus.e = m_plus.e;
        (m_minus, m_plus)
    }

    fn to_diy_fp(self) -> Diyfp {
        let (f, e) = self.sig_exp();
        Diyfp { f, e }
    }

    fn sig_exp(self) -> (u64, i64) {
        let d64 = self.0.to_bits();
        let mut significand = d64 & K_SIGNIFICAND_MASK;
        let exponent = if d64 & K_EXPONENT_MASK != 0 {
            significand += K_HIDDEN_BIT;
            ((d64 & K_EXPONENT_MASK) >> K_PHYSICAL_SIGNIFICAND_SIZE) as i64 - K_EXPONENT_BIAS
        } else {
            K_DENORMAL_EXPONENT
        };
        (significand, exponent)
    }
}

pub(super) fn shl64(x: u64, n: i64) -> u64 {
    if n < 0 {
        panic!("negative shift amount");
    }
    if n >= 64 { 0 } else { x << n }
}

pub(super) fn shr64(x: u64, n: i64) -> u64 {
    if n < 0 {
        panic!("negative shift amount");
    }
    if n >= 64 { 0 } else { x >> n }
}
