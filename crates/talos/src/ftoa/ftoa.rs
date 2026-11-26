use super::common::{
    BIAS, BNDRY_MASK, EXP_MASK, EXP_MSK1, EXP_SHIFT1, LOG2_P, P, d2b, go_add_mul, go_int64,
    go_sub_mul, hi0bits,
};
use crate::deps::big::Int;
use std::sync::OnceLock;

const EXP_11: u32 = 0x3ff00000;
const FRAC_MASK1: u32 = 0xfffff;
const BLETCH: i64 = 0x10;
const QUICK_MAX: i64 = 14;
const INT_MAX: i64 = 14;

#[allow(clippy::approx_constant)]
const LOG10_2: f64 = 0.301029995663981;

const TENS: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

const BIGTENS: [f64; 5] = [1e16, 1e32, 1e64, 1e128, 1e256];

const POW5_CACHE_LEN: usize = 7;

pub(super) const DTOA_MODES: [i64; 5] = [0, 0, 3, 2, 2];

struct Pow5 {
    big5: Int,
    big10: Int,
    p05: [Int; 3],
    pow5_cache: [Int; POW5_CACHE_LEN],
}

fn pow5() -> &'static Pow5 {
    static POW5: OnceLock<Pow5> = OnceLock::new();
    POW5.get_or_init(|| {
        let mut p = Int::new_int(625);
        let mut cache: Vec<Int> = vec![p.clone()];
        for _ in 1..POW5_CACHE_LEN {
            p = p.mul(&p);
            cache.push(p.clone());
        }
        Pow5 {
            big5: Int::new_int(5),
            big10: Int::new_int(10),
            p05: [Int::new_int(5), Int::new_int(25), Int::new_int(125)],
            pow5_cache: cache.try_into().unwrap_or_else(|_| unreachable!()),
        }
    })
}

