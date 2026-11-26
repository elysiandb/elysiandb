use super::common::{
    BIAS, BNDRY_MASK, EXP_MASK, EXP_MASK_SHIFTED, EXP_MSK1_L, EXP_SHIFT_L, EXP_SHIFT1, FRAC_MASK_L,
    LOG2_P, P, d2b, go_int64,
};
use crate::deps::big::Int;
use gocompat::strconv;

const DIGITS: &[u8; 36] = b"0123456789abcdefghijklmnopqrstuvwxyz";

pub fn f_to_base_str(num: f64, radix: i64) -> String {
    let mut num = num;
    let mut negative = false;
    if num < 0.0 {
        num = -num;
        negative = true;
    }

    let dfloor = num.floor();
    let mut ldfloor = go_int64(dfloor);
    let int_digits = if dfloor == ldfloor as f64 {
        if negative {
            ldfloor = ldfloor.wrapping_neg();
        }
        strconv::format_int(ldfloor, radix as u32)
    } else {
        let floor_bits = num.to_bits();
        let mut exp = ((floor_bits >> EXP_SHIFT_L) as i64) & EXP_MASK_SHIFTED;
        let mut mantissa = if exp == 0 {
            ((floor_bits & FRAC_MASK_L) << 1) as i64
        } else {
            ((floor_bits & FRAC_MASK_L) | EXP_MSK1_L) as i64
        };
        if negative {
            mantissa = mantissa.wrapping_neg();
        }
        exp -= 1075;
        let mut x = Int::new_int(mantissa);
        if exp > 0 {
            x = x.lsh(exp as u64);
        } else if exp < 0 {
            x = x.rsh((-exp) as u64);
        }
        x.text(radix as u32)
    };

    if num == dfloor {
        return int_digits;
    }

    let mut buffer = int_digits;
    buffer.push('.');
    let df = num - dfloor;

    let d_bits = num.to_bits();
    let word0 = (d_bits >> 32) as u32;
    let word1 = d_bits as u32;

    let (e, _, dbl_bits) = d2b(df);

    let mut s2 = -i64::from((word0 >> EXP_SHIFT1) & (EXP_MASK >> EXP_SHIFT1));
    if s2 == 0 {
        s2 = -1;
    }
    s2 += BIAS + P;
    if -s2 >= e {
        panic!("-s2 >= e: {}, {}", -s2, e);
    }
    let mut mlo = Int::new_int(1);
    let mut mhi: Option<Int> = None;
    if word1 == 0 && (word0 & BNDRY_MASK) == 0 && (word0 & (EXP_MASK & (EXP_MASK << 1))) != 0 {
        s2 += LOG2_P;
        mhi = Some(Int::new_int(1 << LOG2_P));
    }

    let mut b = Int::set_bytes(&dbl_bits).lsh((e + s2) as u64);
    let s = Int::new_int(1).lsh(s2 as u64);
    let big_base = Int::new_int(radix);

    let mut done = false;
    while !done {
        b = b.mul(&big_base);
        let (q, m) = b.div_mod(&s);
        let mut digit = q.int64() as u8;
        b = m;
        mlo = mlo.mul(&big_base);
        if let Some(h) = mhi.as_mut() {
            *h = h.mul(&big_base);
        }

        let j = b.cmp(&mlo);

        let delta = s.sub(mhi.as_ref().unwrap_or(&mlo));
        let mut j1 = if delta.sign() <= 0 { 1 } else { b.cmp(&delta) };
        if j1 == 0 && (word1 & 1) == 0 {
            if j > 0 {
                digit = digit.wrapping_add(1);
            }
            done = true;
        } else if j < 0 || (j == 0 && (word1 & 1) == 0) {
            if j1 > 0 {
                b = b.lsh(1);
                j1 = b.cmp(&s);
                if j1 > 0 {
                    digit = digit.wrapping_add(1);
                }
            }
            done = true;
        } else if j1 > 0 {
            digit = digit.wrapping_add(1);
            done = true;
        }
        buffer.push(char::from(DIGITS[usize::from(digit)]));
    }

    buffer
}
