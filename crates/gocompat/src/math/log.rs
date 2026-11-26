use super::bits::frexp;

const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;
const L1: f64 = 6.666666666666735130e-01;
const L2: f64 = 3.999999999940941908e-01;
const L3: f64 = 2.857142874366239149e-01;
const L4: f64 = 2.222219843214978396e-01;
const L5: f64 = 1.818357216161805012e-01;
const L6: f64 = 1.531383769920937332e-01;
const L7: f64 = 1.479819860511658591e-01;
const SQRT2_2: f64 = std::f64::consts::SQRT_2 / 2.0;

#[cfg(target_arch = "x86_64")]
pub fn log(x: f64) -> f64 {
    log_amd64(x)
}

#[cfg(target_arch = "aarch64")]
pub fn log(x: f64) -> f64 {
    log_go(x, true)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn log(x: f64) -> f64 {
    log_go(x, false)
}

pub fn log_go(x: f64, fused: bool) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x < 0.0 {
        return f64::NAN;
    }
    if x == 0.0 {
        return f64::NEG_INFINITY;
    }
    let (mut f1, mut ki) = frexp(x);
    if f1 < SQRT2_2 {
        f1 = if fused { f1 + f1 } else { f1 * 2.0 };
        ki -= 1;
    }
    let f = f1 - 1.0;
    let k = ki as f64;
    let s = f / (2.0 + f);
    let s2 = s * s;
    let s4 = s2 * s2;
    if fused {
        let p1 = L7.mul_add(s4, L5).mul_add(s4, L3).mul_add(s4, L1);
        let p2 = L6.mul_add(s4, L4).mul_add(s4, L2);
        let t2 = s4 * p2;
        let r = s2.mul_add(p1, t2);
        let hf = 0.5 * f;
        let hfsq_r = hf.mul_add(f, r);
        let a = s.mul_add(hfsq_r, k * LN2_LO);
        let b = hf.mul_add(f, -a);
        let c = b - f;
        return k.mul_add(LN2_HI, -c);
    }
    let t1 = s2 * (L1 + s4 * (L3 + s4 * (L5 + s4 * L7)));
    let t2 = s4 * (L2 + s4 * (L4 + s4 * L6));
    let r = t1 + t2;
    let hfsq = 0.5 * f * f;
    k * LN2_HI - ((hfsq - (s * (hfsq + r) + k * LN2_LO)) - f)
}

pub fn log_amd64(x: f64) -> f64 {
    const H_SQRT2: f64 = 7.07106781186547524401e-01;
    let bx = x.to_bits();
    if bx & !(1u64 << 63) == 0 {
        return f64::NEG_INFINITY;
    }
    if (bx as i64) < 0 {
        return f64::from_bits(0x7FF8_0000_0000_0001);
    }
    if bx >= 0x7FF0_0000_0000_0000 {
        return x;
    }
    let mut x2 = f64::from_bits((bx & 0x000F_FFFF_FFFF_FFFF) | 0.5f64.to_bits());
    let e = (((bx >> 52) & 0x7FF) as i32) - 0x3FE;
    let mut x1 = f64::from(e);
    let mask = x2 <= H_SQRT2;
    let mut x3 = if mask { 1.0 } else { 0.0 };
    x1 -= x3;
    x3 += 1.0;
    x2 *= x3;
    x2 -= 1.0;
    let x0 = 2.0 + x2;
    let mut x3 = x2 / x0;
    let mut x4 = x3 * x3;
    let mut x5 = x4 * x4;
    let mut x6 = L7 * x5;
    x6 += L5;
    x6 *= x5;
    x6 += L3;
    x6 *= x5;
    x6 += L1;
    x4 *= x6;
    let mut x6 = L6 * x5;
    x6 += L4;
    x6 *= x5;
    x6 += L2;
    x5 *= x6;
    x4 += x5;
    let mut x0 = 0.5 * x2;
    x0 *= x2;
    x4 += x0;
    x3 *= x4;
    let x4 = LN2_LO * x1;
    x3 += x4;
    x0 -= x3;
    x0 -= x2;
    x1 *= LN2_HI;
    x1 - x0
}
