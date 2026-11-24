use super::{madd, msub, nmsub};

pub fn log1p_impl<const F: bool>(x: f64) -> f64 {
    const SQRT2_M1: f64 = 4.142135623730950488017e-01;
    const SQRT2_HALF_M1: f64 = -2.928932188134524755992e-01;
    const SMALL: f64 = 1.0 / (1u64 << 29) as f64;
    const TINY: f64 = 1.0 / (1u64 << 54) as f64;
    const TWO53: f64 = (1u64 << 53) as f64;
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const LP1: f64 = 6.666666666666735130e-01;
    const LP2: f64 = 3.999999999940941908e-01;
    const LP3: f64 = 2.857142874366239149e-01;
    const LP4: f64 = 2.222219843214978396e-01;
    const LP5: f64 = 1.818357216161805012e-01;
    const LP6: f64 = 1.531383769920937332e-01;
    const LP7: f64 = 1.479819860511658591e-01;
    const TWO_THIRDS: f64 = f64::from_bits(0x3fe5_5555_5555_5555);

    if x < -1.0 || x.is_nan() {
        return f64::NAN;
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    if x == f64::INFINITY {
        return f64::INFINITY;
    }

    let absx = x.abs();

    let mut f = 0.0;
    let mut iu: u64 = 0;
    let mut k: i64 = 1;
    if absx < SQRT2_M1 {
        if absx < SMALL {
            if absx < TINY {
                return x;
            }
            return msub::<F>(x * x, 0.5, x);
        }
        if x > SQRT2_HALF_M1 {
            k = 0;
            f = x;
            iu = 1;
        }
    }
    let mut c = 0.0;
    if k != 0 {
        let mut u;
        if absx < TWO53 {
            u = 1.0 + x;
            iu = u.to_bits();
            k = ((iu >> 52) as i64) - 1023;
            if k > 0 {
                c = 1.0 - (u - x);
            } else {
                c = x - (u - 1.0);
            }
            c /= u;
        } else {
            u = x;
            iu = u.to_bits();
            k = ((iu >> 52) as i64) - 1023;
            c = 0.0;
        }
        iu &= 0x000f_ffff_ffff_ffff;
        if iu < 0x0006_a09e_667f_3bcd {
            u = f64::from_bits(iu | 0x3ff0_0000_0000_0000);
        } else {
            k += 1;
            u = f64::from_bits(iu | 0x3fe0_0000_0000_0000);
            iu = (0x0010_0000_0000_0000 - iu) >> 2;
        }
        f = u - 1.0;
    }
    let half_f = 0.5 * f;
    let hfsq = half_f * f;
    let kf = k as f64;
    if iu == 0 {
        if f == 0.0 {
            if k == 0 {
                return 0.0;
            }
            c = madd::<F>(kf, LN2_LO, c);
            return madd::<F>(kf, LN2_HI, c);
        }
        let inner = msub::<F>(f, TWO_THIRDS, 1.0);
        if k == 0 {
            return msub::<F>(hfsq, inner, f);
        }
        let a = madd::<F>(kf, LN2_LO, c);
        let b = nmsub::<F>(hfsq, inner, a);
        return nmsub::<F>(kf, LN2_HI, b - f);
    }
    let s = f / (2.0 + f);
    let z = s * s;
    let mut p = madd::<F>(z, LP7, LP6);
    p = madd::<F>(z, p, LP5);
    p = madd::<F>(z, p, LP4);
    p = madd::<F>(z, p, LP3);
    p = madd::<F>(z, p, LP2);
    p = madd::<F>(z, p, LP1);
    let r = z * p;
    let hfsq_r = madd::<F>(f, half_f, r);
    if k == 0 {
        return f - msub::<F>(hfsq_r, s, hfsq);
    }
    let a = madd::<F>(kf, LN2_LO, c);
    let b = madd::<F>(hfsq_r, s, a);
    let d = nmsub::<F>(f, half_f, b);
    nmsub::<F>(kf, LN2_HI, d - f)
}

pub fn log1p_go(x: f64, fused: bool) -> f64 {
    if fused {
        log1p_impl::<true>(x)
    } else {
        log1p_impl::<false>(x)
    }
}