pub fn ftoa(d: f64, mode: i64, bias_up: bool, ndigits: i64, buf: &mut Vec<u8>) -> i64 {
    let mut d = d;
    let mut mode = mode;
    let mut ndigits = ndigits;
    let start_pos = buf.len();
    let (be, bbits, dbl_bits) = d2b(d);

    let d_bits = d.to_bits();
    let word0 = (d_bits >> 32) as u32;
    let word1 = d_bits as u32;

    let mut i = i64::from((word0 >> EXP_SHIFT1) & (EXP_MASK >> EXP_SHIFT1));
    let mut d2;
    let denorm;
    if i != 0 {
        d2 = set_word0(d, (word0 & FRAC_MASK1) | EXP_11);
        i -= BIAS;
        denorm = false;
    } else {
        i = bbits + be + (BIAS + (P - 1) - 1);
        let x: u64 = if i > 32 {
            shl64(u64::from(word0), 64 - i) | shr64(u64::from(word1), i - 32)
        } else {
            shl64(u64::from(word1), 32 - i)
        };
        d2 = set_word0(
            x as f64,
            (x >> 32).wrapping_sub(31 * u64::from(EXP_MASK)) as u32,
        );
        i -= (BIAS + (P - 1) - 1) + 1;
        denorm = true;
    }
    let mut ds = go_add_mul(
        go_add_mul(0.1760912590558, d2 - 1.5, 0.289529654602168),
        i as f64,
        LOG10_2,
    );
    let mut k = go_int64(ds);
    if ds < 0.0 && ds != k as f64 {
        k = k.wrapping_sub(1);
    }
    let mut k_check = true;
    if k >= 0 && k < TENS.len() as i64 {
        if d < TENS[k as usize] {
            k = k.wrapping_sub(1);
        }
        k_check = false;
    }
    let mut j = bbits - i - 1;
    let mut b2;
    let mut s2;
    let mut b5;
    let mut s5;
    if j >= 0 {
        b2 = 0;
        s2 = j;
    } else {
        b2 = -j;
        s2 = 0;
    }
    if k >= 0 {
        b5 = 0;
        s5 = k;
        s2 = s2.wrapping_add(k);
    } else {
        b2 = b2.wrapping_sub(k);
        b5 = k.wrapping_neg();
        s5 = 0;
    }
    if !(0..=9).contains(&mode) {
        mode = 0;
    }
    let mut try_quick = true;
    if mode > 5 {
        mode -= 4;
        try_quick = false;
    }
    let mut leftright = true;
    let mut ilim;
    let ilim1;
    match mode {
        0 | 1 => {
            ilim = -1;
            ilim1 = -1;
            ndigits = 0;
        }
        2 | 4 => {
            if mode == 2 {
                leftright = false;
            }
            if ndigits <= 0 {
                ndigits = 1;
            }
            ilim = ndigits;
            ilim1 = ndigits;
        }
        3 | 5 => {
            if mode == 3 {
                leftright = false;
            }
            i = ndigits.wrapping_add(k).wrapping_add(1);
            ilim = i;
            ilim1 = i.wrapping_sub(1);
        }
        _ => {
            ilim = 0;
            ilim1 = 0;
        }
    }
    let mut fast_failed = false;
    if (0..=QUICK_MAX).contains(&ilim) && try_quick {
        i = 0;
        d2 = d;
        let k0 = k;
        let ilim0 = ilim;
        let mut ieps: i64 = 2;
        if k > 0 {
            ds = TENS[(k & 0xf) as usize];
            j = k >> 4;
            if j & BLETCH != 0 {
                j &= BLETCH - 1;
                d /= BIGTENS[BIGTENS.len() - 1];
                ieps += 1;
            }
            while j != 0 {
                if j & 1 != 0 {
                    ieps += 1;
                    ds *= BIGTENS[i as usize];
                }
                j >>= 1;
                i += 1;
            }
            d /= ds;
        } else {
            let j1 = k.wrapping_neg();
            if j1 != 0 {
                d *= TENS[(j1 & 0xf) as usize];
                j = j1 >> 4;
                while j != 0 {
                    if j & 1 != 0 {
                        ieps += 1;
                        d *= BIGTENS[i as usize];
                    }
                    j >>= 1;
                    i += 1;
                }
            }
        }
        if k_check && d < 1.0 && ilim > 0 {
            if ilim1 <= 0 {
                fast_failed = true;
            } else {
                ilim = ilim1;
                k = k.wrapping_sub(1);
                d *= 10.0;
                ieps += 1;
            }
        }
        let mut eps = go_add_mul(7.0, ieps as f64, d);
        eps = set_word0(eps, _word0(eps).wrapping_sub(((P - 1) as u32) * EXP_MSK1));
        if ilim == 0 {
            d -= 5.0;
            if d > eps {
                buf.push(b'1');
                k = k.wrapping_add(1);
                return k.wrapping_add(1);
            }
            if d < -eps {
                buf.push(b'0');
                return 1;
            }
            fast_failed = true;
        }
        if !fast_failed {
            fast_failed = true;
            if leftright {
                eps = 0.5 / TENS[(ilim - 1) as usize] - eps;
                i = 0;
                loop {
                    let l = go_int64(d);
                    d -= l as f64;
                    buf.push(i64::from(b'0').wrapping_add(l) as u8);
                    if d < eps {
                        return k.wrapping_add(1);
                    }
                    if 1.0 - d < eps {
                        k = bump_up(buf, k);
                        return k.wrapping_add(1);
                    }
                    i += 1;
                    if i >= ilim {
                        break;
                    }
                    eps *= 10.0;
                    d *= 10.0;
                }
            } else {
                let eps_scale = TENS[(ilim - 1) as usize];
                i = 1;
                loop {
                    let l = go_int64(d);
                    d -= l as f64;
                    buf.push(i64::from(b'0').wrapping_add(l) as u8);
                    if i == ilim {
                        if d > go_add_mul(0.5, eps, eps_scale) {
                            k = bump_up(buf, k);
                            return k.wrapping_add(1);
                        } else if d < go_sub_mul(0.5, eps, eps_scale) {
                            strip_trailing_zeroes(buf, start_pos);
                            return k.wrapping_add(1);
                        }
                        break;
                    }
                    d *= 10.0;
                    i += 1;
                }
            }
        }
        if fast_failed {
            buf.truncate(start_pos);
            d = d2;
            k = k0;
            ilim = ilim0;
        }
    }

    if be >= 0 && k <= INT_MAX {
        ds = TENS[k as usize];
        if ndigits < 0 && ilim <= 0 {
            if ilim < 0 || d < 5.0 * ds || (!bias_up && d == 5.0 * ds) {
                buf.truncate(start_pos);
                buf.push(b'0');
                return 1;
            }
            buf.push(b'1');
            k = k.wrapping_add(1);
            return k.wrapping_add(1);
        }
        i = 1;
        loop {
            let l = go_int64(d / ds);
            d = go_sub_mul(d, l as f64, ds);
            buf.push(i64::from(b'0').wrapping_add(l) as u8);
            if i == ilim {
                d += d;
                if d > ds || (d == ds && ((l & 1) != 0 || bias_up)) {
                    k = bump_up(buf, k);
                }
                break;
            }
            d *= 10.0;
            if d == 0.0 {
                break;
            }
            i += 1;
        }
        return k.wrapping_add(1);
    }

    let mut m2 = b2;
    let mut m5 = b5;
    let mut mhi: Option<Int> = None;
    if leftright {
        if mode < 2 {
            i = if denorm {
                be + (BIAS + (P - 1) - 1 + 1)
            } else {
                1 + P - bbits
            };
        } else {
            j = ilim.wrapping_sub(1);
            if m5 >= j {
                m5 = m5.wrapping_sub(j);
            } else {
                j = j.wrapping_sub(m5);
                s5 = s5.wrapping_add(j);
                b5 = b5.wrapping_add(j);
                m5 = 0;
            }
            i = ilim;
            if i < 0 {
                m2 = m2.wrapping_sub(i);
                i = 0;
            }
        }
        b2 = b2.wrapping_add(i);
        s2 = s2.wrapping_add(i);
        mhi = Some(Int::new_int(1));
    }

    if m2 > 0 && s2 > 0 {
        i = if m2 < s2 { m2 } else { s2 };
        b2 = b2.wrapping_sub(i);
        m2 = m2.wrapping_sub(i);
        s2 = s2.wrapping_sub(i);
    }

    let consts = pow5();
    let mut b = Int::set_bytes(&dbl_bits);
    if b5 > 0 {
        if leftright {
            if m5 > 0 {
                let h = pow5mult(mhi.take().unwrap_or_default(), m5);
                b = h.mul(&b);
                mhi = Some(h);
            }
            j = b5.wrapping_sub(m5);
            if j != 0 {
                b = pow5mult(b, j);
            }
        } else {
            b = pow5mult(b, b5);
        }
    }

    let mut s = Int::new_int(1);
    if s5 > 0 {
        s = pow5mult(s, s5);
    }

    let mut spec_case = false;
    if mode < 2
        && _word1(d) == 0
        && (_word0(d) & BNDRY_MASK) == 0
        && (_word0(d) & (EXP_MASK & (EXP_MASK << 1))) != 0
    {
        b2 = b2.wrapping_add(LOG2_P);
        s2 = s2.wrapping_add(LOG2_P);
        spec_case = true;
    }

    let zz = if s5 != 0 {
        let s_bytes = s.bytes();
        let mut s_hi_word: u32 = 0;
        for idx in 0..4 {
            s_hi_word <<= 8;
            if idx < s_bytes.len() {
                s_hi_word |= u32::from(s_bytes[idx]);
            }
        }
        32 - hi0bits(s_hi_word)
    } else {
        1
    };
    i = zz.wrapping_add(s2) & 0x1f;
    if i != 0 {
        i = 32 - i;
    }
    if i > 4 {
        i -= 4;
        b2 = b2.wrapping_add(i);
        m2 = m2.wrapping_add(i);
        s2 = s2.wrapping_add(i);
    } else if i < 4 {
        i += 28;
        b2 = b2.wrapping_add(i);
        m2 = m2.wrapping_add(i);
        s2 = s2.wrapping_add(i);
    }
    if b2 > 0 {
        b = b.lsh(b2 as u64);
    }
    if s2 > 0 {
        s = s.lsh(s2 as u64);
    }
    if k_check && b.cmp(&s) < 0 {
        k = k.wrapping_sub(1);
        b = b.mul(&consts.big10);
        if leftright {
            mhi = mhi.map(|h| h.mul(&consts.big10));
        }
        ilim = ilim1;
    }

    if ilim <= 0 && mode > 2 {
        if ilim >= 0 {
            s = s.mul(&consts.big5);
            i = i64::from(b.cmp(&s));
        }
        if ilim < 0 || i < 0 || (i == 0 && !bias_up) {
            buf.truncate(start_pos);
            buf.push(b'0');
            return 1;
        }
        buf.push(b'1');
        k = k.wrapping_add(1);
        return k.wrapping_add(1);
    }

    let mut dig: u8;
    if leftright {
        let mut mlo = mhi.unwrap_or_default();
        if m2 > 0 {
            mlo = mlo.lsh(m2 as u64);
        }
        let mut mhi: Option<Int> = None;
        if spec_case {
            mhi = Some(mlo.lsh(LOG2_P as u64));
        }
        i = 1;
        loop {
            let (z, r) = b.div_mod(&s);
            b = r;
            dig = z.int64().wrapping_add(i64::from(b'0')) as u8;
            j = i64::from(b.cmp(&mlo));
            let delta = s.sub(mhi.as_ref().unwrap_or(&mlo));
            let mut j1 = if delta.sign() <= 0 {
                1
            } else {
                i64::from(b.cmp(&delta))
            };
            if j1 == 0 && mode == 0 && (_word1(d) & 1) == 0 {
                if dig == b'9' {
                    buf.push(b'9');
                    if round_off(buf, start_pos) {
                        k = k.wrapping_add(1);
                        buf.push(b'1');
                    }
                    return k.wrapping_add(1);
                }
                if j > 0 {
                    dig = dig.wrapping_add(1);
                }
                buf.push(dig);
                return k.wrapping_add(1);
            }
            if j < 0 || (j == 0 && mode == 0 && (_word1(d) & 1) == 0) {
                if j1 > 0 {
                    b = b.lsh(1);
                    j1 = i64::from(b.cmp(&s));
                    if j1 > 0 || (j1 == 0 && ((dig & 1) == 1 || bias_up)) {
                        dig = dig.wrapping_add(1);
                        if dig == b'9' {
                            buf.push(b'9');
                            if round_off(buf, start_pos) {
                                k = k.wrapping_add(1);
                                buf.push(b'1');
                            }
                            return k.wrapping_add(1);
                        }
                    }
                }
                buf.push(dig);
                return k.wrapping_add(1);
            }
            if j1 > 0 {
                if dig == b'9' {
                    buf.push(b'9');
                    if round_off(buf, start_pos) {
                        k = k.wrapping_add(1);
                        buf.push(b'1');
                    }
                    return k.wrapping_add(1);
                }
                buf.push(dig.wrapping_add(1));
                return k.wrapping_add(1);
            }
            buf.push(dig);
            if i == ilim {
                break;
            }
            b = b.mul(&consts.big10);
            mlo = mlo.mul(&consts.big10);
            if let Some(h) = mhi.as_mut() {
                *h = h.mul(&consts.big10);
            }
            i += 1;
        }
    } else {
        i = 1;
        loop {
            let (z, r) = b.div_mod(&s);
            b = r;
            dig = z.int64().wrapping_add(i64::from(b'0')) as u8;
            buf.push(dig);
            if i >= ilim {
                break;
            }
            b = b.mul(&consts.big10);
            i += 1;
        }
    }

    b = b.lsh(1);
    j = i64::from(b.cmp(&s));
    if j > 0 || (j == 0 && ((dig & 1) == 1 || bias_up)) {
        if round_off(buf, start_pos) {
            k = k.wrapping_add(1);
            buf.push(b'1');
            return k.wrapping_add(1);
        }
    } else {
        strip_trailing_zeroes(buf, start_pos);
    }

    k.wrapping_add(1)
}

