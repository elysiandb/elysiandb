use super::{madd, msub};

const PI: f64 = f64::from_bits(0x4009_21fb_5444_2d18);
const PI_OVER_2: f64 = f64::from_bits(0x3ff9_21fb_5444_2d18);
const PI_OVER_4: f64 = f64::from_bits(0x3fe9_21fb_5444_2d18);
const THREE_PI_OVER_4: f64 = f64::from_bits(0x4002_d97c_7f33_21d2);
const HALF_MOREBITS: f64 = f64::from_bits(0x3c81_a626_3314_5c07);

fn xatan<const F: bool>(x: f64) -> f64 {
    const P0: f64 = -8.750608600031904122785e-01;
    const P1: f64 = -1.615753718733365076637e+01;
    const P2: f64 = -7.500855792314704667340e+01;
    const P3: f64 = -1.228866684490136173410e+02;
    const P4: f64 = -6.485021904942025371773e+01;
    const Q0: f64 = 2.485846490142306297962e+01;
    const Q1: f64 = 1.650270098316988542046e+02;
    const Q2: f64 = 4.328810604912902668951e+02;
    const Q3: f64 = 4.853903996359136964868e+02;
    const Q4: f64 = 1.945506571482613964425e+02;
    let mut z = x * x;
    let p = madd::<F>(
        madd::<F>(madd::<F>(madd::<F>(P0, z, P1), z, P2), z, P3),
        z,
        P4,
    );
    let num = z * p;
    let mut q = madd::<F>(x, x, Q0);
    q = madd::<F>(q, z, Q1);
    q = madd::<F>(q, z, Q2);
    q = madd::<F>(q, z, Q3);
    q = madd::<F>(q, z, Q4);
    z = num / q;
    madd::<F>(x, z, x)
}

fn satan<const F: bool>(x: f64) -> f64 {
    const MOREBITS: f64 = 6.123233995736765886130e-17;
    const TAN3PIO8: f64 = 2.41421356237309504880;
    if x <= 0.66 {
        return xatan::<F>(x);
    }
    if x > TAN3PIO8 {
        return PI_OVER_2 - xatan::<F>(1.0 / x) + MOREBITS;
    }
    PI_OVER_4 + xatan::<F>((x - 1.0) / (x + 1.0)) + HALF_MOREBITS
}

pub fn atan_impl<const F: bool>(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    if x > 0.0 {
        return satan::<F>(x);
    }
    -satan::<F>(-x)
}

pub fn asin_impl<const F: bool>(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    if x > 1.0 {
        return f64::NAN;
    }
    let mut temp = msub::<F>(x, x, 1.0).sqrt();
    if x > 0.7 {
        temp = PI_OVER_2 - satan::<F>(temp / x);
    } else {
        temp = satan::<F>(x / temp);
    }
    if sign {
        temp = -temp;
    }
    temp
}

pub fn acos_impl<const F: bool>(x: f64) -> f64 {
    PI_OVER_2 - asin_impl::<F>(x)
}

fn copysign(f: f64, sign: f64) -> f64 {
    f64::from_bits((f.to_bits() & !(1u64 << 63)) | (sign.to_bits() & (1u64 << 63)))
}

pub fn atan2_impl<const F: bool>(y: f64, x: f64) -> f64 {
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        if x >= 0.0 && !x.is_sign_negative() {
            return copysign(0.0, y);
        }
        return copysign(PI, y);
    }
    if x == 0.0 {
        return copysign(PI_OVER_2, y);
    }
    if x.is_infinite() {
        if x == f64::INFINITY {
            if y.is_infinite() {
                return copysign(PI_OVER_4, y);
            }
            return copysign(0.0, y);
        }
        if y.is_infinite() {
            return copysign(THREE_PI_OVER_4, y);
        }
        return copysign(PI, y);
    }
    if y.is_infinite() {
        return copysign(PI_OVER_2, y);
    }
    let q = atan_impl::<F>(y / x);
    if x < 0.0 {
        if q <= 0.0 {
            return q + PI;
        }
        return q - PI;
    }
    q
}

pub fn atan_go(x: f64, fused: bool) -> f64 {
    if fused {
        atan_impl::<true>(x)
    } else {
        atan_impl::<false>(x)
    }
}

pub fn asin_go(x: f64, fused: bool) -> f64 {
    if fused {
        asin_impl::<true>(x)
    } else {
        asin_impl::<false>(x)
    }
}

pub fn acos_go(x: f64, fused: bool) -> f64 {
    if fused {
        acos_impl::<true>(x)
    } else {
        acos_impl::<false>(x)
    }
}

pub fn atan2_go(y: f64, x: f64, fused: bool) -> f64 {
    if fused {
        atan2_impl::<true>(y, x)
    } else {
        atan2_impl::<false>(y, x)
    }
}
