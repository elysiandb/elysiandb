use super::bits::{frexp, ldexp, modf_arm64, modf_go};
use super::exp::{exp_amd64, exp_arm64, exp_go};
use super::log::{log_amd64, log_go};

#[cfg(target_arch = "aarch64")]
pub fn pow(x: f64, y: f64) -> f64 {
    pow_arm64(x, y)
}

#[cfg(target_arch = "x86_64")]
pub fn pow(x: f64, y: f64) -> f64 {
    pow_amd64(x, y, super::use_fma())
}

#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
pub fn pow(x: f64, y: f64) -> f64 {
    pow_go(x, y)
}

pub fn pow_arm64(x: f64, y: f64) -> f64 {
    pow_with::<true>(x, y, exp_arm64, |x| log_go(x, true), modf_arm64)
}

pub fn pow_amd64(x: f64, y: f64, use_fma: bool) -> f64 {
    pow_with::<false>(x, y, move |x| exp_amd64(x, use_fma), log_amd64, modf_go)
}

pub fn pow_go(x: f64, y: f64) -> f64 {
    pow_with::<false>(x, y, exp_go, |x| log_go(x, false), modf_go)
}

fn is_odd_int(x: f64, modf: impl Fn(f64) -> (f64, f64)) -> bool {
    if x.abs() >= (1u64 << 53) as f64 {
        return false;
    }
    let (xi, xf) = modf(x);
    xf == 0.0 && (xi as i64) & 1 == 1
}

#[inline]
fn pow_with<const FUSED: bool>(
    x: f64,
    y: f64,
    exp: impl Fn(f64) -> f64 + Copy,
    log: impl Fn(f64) -> f64 + Copy,
    modf: impl Fn(f64) -> (f64, f64) + Copy,
) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if y == 1.0 {
        return x;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 {
        if y < 0.0 {
            if x.is_sign_negative() && is_odd_int(y, modf) {
                return f64::NEG_INFINITY;
            }
            return f64::INFINITY;
        }
        if y > 0.0 {
            if x.is_sign_negative() && is_odd_int(y, modf) {
                return x;
            }
            return 0.0;
        }
    } else if y.is_infinite() {
        if x == -1.0 {
            return 1.0;
        }
        if (x.abs() < 1.0) == (y == f64::INFINITY) {
            return 0.0;
        }
        return f64::INFINITY;
    } else if x.is_infinite() {
        if x == f64::NEG_INFINITY {
            return pow_with::<FUSED>(1.0 / x, -y, exp, log, modf);
        }
        if y < 0.0 {
            return 0.0;
        }
        if y > 0.0 {
            return f64::INFINITY;
        }
    } else if y == 0.5 {
        return x.sqrt();
    } else if y == -0.5 {
        return 1.0 / x.sqrt();
    }

    let (mut yi, mut yf) = modf(y.abs());
    if yf != 0.0 && x < 0.0 {
        return f64::NAN;
    }
    if yi >= 9.223_372_036_854_775_808e18 {
        if x == -1.0 {
            return 1.0;
        }
        if (x.abs() < 1.0) == (y > 0.0) {
            return 0.0;
        }
        return f64::INFINITY;
    }

    let mut a1 = 1.0;
    let mut ae: i64 = 0;

    if yf != 0.0 {
        if yf > 0.5 {
            yf -= 1.0;
            yi += 1.0;
        }
        a1 = exp(yf * log(x));
    }

    let (mut x1, mut xe) = frexp(x);
    let mut i = yi as i64;
    while i != 0 {
        if !(-(1 << 12)..=(1 << 12)).contains(&xe) {
            ae += xe;
            break;
        }
        if i & 1 == 1 {
            a1 *= x1;
            ae += xe;
        }
        let sq = x1 * x1;
        xe <<= 1;
        if sq < 0.5 {
            x1 = if FUSED { x1.mul_add(x1, sq) } else { sq + sq };
            xe -= 1;
        } else {
            x1 = sq;
        }
        i >>= 1;
    }

    if y < 0.0 {
        a1 = 1.0 / a1;
        ae = -ae;
    }
    ldexp(a1, ae)
}