fn bump_up(buf: &mut Vec<u8>, k: i64) -> i64 {
    let mut k = k;
    let mut last_ch;
    let stop = if !buf.is_empty() && buf[0] == b'-' {
        1
    } else {
        0
    };
    loop {
        last_ch = buf[buf.len() - 1];
        buf.truncate(buf.len() - 1);
        if last_ch != b'9' {
            break;
        }
        if buf.len() == stop {
            k = k.wrapping_add(1);
            last_ch = b'0';
            break;
        }
    }
    buf.push(last_ch.wrapping_add(1));
    k
}

fn set_word0(d: f64, w: u32) -> f64 {
    let d_bits = d.to_bits();
    f64::from_bits((u64::from(w) << 32) | (d_bits & 0xffffffff))
}

fn _word0(d: f64) -> u32 {
    (d.to_bits() >> 32) as u32
}

fn _word1(d: f64) -> u32 {
    d.to_bits() as u32
}

fn strip_trailing_zeroes(buf: &mut Vec<u8>, start_pos: usize) {
    let mut bl = buf.len();
    while bl > start_pos && buf[bl - 1] == b'0' {
        bl -= 1;
    }
    buf.truncate(bl);
}

fn pow5mult(b: Int, k: i64) -> Int {
    let consts = pow5();
    if k < (1 << (POW5_CACHE_LEN + 2)) {
        let mut b = b;
        let mut k = k;
        let mut i = k & 3;
        if i != 0 {
            b = b.mul(&consts.p05[(i - 1) as usize]);
        }
        k >>= 2;
        i = 0;
        loop {
            if k & 1 != 0 {
                b = b.mul(&consts.pow5_cache[i as usize]);
            }
            k >>= 1;
            if k == 0 {
                break;
            }
            i += 1;
        }
        return b;
    }
    let p = consts
        .big5
        .exp(&Int::new_int(k), None)
        .unwrap_or_else(|| unreachable!());
    b.mul(&p)
}

fn round_off(buf: &mut Vec<u8>, start_pos: usize) -> bool {
    let mut i = buf.len();
    while i != start_pos {
        i -= 1;
        if buf[i] != b'9' {
            buf[i] = buf[i].wrapping_add(1);
            buf.truncate(i + 1);
            return false;
        }
    }
    buf.truncate(start_pos);
    true
}

fn shl64(x: u64, n: i64) -> u64 {
    if n < 0 {
        panic!("negative shift amount");
    }
    if n >= 64 { 0 } else { x << n }
}

fn shr64(x: u64, n: i64) -> u64 {
    if n < 0 {
        panic!("negative shift amount");
    }
    if n >= 64 { 0 } else { x >> n }
}
