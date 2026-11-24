use super::bits::{BIAS, MASK, SHIFT};
use super::{madd, msub};

const PI4A: f64 = 7.85398125648498535156e-1;
const PI4B: f64 = 3.77489470793079817668e-8;
const PI4C: f64 = 2.69515142907905952645e-15;
const REDUCE_THRESHOLD: f64 = (1u64 << 29) as f64;
const FOUR_OVER_PI: f64 = f64::from_bits(0x3ff4_5f30_6dc9_c883);
const PI_OVER_4: f64 = f64::from_bits(0x3fe9_21fb_5444_2d18);

const SIN: [f64; 6] = [
    1.58962301576546568060e-10,
    -2.50507477628578072866e-8,
    2.75573136213857245213e-6,
    -1.98412698295895385996e-4,
    8.33333333332211858878e-3,
    -1.66666666666666307295e-1,
];

const COS: [f64; 6] = [
    -1.13585365213876817300e-11,
    2.08757008419747316778e-9,
    -2.75573141792967388112e-7,
    2.48015872888517045348e-5,
    -1.38888888888730564116e-3,
    4.16666666666665929218e-2,
];

const TAN_P: [f64; 3] = [
    -1.30936939181383777646e4,
    1.15351664838587416140e6,
    -1.79565251976484877988e7,
];

const TAN_Q: [f64; 5] = [
    1.00000000000000000000e0,
    1.36812963470692954678e4,
    -1.32089234440210967447e6,
    2.50083801823357915839e7,
    -5.38695755929454629881e7,
];

const M_PI4: [u64; 20] = [
    0x0000000000000001,
    0x45f306dc9c882a53,
    0xf84eafa3ea69bb81,
    0xb6c52b3278872083,
    0xfca2c757bd778ac3,
    0x6e48dc74849ba5c0,
    0x0c925dd413a32439,
    0xfc3bd63962534e7d,
    0xd1046bea5d768909,
    0xd338e04d68befc82,
    0x7323ac7306a673e9,
    0x3908bf177bf25076,
    0x3ff12fffbc0b301f,
    0xde5e2316b414da3e,
    0xda6cfd9e4f96136e,
    0x9e8c7ecd3cbfd45a,
    0xea4f758fd7cbe2f6,
    0x7a0e73ef14a525d4,
    0xd7f6bf623f1aba10,
    0xac06608df8f6d757,
];

fn shl(x: u64, s: u64) -> u64 {
    if s >= 64 { 0 } else { x << s }
}

fn shr(x: u64, s: u64) -> u64 {
    if s >= 64 { 0 } else { x >> s }
}

fn mul64(x: u64, y: u64) -> (u64, u64) {
    let p = u128::from(x) * u128::from(y);
    ((p >> 64) as u64, p as u64)
}

fn add64(x: u64, y: u64, carry: u64) -> (u64, u64) {
    let sum = x.wrapping_add(y).wrapping_add(carry);
    let carry_out = ((x & y) | ((x | y) & !sum)) >> 63;
    (sum, carry_out)
}

pub fn trig_reduce(x: f64) -> (u64, f64) {
    if x < PI_OVER_4 {
        return (0, x);
    }
    let mut ix = x.to_bits();
    let exp = ((ix >> SHIFT) & MASK) as i64 - BIAS - SHIFT as i64;
    ix &= !(MASK << SHIFT);
    ix |= 1 << SHIFT;
    let e61 = (exp + 61) as u64;
    let (digit, bitshift) = ((e61 / 64) as usize, e61 % 64);
    let z0 = shl(M_PI4[digit], bitshift) | shr(M_PI4[digit + 1], 64 - bitshift);
    let z1 = shl(M_PI4[digit + 1], bitshift) | shr(M_PI4[digit + 2], 64 - bitshift);
    let z2 = shl(M_PI4[digit + 2], bitshift) | shr(M_PI4[digit + 3], 64 - bitshift);
    let (z2hi, _) = mul64(z2, ix);
    let (z1hi, z1lo) = mul64(z1, ix);
    let z0lo = z0.wrapping_mul(ix);
    let (lo, c) = add64(z1lo, z2hi, 0);
    let (hi, _) = add64(z0lo, z1hi, c);
    let mut j = hi >> 61;
    let mut hi = (hi << 3) | (lo >> 61);
    let lz = u64::from(hi.leading_zeros());
    let e = (BIAS as u64).wrapping_sub(lz + 1);
    hi = shl(hi, lz + 1) | shr(lo, 64 - (lz + 1));
    hi >>= 64 - SHIFT;
    hi |= e << SHIFT;
    let mut z = f64::from_bits(hi);
    if j & 1 == 1 {
        j += 1;
        j &= 7;
        z -= 1.0;
    }
    (j, z * PI_OVER_4)
}

