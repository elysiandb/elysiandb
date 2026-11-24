pub(super) const FRAC_MASK: u32 = 0xfffff;
pub(super) const EXP_SHIFT: u32 = 20;
pub(super) const EXP_MSK1: u32 = 0x100000;

pub(super) const EXP_SHIFT_L: u64 = 52;
pub(super) const EXP_MASK_SHIFTED: i64 = 0x7ff;
pub(super) const FRAC_MASK_L: u64 = 0xfffffffffffff;
pub(super) const EXP_MSK1_L: u64 = 0x10000000000000;
pub(super) const EXP_SHIFT1: u32 = 20;
pub(super) const EXP_MASK: u32 = 0x7ff00000;
pub(super) const BIAS: i64 = 1023;
pub(super) const P: i64 = 53;
pub(super) const BNDRY_MASK: u32 = 0xfffff;
pub(super) const LOG2_P: i64 = 1;

pub(super) fn lo0bits(mut x: u32) -> i64 {
    let mut k = 0;
    if x & 7 != 0 {
        if x & 1 != 0 {
            return 0;
        }
        if x & 2 != 0 {
            return 1;
        }
        return 2;
    }
    if x & 0xffff == 0 {
        k = 16;
        x >>= 16;
    }
    if x & 0xff == 0 {
        k += 8;
        x >>= 8;
    }
    if x & 0xf == 0 {
        k += 4;
        x >>= 4;
    }
    if x & 0x3 == 0 {
        k += 2;
        x >>= 2;
    }
    if x & 1 == 0 {
        k += 1;
        x >>= 1;
        if x & 1 == 0 {
            return 32;
        }
    }
    k
}

pub(super) fn hi0bits(mut x: u32) -> i64 {
    let mut k = 0;
    if x & 0xffff0000 == 0 {
        k = 16;
        x <<= 16;
    }
    if x & 0xff000000 == 0 {
        k += 8;
        x <<= 8;
    }
    if x & 0xf0000000 == 0 {
        k += 4;
        x <<= 4;
    }
    if x & 0xc0000000 == 0 {
        k += 2;
        x <<= 2;
    }
    if x & 0x80000000 == 0 {
        k += 1;
        if x & 0x40000000 == 0 {
            return 32;
        }
    }
    k
}

fn stuff_bits(bits: &mut [u8], offset: usize, val: u32) {
    bits[offset] = (val >> 24) as u8;
    bits[offset + 1] = (val >> 16) as u8;
    bits[offset + 2] = (val >> 8) as u8;
    bits[offset + 3] = val as u8;
}

pub(super) fn shl32(x: u32, n: i64) -> u32 {
    if n < 0 {
        panic!("negative shift amount");
    }
    if n >= 32 { 0 } else { x << n }
}

pub(super) fn shr32(x: u32, n: i64) -> u32 {
    if n < 0 {
        panic!("negative shift amount");
    }
    if n >= 32 { 0 } else { x >> n }
}

pub(super) fn d2b(d: f64) -> (i64, i64, Vec<u8>) {
    let d_bits = d.to_bits();
    let mut d0 = (d_bits >> 32) as u32;
    let d1 = d_bits as u32;

    let mut z = d0 & FRAC_MASK;
    d0 &= 0x7fffffff;

    let de = i64::from(d0 >> EXP_SHIFT);
    if de != 0 {
        z |= EXP_MSK1;
    }

    let mut k;
    let i;
    let dbl_bits;
    let mut y = d1;
    if y != 0 {
        let mut b = vec![0u8; 8];
        k = lo0bits(y);
        y = shr32(y, k);
        if k != 0 {
            stuff_bits(&mut b, 4, y | shl32(z, 32 - k));
            z = shr32(z, k);
        } else {
            stuff_bits(&mut b, 4, y);
        }
        stuff_bits(&mut b, 0, z);
        i = if z != 0 { 2 } else { 1 };
        dbl_bits = b;
    } else {
        let mut b = vec![0u8; 4];
        k = lo0bits(z);
        z = shr32(z, k);
        stuff_bits(&mut b, 0, z);
        k += 32;
        i = 1;
        dbl_bits = b;
    }

    let (e, bits) = if de != 0 {
        (de - BIAS - (P - 1) + k, P - k)
    } else {
        (de - BIAS - (P - 1) + 1 + k, 32 * i - hi0bits(z))
    };
    (e, bits, dbl_bits)
}

#[cfg(target_arch = "aarch64")]
pub(super) fn go_int64(f: f64) -> i64 {
    f as i64
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) fn go_int64(f: f64) -> i64 {
    const MIN: f64 = i64::MIN as f64;
    if f.is_nan() || !(MIN..-MIN).contains(&f) {
        i64::MIN
    } else {
        f as i64
    }
}

#[cfg(target_arch = "aarch64")]
pub(super) fn go_add_mul(a: f64, x: f64, y: f64) -> f64 {
    x.mul_add(y, a)
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) fn go_add_mul(a: f64, x: f64, y: f64) -> f64 {
    a + x * y
}

#[cfg(target_arch = "aarch64")]
pub(super) fn go_sub_mul(a: f64, x: f64, y: f64) -> f64 {
    (-x).mul_add(y, a)
}

#[cfg(not(target_arch = "aarch64"))]
pub(super) fn go_sub_mul(a: f64, x: f64, y: f64) -> f64 {
    a - x * y
}
