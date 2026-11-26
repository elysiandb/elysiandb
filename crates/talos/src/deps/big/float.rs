use super::int::Int;
use super::nat;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Accuracy {
    Below = -1,
    Exact = 0,
    Above = 1,
}

fn decompose(f: f64) -> (bool, u64, i64) {
    let bits = f.to_bits();
    let neg = bits >> 63 == 1;
    let exp = ((bits >> 52) & 0x7ff) as i64;
    let frac = bits & ((1 << 52) - 1);
    if exp == 0 {
        (neg, frac, -1074)
    } else {
        (neg, frac | (1 << 52), exp - 1075)
    }
}

fn check_nan(f: f64) {
    if f.is_nan() {
        panic!("NewFloat(NaN)");
    }
}

pub fn float_is_int(f: f64) -> bool {
    check_nan(f);
    if f.is_infinite() {
        return false;
    }
    f == f.trunc()
}

pub fn float_int(f: f64) -> (Option<Int>, Accuracy) {
    check_nan(f);
    if f.is_infinite() {
        return (
            None,
            if f < 0.0 {
                Accuracy::Above
            } else {
                Accuracy::Below
            },
        );
    }
    if f == 0.0 {
        return (Some(Int::new()), Accuracy::Exact);
    }
    let (neg, mant, e) = decompose(f);
    let (abs, exact) = if e >= 0 {
        (nat::shl(&nat::from_u64(mant), e as u64), true)
    } else {
        let s = (-e) as u64;
        let truncated = if s >= 64 { 0 } else { mant >> s };
        let exact = s < 64 && truncated << s == mant;
        (nat::from_u64(truncated), exact)
    };
    let z = Int::from_parts(neg, abs);
    let acc = if exact {
        Accuracy::Exact
    } else if neg {
        Accuracy::Above
    } else {
        Accuracy::Below
    };
    (Some(z), acc)
}

pub fn cmp_float64(x: &Int, f: f64) -> i32 {
    check_nan(f);
    if f.is_infinite() {
        return if f > 0.0 { -1 } else { 1 };
    }
    let (t, acc) = float_int(f);
    let c = x.cmp(&t.unwrap_or_default());
    if acc == Accuracy::Exact {
        return c;
    }
    if f > 0.0 {
        if c <= 0 { -1 } else { 1 }
    } else if c >= 0 {
        1
    } else {
        -1
    }
}

impl Int {
    pub fn float64(&self) -> (f64, Accuracy) {
        let abs = self.abs_words();
        let n = nat::bit_len(abs);
        if n == 0 {
            return (0.0, Accuracy::Exact);
        }
        if n <= 53 || n < 64 && n - nat::trailing_zero_bits(abs) <= 53 {
            let mut f = nat::low64(abs) as f64;
            if self.is_neg() {
                f = -f;
            }
            return (f, Accuracy::Exact);
        }
        let shift = n - 53;
        let mut m = nat::low64(&nat::shr(abs, shift));
        let half = (abs[((shift - 1) / 64) as usize] >> ((shift - 1) % 64)) & 1 == 1;
        let sticky = nat::trailing_zero_bits(abs) < shift - 1;
        let inexact = half || sticky;
        let mut exp = n - 1;
        let mut rounded_up = false;
        if half && (sticky || m & 1 == 1) {
            m += 1;
            rounded_up = true;
            if m == 1 << 53 {
                m >>= 1;
                exp += 1;
            }
        }
        let neg = self.is_neg();
        if exp > 1023 {
            return if neg {
                (f64::NEG_INFINITY, Accuracy::Below)
            } else {
                (f64::INFINITY, Accuracy::Above)
            };
        }
        let bits = ((exp + 1023) << 52) | (m & ((1 << 52) - 1));
        let mut f = f64::from_bits(bits);
        if neg {
            f = -f;
        }
        let acc = if !inexact {
            Accuracy::Exact
        } else if rounded_up != neg {
            Accuracy::Above
        } else {
            Accuracy::Below
        };
        (f, acc)
    }
}