fn poly<const F: bool>(c: &[f64; 6], zz: f64) -> f64 {
    let mut p = c[0];
    for &k in &c[1..] {
        p = madd::<F>(p, zz, k);
    }
    p
}

fn reduce<const F: bool>(x: f64, mask: bool) -> (u64, f64) {
    if x >= REDUCE_THRESHOLD {
        return trig_reduce(x);
    }
    let mut j = (x * FOUR_OVER_PI) as u64;
    let mut y = j as f64;
    if j & 1 == 1 {
        j += 1;
        y += 1.0;
    }
    if mask {
        j &= 7;
    }
    let z = msub::<F>(y, PI4C, msub::<F>(y, PI4B, msub::<F>(y, PI4A, x)));
    (j, z)
}

fn sin_poly<const F: bool>(z: f64, zz: f64) -> f64 {
    madd::<F>(z * zz, poly::<F>(&SIN, zz), z)
}

fn cos_poly<const F: bool>(zz: f64) -> f64 {
    madd::<F>(zz * zz, poly::<F>(&COS, zz), msub::<F>(zz, 0.5, 1.0))
}

pub fn cos_impl<const F: bool>(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let x = x.abs();
    let (mut j, z) = reduce::<F>(x, true);
    if j > 3 {
        j -= 4;
        sign = !sign;
    }
    if j > 1 {
        sign = !sign;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        sin_poly::<F>(z, zz)
    } else {
        cos_poly::<F>(zz)
    };
    if sign { -y } else { y }
}

pub fn sin_impl<const F: bool>(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let (mut j, z) = reduce::<F>(x, true);
    if j > 3 {
        sign = !sign;
        j -= 4;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        cos_poly::<F>(zz)
    } else {
        sin_poly::<F>(z, zz)
    };
    if sign { -y } else { y }
}

pub fn tan_impl<const F: bool>(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let (j, z) = reduce::<F>(x, false);
    let zz = z * z;
    let mut y = if zz > 1e-14 {
        let p = madd::<F>(madd::<F>(TAN_P[0], zz, TAN_P[1]), zz, TAN_P[2]);
        let num = zz * p;
        let mut q = madd::<F>(z, z, TAN_Q[1]);
        q = madd::<F>(q, zz, TAN_Q[2]);
        q = madd::<F>(q, zz, TAN_Q[3]);
        q = madd::<F>(q, zz, TAN_Q[4]);
        madd::<F>(num / q, z, z)
    } else {
        z
    };
    if j & 2 == 2 {
        y = -1.0 / y;
    }
    if sign {
        y = -y;
    }
    y
}

pub fn sin_go(x: f64, fused: bool) -> f64 {
    if fused {
        sin_impl::<true>(x)
    } else {
        sin_impl::<false>(x)
    }
}

pub fn cos_go(x: f64, fused: bool) -> f64 {
    if fused {
        cos_impl::<true>(x)
    } else {
        cos_impl::<false>(x)
    }
}

pub fn tan_go(x: f64, fused: bool) -> f64 {
    if fused {
        tan_impl::<true>(x)
    } else {
        tan_impl::<false>(x)
    }
}
