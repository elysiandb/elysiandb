use super::madd;

pub fn cbrt_impl<const F: bool>(x: f64) -> f64 {
    const B1: u64 = 715094163;
    const B2: u64 = 696219795;
    const C: f64 = 5.42857142857142815906e-01;
    const D: f64 = -7.05306122448979611050e-01;
    const E: f64 = 1.41428571428571436819e+00;
    const FF: f64 = 1.60714285714285720630e+00;
    const G: f64 = 3.57142857142857150787e-01;
    const SMALLEST_NORMAL: f64 = 2.22507385850720138309e-308;

    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }

    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }

    let mut t = f64::from_bits(x.to_bits() / 3 + (B1 << 32));
    if x < SMALLEST_NORMAL {
        t = (1u64 << 54) as f64;
        t *= x;
        t = f64::from_bits(t.to_bits() / 3 + (B2 << 32));
    }

    let mut r = t * t / x;
    let mut s = madd::<F>(r, t, C);
    t *= G + FF / (s + E + D / s);

    t = f64::from_bits((t.to_bits() & (0xF_FFFF_FFFC << 28)) + (1 << 30));

    s = t * t;
    r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t = madd::<F>(r, t, t);

    if sign {
        t = -t;
    }
    t
}

pub fn cbrt_go(x: f64, fused: bool) -> f64 {
    if fused {
        cbrt_impl::<true>(x)
    } else {
        cbrt_impl::<false>(x)
    }
}
