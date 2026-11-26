use super::log1p::log1p_impl;
use super::{Arch, madd, nmsub};

const LN2: f64 = f64::from_bits(0x3fe6_2e42_fefa_39ef);

pub fn sinh_impl<const F: bool>(x: f64, exp: fn(f64) -> f64) -> f64 {
    const P0: f64 = -0.6307673640497716991184787251e+6;
    const P1: f64 = -0.8991272022039509355398013511e+5;
    const P2: f64 = -0.2894211355989563807284660366e+4;
    const P3: f64 = -0.2630563213397497062819489e+2;
    const Q0: f64 = -0.6307673640497716991212077277e+6;
    const Q1: f64 = 0.1521517378790019070696485176e+5;
    const Q2: f64 = -0.173678953558233699533450911e+3;
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let mut temp = if x > 21.0 {
        exp(x) * 0.5
    } else if x > 0.5 {
        let ex = exp(x);
        (ex - 1.0 / ex) * 0.5
    } else {
        let sq = x * x;
        let p = madd::<F>(madd::<F>(madd::<F>(P3, sq, P2), sq, P1), sq, P0);
        let num = p * x;
        let q = madd::<F>(madd::<F>(madd::<F>(x, x, Q2), sq, Q1), sq, Q0);
        num / q
    };
    if sign {
        temp = -temp;
    }
    temp
}

pub fn cosh_impl(x: f64, exp: fn(f64) -> f64) -> f64 {
    let x = x.abs();
    if x > 21.0 {
        return exp(x) * 0.5;
    }
    let ex = exp(x);
    (ex + 1.0 / ex) * 0.5
}

pub fn tanh_impl<const F: bool>(x: f64, exp: fn(f64) -> f64) -> f64 {
    const HALF_MAXLOG: f64 = f64::from_bits(0x4046_01e6_78fc_457b);
    const P: [f64; 3] = [
        -9.64399179425052238628e-1,
        -9.92877231001918586564e1,
        -1.61468768441708447952e3,
    ];
    const Q: [f64; 3] = [
        1.12811678491632931402e2,
        2.23548839060100448583e3,
        4.84406305325125486048e3,
    ];
    let z = x.abs();
    if z > HALF_MAXLOG {
        if x < 0.0 {
            return -1.0;
        }
        return 1.0;
    }
    if z >= 0.625 {
        let s = exp(2.0 * z);
        let mut z = 1.0 - 2.0 / (s + 1.0);
        if x < 0.0 {
            z = -z;
        }
        return z;
    }
    if x == 0.0 {
        return x;
    }
    let s = x * x;
    let p = madd::<F>(madd::<F>(P[0], s, P[1]), s, P[2]);
    let num = (x * s) * p;
    let q = madd::<F>(madd::<F>(madd::<F>(x, x, Q[0]), s, Q[1]), s, Q[2]);
    x + num / q
}

pub fn asinh_impl<const F: bool>(x: f64, log: fn(f64) -> f64) -> f64 {
    const NEAR_ZERO: f64 = 1.0 / (1u64 << 28) as f64;
    const LARGE: f64 = (1u64 << 28) as f64;
    if x.is_nan() || x.is_infinite() {
        return x;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let mut temp = if x > LARGE {
        log(x) + LN2
    } else if x > 2.0 {
        log(2.0 * x + 1.0 / (madd::<F>(x, x, 1.0).sqrt() + x))
    } else if x < NEAR_ZERO {
        x
    } else {
        log1p_impl::<F>(x + x * x / (1.0 + madd::<F>(x, x, 1.0).sqrt()))
    };
    if sign {
        temp = -temp;
    }
    temp
}

pub fn acosh_impl<const F: bool>(x: f64, log: fn(f64) -> f64) -> f64 {
    const LARGE: f64 = (1u64 << 28) as f64;
    if x < 1.0 || x.is_nan() {
        return f64::NAN;
    }
    if x == 1.0 {
        return 0.0;
    }
    if x >= LARGE {
        return log(x) + LN2;
    }
    if x > 2.0 {
        return log(2.0 * x - 1.0 / (x + nmsub::<F>(x, x, 1.0).sqrt()));
    }
    let t = x - 1.0;
    log1p_impl::<F>(t + madd::<F>(t, t, 2.0 * t).sqrt())
}

pub fn atanh_impl<const F: bool>(x: f64) -> f64 {
    const NEAR_ZERO: f64 = 1.0 / (1u64 << 28) as f64;
    if !(-1.0..=1.0).contains(&x) || x.is_nan() {
        return f64::NAN;
    }
    if x == 1.0 {
        return f64::INFINITY;
    }
    if x == -1.0 {
        return f64::NEG_INFINITY;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let mut temp = if x < NEAR_ZERO {
        x
    } else if x < 0.5 {
        let temp = x + x;
        0.5 * log1p_impl::<F>(temp + temp * x / (1.0 - x))
    } else {
        0.5 * log1p_impl::<F>((x + x) / (1.0 - x))
    };
    if sign {
        temp = -temp;
    }
    temp
}

pub fn sinh_arch(x: f64, a: Arch) -> f64 {
    if a.fused {
        sinh_impl::<true>(x, a.exp)
    } else {
        sinh_impl::<false>(x, a.exp)
    }
}

pub fn cosh_arch(x: f64, a: Arch) -> f64 {
    cosh_impl(x, a.exp)
}

pub fn tanh_arch(x: f64, a: Arch) -> f64 {
    if a.fused {
        tanh_impl::<true>(x, a.exp)
    } else {
        tanh_impl::<false>(x, a.exp)
    }
}

pub fn asinh_arch(x: f64, a: Arch) -> f64 {
    if a.fused {
        asinh_impl::<true>(x, a.log)
    } else {
        asinh_impl::<false>(x, a.log)
    }
}

pub fn acosh_arch(x: f64, a: Arch) -> f64 {
    if a.fused {
        acosh_impl::<true>(x, a.log)
    } else {
        acosh_impl::<false>(x, a.log)
    }
}

pub fn atanh_arch(x: f64, a: Arch) -> f64 {
    if a.fused {
        atanh_impl::<true>(x)
    } else {
        atanh_impl::<false>(x)
    }
}
