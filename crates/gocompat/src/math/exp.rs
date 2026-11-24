use super::bits::ldexp;

const LN2_HI: f64 = 6.93147180369123816490e-01;
const LN2_LO: f64 = 1.90821492927058770002e-10;
const LOG2E: f64 = 1.44269504088896338700e+00;
const OVERFLOW: f64 = 7.09782712893383973096e+02;
const UNDERFLOW: f64 = -7.45133219101941108420e+02;
const NEAR_ZERO: f64 = 1.0 / (1u64 << 28) as f64;

const P1: f64 = 1.66666666666666657415e-01;
const P2: f64 = -2.77777777770155933842e-03;
const P3: f64 = 6.61375632143793436117e-05;
const P4: f64 = -1.65339022054652515390e-06;
const P5: f64 = 4.13813679705723846039e-08;

#[cfg(target_arch = "x86_64")]
pub fn exp(x: f64) -> f64 {
    exp_amd64(x, super::use_fma())
}

#[cfg(target_arch = "aarch64")]
pub fn exp(x: f64) -> f64 {
    exp_arm64(x)
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn exp(x: f64) -> f64 {
    exp_go(x)
}

pub fn exp_go(x: f64) -> f64 {
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == f64::NEG_INFINITY {
        return 0.0;
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    if x < UNDERFLOW {
        return 0.0;
    }
    if -NEAR_ZERO < x && x < NEAR_ZERO {
        return 1.0 + x;
    }
    let k: i64 = if x < 0.0 {
        (LOG2E * x - 0.5) as i64
    } else if x > 0.0 {
        (LOG2E * x + 0.5) as i64
    } else {
        0
    };
    let hi = x - k as f64 * LN2_HI;
    let lo = k as f64 * LN2_LO;
    expmulti(hi, lo, k)
}

fn expmulti(hi: f64, lo: f64, k: i64) -> f64 {
    let r = hi - lo;
    let t = r * r;
    let c = r - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let y = 1.0 - ((lo - (r * c) / (2.0 - c)) - hi);
    ldexp(y, k)
}

pub fn exp_arm64(x: f64) -> f64 {
    const NEAR_ZERO_BITS: u64 = 0x3e30_0000_0000_0000;
    const FRAC_MASK: u64 = 0x000f_ffff_ffff_ffff;
    const C1: u64 = 0x3cb0_0000_0000_0000;
    if x.is_nan() {
        return x;
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    if x < UNDERFLOW {
        return 0.0;
    }
    let mut m = 1.0;
    if x.abs() < f64::from_bits(NEAR_ZERO_BITS) {
        return m + x;
    }
    let k = if x < 0.0 {
        LOG2E.mul_add(x, -0.5)
    } else {
        LOG2E.mul_add(x, 0.5)
    };
    let ki = k as i64;
    let kf = ki as f64;
    let hi = (-LN2_HI).mul_add(kf, x);
    let lo = LN2_LO * kf;
    let r = hi - lo;
    let t = r * r;
    let mut c = P5.mul_add(t, P4);
    c = c.mul_add(t, P3);
    c = c.mul_add(t, P2);
    c = c.mul_add(t, P1);
    let c = (-c).mul_add(t, r);
    let d = 2.0 - c;
    let q = (c * r) / d;
    let y = 1.0 - ((lo - q) - hi);
    let yb = y.to_bits();
    let frac = yb & FRAC_MASK;
    let mut e = (yb >> 52) as i64;
    e = e.wrapping_add(ki);
    if e < 1 {
        e += 52;
        m = f64::from_bits(C1);
    }
    let r0 = frac | ((e as u64) << 52);
    m * f64::from_bits(r0)
}

pub fn exp_amd64(x: f64, use_fma: bool) -> f64 {
    const LN2U: f64 = 0.69314718055966295651160180568695068359375;
    const LN2L: f64 = 0.28235290563031577122588448175013436025525412068e-12;
    const OVERFLOW_AMD64: f64 = 7.09782712893384e+02;
    const D: [f64; 9] = [
        0.5,
        1.0,
        2.0,
        1.6666666666666666667e-1,
        4.1666666666666666667e-2,
        8.3333333333333333333e-3,
        1.3888888888888888889e-3,
        1.9841269841269841270e-4,
        2.4801587301587301587e-5,
    ];
    let bits = x.to_bits();
    if bits & !(1u64 << 63) >= 0x7FF0_0000_0000_0000 {
        if bits == 0xFFF0_0000_0000_0000 {
            return 0.0;
        }
        return x;
    }
    if x > OVERFLOW_AMD64 {
        return f64::INFINITY;
    }
    let x1 = LOG2E * x;
    let mut bx: i32 = cvtsd2sl(x1);
    let kf = f64::from(bx);
    let mut x0 = x;
    let mut x1;
    if use_fma {
        x0 = (-LN2U).mul_add(kf, x0);
        x0 = (-LN2L).mul_add(kf, x0);
        x0 *= 0.0625;
        x1 = D[8];
        x1 = x1.mul_add(x0, D[7]);
        x1 = x1.mul_add(x0, D[6]);
        x1 = x1.mul_add(x0, D[5]);
        x1 = x1.mul_add(x0, D[4]);
        x1 = x1.mul_add(x0, D[3]);
        x1 = x1.mul_add(x0, D[0]);
        x1 = x1.mul_add(x0, D[1]);
        x0 *= x1;
        for _ in 0..3 {
            x1 = x0 + D[2];
            x0 *= x1;
        }
        x1 = x0 + D[2];
        x0 = x0.mul_add(x1, D[1]);
    } else {
        x0 -= LN2U * kf;
        x0 -= LN2L * kf;
        x0 *= 0.0625;
        x1 = D[8];
        x1 *= x0;
        x1 += D[7];
        x1 *= x0;
        x1 += D[6];
        x1 *= x0;
        x1 += D[5];
        x1 *= x0;
        x1 += D[4];
        x1 *= x0;
        x1 += D[3];
        x1 *= x0;
        x1 += D[0];
        x1 *= x0;
        x1 += D[1];
        x0 *= x1;
        for _ in 0..4 {
            x1 = D[2];
            x1 += x0;
            x0 *= x1;
        }
        x0 += D[1];
    }
    bx = bx.wrapping_add(0x3FF);
    if bx <= 0 {
        if bx < -52 {
            return 0.0;
        }
        bx += 0x3FE;
        x0 *= f64::from_bits(u64::from(bx as u32) << 52);
        bx = 1;
    } else if bx >= 0x7FF {
        return f64::INFINITY;
    }
    x0 * f64::from_bits(u64::from(bx as u32) << 52)
}

fn cvtsd2sl(x: f64) -> i32 {
    let r = x.round_ties_even();
    if r.is_nan() || r < i32::MIN as f64 || r > i32::MAX as f64 {
        return i32::MIN;
    }
    r as i32
}
