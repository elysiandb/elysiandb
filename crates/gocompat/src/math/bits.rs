pub const SHIFT: u64 = 52;
pub const MASK: u64 = 0x7FF;
pub const BIAS: i64 = 1023;
pub const SMALLEST_NORMAL: f64 = 2.2250738585072014e-308;

pub fn normalize(x: f64) -> (f64, i64) {
    if x.abs() < SMALLEST_NORMAL {
        return (x * (1u64 << 52) as f64, -52);
    }
    (x, 0)
}

pub fn frexp(f: f64) -> (f64, i64) {
    if f == 0.0 || f.is_infinite() || f.is_nan() {
        return (f, 0);
    }
    let (f, mut exp) = normalize(f);
    let mut x = f.to_bits();
    exp += ((x >> SHIFT) & MASK) as i64 - BIAS + 1;
    x &= !(MASK << SHIFT);
    x |= ((-1 + BIAS) as u64) << SHIFT;
    (f64::from_bits(x), exp)
}

pub fn ldexp(frac: f64, exp: i64) -> f64 {
    if frac == 0.0 || frac.is_infinite() || frac.is_nan() {
        return frac;
    }
    let (frac, e) = normalize(frac);
    let mut exp = exp.wrapping_add(e);
    let mut x = frac.to_bits();
    exp = exp.wrapping_add(((x >> SHIFT) & MASK) as i64 - BIAS);
    if exp < -1075 {
        return 0.0f64.copysign(frac);
    }
    if exp > 1023 {
        if frac < 0.0 {
            return f64::NEG_INFINITY;
        }
        return f64::INFINITY;
    }
    let mut m = 1.0;
    if exp < -1022 {
        exp += 53;
        m = 1.0 / (1u64 << 53) as f64;
    }
    x &= !(MASK << SHIFT);
    x |= ((exp + BIAS) as u64) << SHIFT;
    m * f64::from_bits(x)
}

#[cfg(target_arch = "aarch64")]
pub fn modf(f: f64) -> (f64, f64) {
    modf_arm64(f)
}

#[cfg(not(target_arch = "aarch64"))]
pub fn modf(f: f64) -> (f64, f64) {
    modf_go(f)
}

pub fn modf_arm64(f: f64) -> (f64, f64) {
    let int = f.trunc();
    let frac = f - int;
    let frac = f64::from_bits(frac.to_bits() | (f.to_bits() & (1u64 << 63)));
    (int, frac)
}

pub fn modf_go(f: f64) -> (f64, f64) {
    if f < 1.0 {
        if f < 0.0 {
            let (int, frac) = modf_go(-f);
            return (-int, -frac);
        }
        if f == 0.0 {
            return (f, f);
        }
        return (0.0, f);
    }
    let mut x = f.to_bits();
    let e = ((x >> SHIFT) & MASK).wrapping_sub(BIAS as u64);
    if e < 64 - 12 {
        x &= !((1u64 << (64 - 12 - e)) - 1);
    }
    let int = f64::from_bits(x);
    (int, f - int)
}
