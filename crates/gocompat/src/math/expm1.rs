use super::{madd, msub, nmsub};

pub fn expm1_impl<const F: bool>(x: f64) -> f64 {
    const OTHRESHOLD: f64 = 7.09782712893383973096e+02;
    const LN2_X56: f64 = 3.88162421113569373274e+01;
    const LN2_HALF_X3: f64 = 1.03972077083991796413e+00;
    const LN2_HALF: f64 = 3.46573590279972654709e-01;
    const LN2_HI: f64 = 6.93147180369123816490e-01;
    const LN2_LO: f64 = 1.90821492927058770002e-10;
    const INV_LN2: f64 = 1.44269504088896338700e+00;
    const TINY: f64 = 1.0 / (1u64 << 54) as f64;
    const Q1: f64 = -3.33333333333331316428e-02;
    const Q2: f64 = 1.58730158725481460165e-03;
    const Q3: f64 = -7.93650757867487942473e-05;
    const Q4: f64 = 4.00821782732936239552e-06;
    const Q5: f64 = -2.01099218183624371326e-07;

    if x == f64::INFINITY || x.is_nan() {
        return x;
    }
    if x == f64::NEG_INFINITY {
        return -1.0;
    }

    let mut x = x;
    let mut absx = x;
    let mut sign = false;
    if x < 0.0 {
        absx = -absx;
        sign = true;
    }

    if absx >= LN2_X56 {
        if sign {
            return -1.0;
        }
        if absx >= OTHRESHOLD {
            return f64::INFINITY;
        }
    }

    let mut c = 0.0;
    let mut k: i64 = 0;
    if absx > LN2_HALF {
        let hi;
        let lo;
        if absx < LN2_HALF_X3 {
            if !sign {
                hi = x - LN2_HI;
                lo = LN2_LO;
                k = 1;
            } else {
                hi = x + LN2_HI;
                lo = -LN2_LO;
                k = -1;
            }
        } else {
            if !sign {
                k = madd::<F>(INV_LN2, x, 0.5) as i64;
            } else {
                k = nmsub::<F>(INV_LN2, x, 0.5) as i64;
            }
            let t = k as f64;
            hi = msub::<F>(t, LN2_HI, x);
            lo = t * LN2_LO;
        }
        x = hi - lo;
        c = (hi - x) - lo;
    } else if absx < TINY {
        return x;
    }

    let hfx = 0.5 * x;
    let hxs = x * hfx;
    let mut r1 = madd::<F>(hxs, Q5, Q4);
    r1 = madd::<F>(hxs, r1, Q3);
    r1 = madd::<F>(hxs, r1, Q2);
    r1 = madd::<F>(hxs, r1, Q1);
    r1 = madd::<F>(hxs, r1, 1.0);
    let t = msub::<F>(r1, hfx, 3.0);
    let quot = (r1 - t) / msub::<F>(x, t, 6.0);
    if k == 0 {
        let e = hxs * quot;
        return x - msub::<F>(x, hfx, x * e);
    }
    let mut e = nmsub::<F>(hxs, quot, c);
    e = nmsub::<F>(x, e, c);
    e = msub::<F>(x, hfx, e);
    if k == -1 {
        return nmsub::<F>(x - e, 0.5, 0.5);
    }
    if k == 1 {
        if x < -0.25 {
            return -2.0 * (e - (x + 0.5));
        }
        return 1.0 + 2.0 * (x - e);
    }
    if k <= -2 || k > 56 {
        let mut y = 1.0 - (e - x);
        y = f64::from_bits(y.to_bits().wrapping_add((k as u64) << 52));
        return y - 1.0;
    }
    if k < 20 {
        let t = f64::from_bits(0x3ff0_0000_0000_0000 - (0x0020_0000_0000_0000u64 >> k));
        let mut y = t - (e - x);
        y = f64::from_bits(y.to_bits().wrapping_add((k as u64) << 52));
        return y;
    }
    let t = f64::from_bits(((0x3ff - k) as u64) << 52);
    let mut y = x - (e + t);
    y += 1.0;
    f64::from_bits(y.to_bits().wrapping_add((k as u64) << 52))
}

pub fn expm1_go(x: f64, fused: bool) -> f64 {
    if fused {
        expm1_impl::<true>(x)
    } else {
        expm1_impl::<false>(x)
    }
}
