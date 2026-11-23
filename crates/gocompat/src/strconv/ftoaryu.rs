use super::eisel_lemire::{
    DETAILED_POWERS_OF_TEN, DETAILED_POWERS_OF_TEN_MAX_EXP10, DETAILED_POWERS_OF_TEN_MIN_EXP10,
};
use super::ftoa::FloatInfo;
use super::itoa::SMALLS_STRING;
use std::cmp::Ordering;

pub(super) struct DecimalSlice<'a> {
    pub(super) d: &'a mut [u8],
    pub(super) nd: isize,
    pub(super) dp: isize,
}

impl DecimalSlice<'_> {
    fn advance(&mut self, n: usize) {
        let d = std::mem::take(&mut self.d);
        self.d = &mut d[n..];
    }
}

pub(super) fn mul64(x: u64, y: u64) -> (u64, u64) {
    let p = u128::from(x) * u128::from(y);
    ((p >> 64) as u64, p as u64)
}

fn add64(x: u64, y: u64, carry: u64) -> (u64, u64) {
    let (s1, c1) = x.overflowing_add(y);
    let (s2, c2) = s1.overflowing_add(carry);
    (s2, u64::from(c1 || c2))
}

fn mask32(extra: u32) -> u32 {
    1u32.checked_shl(extra).unwrap_or(0).wrapping_sub(1)
}

fn mask64(extra: u32) -> u64 {
    1u64.checked_shl(extra).unwrap_or(0).wrapping_sub(1)
}

pub(super) fn ryu_ftoa_fixed32(d: &mut DecimalSlice, mant: u32, exp: isize, prec: isize) {
    if prec < 0 {
        panic!("fixed precision formatting called with a negative precision");
    }
    if prec > 9 {
        panic!("fixed precision formatting called with a precision above 9");
    }
    if mant == 0 {
        d.nd = 0;
        d.dp = 0;
        return;
    }
    let mut mant = mant;
    let mut e2 = exp;
    let b = (32 - mant.leading_zeros()) as isize;
    if b < 25 {
        mant <<= (25 - b) as u32;
        e2 += b - 25;
    }
    let q = -mul_by_log2_log10(e2 + 24) + prec - 1;

    let mut exact = (0..=27).contains(&q);

    let (mut di, dexp2, mut d0) = mult64bit_pow10(mant, e2, q);
    if dexp2 >= 0 {
        panic!("not enough significant bits after the power of ten multiplication");
    }
    if (-10..0).contains(&q) && divisible_by_power5(u64::from(mant), -q) {
        exact = true;
        d0 = true;
    }
    let extra = (-dexp2) as u32;
    let extra_mask = mask32(extra);

    let dfrac = di & extra_mask;
    di = di.checked_shr(extra).unwrap_or(0);
    let half = 1u32 << (extra - 1);
    let round_up = if exact {
        dfrac > half || (dfrac == half && (!d0 || (di & 1) == 1))
    } else {
        (dfrac >> (extra - 1)) == 1
    };
    if dfrac != 0 {
        d0 = false;
    }
    format_decimal(d, u64::from(di), !d0, round_up, prec);
    d.dp -= q;
}

pub(super) fn ryu_ftoa_fixed64(d: &mut DecimalSlice, mant: u64, exp: isize, prec: isize) {
    if prec > 18 {
        panic!("fixed precision formatting called with a precision above 18");
    }
    if mant == 0 {
        d.nd = 0;
        d.dp = 0;
        return;
    }
    let mut mant = mant;
    let mut e2 = exp;
    let b = (64 - mant.leading_zeros()) as isize;
    if b < 55 {
        mant <<= (55 - b) as u32;
        e2 += b - 55;
    }
    let q = -mul_by_log2_log10(e2 + 54) + prec - 1;

    let mut exact = (0..=55).contains(&q);

    let (mut di, dexp2, mut d0) = mult128bit_pow10(mant, e2, q);
    if dexp2 >= 0 {
        panic!("not enough significant bits after the power of ten multiplication");
    }
    if (-22..0).contains(&q) && divisible_by_power5(mant, -q) {
        exact = true;
        d0 = true;
    }
    let extra = (-dexp2) as u32;
    let extra_mask = mask64(extra);

    let dfrac = di & extra_mask;
    di = di.checked_shr(extra).unwrap_or(0);
    let half = 1u64 << (extra - 1);
    let round_up = if exact {
        dfrac > half || (dfrac == half && (!d0 || (di & 1) == 1))
    } else {
        (dfrac >> (extra - 1)) == 1
    };
    if dfrac != 0 {
        d0 = false;
    }
    format_decimal(d, di, !d0, round_up, prec);
    d.dp -= q;
}

static UINT64POW10: [u64; 20] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
    1_000_000_000_000_000_000,
    10_000_000_000_000_000_000,
];

fn format_decimal(d: &mut DecimalSlice, m: u64, trunc: bool, round_up: bool, prec: isize) {
    let max = UINT64POW10[prec as usize];
    let mut m = m;
    let mut trunc = trunc;
    let mut round_up = round_up;
    let mut trimmed: isize = 0;
    while m >= max {
        let a = m / 10;
        let b = m % 10;
        m = a;
        trimmed += 1;
        round_up = match b.cmp(&5) {
            Ordering::Greater => true,
            Ordering::Less => false,
            Ordering::Equal => trunc || (m & 1) == 1,
        };
        if b != 0 {
            trunc = true;
        }
    }
    if round_up {
        m += 1;
    }
    if m >= max {
        m /= 10;
        trimmed += 1;
    }
    let mut n = prec as usize;
    d.nd = prec;
    let mut v = m;
    while v >= 100 {
        let v1 = v / 100;
        let v2 = (v % 100) as usize;
        n -= 2;
        d.d[n + 1] = SMALLS_STRING[2 * v2 + 1];
        d.d[n] = SMALLS_STRING[2 * v2];
        v = v1;
    }
    if v > 0 {
        n -= 1;
        d.d[n] = SMALLS_STRING[2 * v as usize + 1];
    }
    if v >= 10 {
        n -= 1;
        d.d[n] = SMALLS_STRING[2 * v as usize];
    }
    while d.d[(d.nd - 1) as usize] == b'0' {
        d.nd -= 1;
        trimmed += 1;
    }
    d.dp = d.nd + trimmed;
}

pub(super) fn ryu_ftoa_shortest(d: &mut DecimalSlice, mant: u64, exp: isize, flt: &FloatInfo) {
    if mant == 0 {
        d.nd = 0;
        d.dp = 0;
        return;
    }
    if exp <= 0 && mant.trailing_zeros() as isize >= -exp {
        let mant = mant >> (-exp) as u32;
        ryu_digits(d, mant, mant, mant, true, false);
        return;
    }
    let (ml, mc, mu, bounds_e2) = compute_bounds(mant, exp, flt);
    if bounds_e2 == 0 {
        ryu_digits(d, ml, mc, mu, true, false);
        return;
    }
    let q = mul_by_log2_log10(-bounds_e2) + 1;

    let (mut dl, mut dc, mut du, mut dl0, mut dc0, mut du0, e2) = if flt.mantbits == 23 {
        let (l, _, l0) = mult64bit_pow10(ml as u32, bounds_e2, q);
        let (c, _, c0) = mult64bit_pow10(mc as u32, bounds_e2, q);
        let (u, e, u0) = mult64bit_pow10(mu as u32, bounds_e2, q);
        (u64::from(l), u64::from(c), u64::from(u), l0, c0, u0, e)
    } else {
        let (l, _, l0) = mult128bit_pow10(ml, bounds_e2, q);
        let (c, _, c0) = mult128bit_pow10(mc, bounds_e2, q);
        let (u, e, u0) = mult128bit_pow10(mu, bounds_e2, q);
        (l, c, u, l0, c0, u0, e)
    };
    if e2 >= 0 {
        panic!("not enough significant bits after the power of ten multiplication");
    }
    if q > 55 {
        dl0 = false;
        dc0 = false;
        du0 = false;
    }
    if (-24..0).contains(&q) {
        if divisible_by_power5(ml, -q) {
            dl0 = true;
        }
        if divisible_by_power5(mc, -q) {
            dc0 = true;
        }
        if divisible_by_power5(mu, -q) {
            du0 = true;
        }
    }
    let extra = (-e2) as u32;
    let extra_mask = mask64(extra);
    let fracl = dl & extra_mask;
    dl = dl.checked_shr(extra).unwrap_or(0);
    let fracc = dc & extra_mask;
    dc = dc.checked_shr(extra).unwrap_or(0);
    let fracu = du & extra_mask;
    du = du.checked_shr(extra).unwrap_or(0);
    let mut uok = !du0 || fracu > 0;
    if du0 && fracu == 0 {
        uok = (mant & 1) == 0;
    }
    if !uok {
        du = du.wrapping_sub(1);
    }
    let half = 1u64 << (extra - 1);
    let cup = if dc0 {
        fracc > half || (fracc == half && (dc & 1) == 1)
    } else {
        (fracc >> (extra - 1)) == 1
    };
    let lok = dl0 && fracl == 0 && (mant & 1) == 0;
    if !lok {
        dl = dl.wrapping_add(1);
    }
    let c0 = dc0 && fracc == 0;
    ryu_digits(d, dl, dc, du, c0, cup);
    d.dp -= q;
}

fn mul_by_log2_log10(x: isize) -> isize {
    (x * 78913) >> 18
}

fn mul_by_log10_log2(x: isize) -> isize {
    (x * 108853) >> 15
}

fn compute_bounds(mant: u64, exp: isize, flt: &FloatInfo) -> (u64, u64, u64, isize) {
    if mant != (1u64 << flt.mantbits) || exp == flt.bias + 1 - flt.mantbits as isize {
        ((2 * mant).wrapping_sub(1), 2 * mant, 2 * mant + 1, exp - 1)
    } else {
        (4 * mant - 1, 4 * mant, 4 * mant + 2, exp - 2)
    }
}

fn ryu_digits(d: &mut DecimalSlice, lower: u64, central: u64, upper: u64, c0: bool, cup: bool) {
    let (lhi, llo) = divmod1e9(lower);
    let (chi, clo) = divmod1e9(central);
    let (uhi, ulo) = divmod1e9(upper);
    if uhi == 0 {
        ryu_digits32(d, llo, clo, ulo, c0, cup, 8);
    } else if lhi < uhi {
        let mut lhi = lhi;
        if llo != 0 {
            lhi += 1;
        }
        let c0 = c0 && clo == 0;
        let cup = (clo > 500_000_000) || (clo == 500_000_000 && cup);
        ryu_digits32(d, lhi, chi, uhi, c0, cup, 8);
        d.dp += 9;
    } else {
        d.nd = 0;
        let mut n: usize = 9;
        let mut v = chi;
        while v > 0 {
            let v1 = v / 10;
            let v2 = v % 10;
            v = v1;
            n -= 1;
            d.d[n] = v2 as u8 + b'0';
        }
        d.advance(n);
        d.nd = (9 - n) as isize;
        let end = d.nd + 8;
        ryu_digits32(d, llo, clo, ulo, c0, cup, end);
    }
    while d.nd > 0 && d.d[(d.nd - 1) as usize] == b'0' {
        d.nd -= 1;
    }
    while d.nd > 0 && d.d[0] == b'0' {
        d.nd -= 1;
        d.dp -= 1;
        d.advance(1);
    }
}

fn ryu_digits32(
    d: &mut DecimalSlice,
    lower: u32,
    central: u32,
    upper: u32,
    c0: bool,
    cup: bool,
    endindex: isize,
) {
    if upper == 0 {
        d.dp = endindex + 1;
        return;
    }
    let mut lower = lower;
    let mut central = central;
    let mut upper = upper;
    let mut c0 = c0;
    let mut cup = cup;
    let mut trimmed: isize = 0;
    let mut c_next_digit: u32 = 0;
    while upper > 0 {
        let l = lower.div_ceil(10);
        let mut c = central / 10;
        let mut cdigit = central % 10;
        let u = upper / 10;
        if l > u {
            break;
        }
        if l == c + 1 && c < u {
            c += 1;
            cdigit = 0;
            cup = false;
        }
        trimmed += 1;
        c0 = c0 && c_next_digit == 0;
        c_next_digit = cdigit;
        lower = l;
        central = c;
        upper = u;
    }
    if trimmed > 0 {
        cup = c_next_digit > 5 || (c_next_digit == 5 && (!c0 || (central & 1) == 1));
    }
    if central < upper && cup {
        central += 1;
    }
    let endindex = endindex - trimmed;
    let mut v = central;
    let mut n = endindex;
    while n > d.nd {
        let v1 = v / 100;
        let v2 = (v % 100) as usize;
        d.d[n as usize] = SMALLS_STRING[2 * v2 + 1];
        d.d[(n - 1) as usize] = SMALLS_STRING[2 * v2];
        n -= 2;
        v = v1;
    }
    if n == d.nd {
        d.d[n as usize] = v as u8 + b'0';
    }
    d.nd = endindex + 1;
    d.dp = d.nd + trimmed;
}

fn mult64bit_pow10(m: u32, e2: isize, q: isize) -> (u32, isize, bool) {
    if q == 0 {
        return (m << 6, e2 - 6, true);
    }
    if !(DETAILED_POWERS_OF_TEN_MIN_EXP10..=DETAILED_POWERS_OF_TEN_MAX_EXP10).contains(&q) {
        panic!("power of ten out of range");
    }
    let mut pow = DETAILED_POWERS_OF_TEN[(q - DETAILED_POWERS_OF_TEN_MIN_EXP10) as usize][1];
    if q < 0 {
        pow = pow.wrapping_add(1);
    }
    let (hi, lo) = mul64(u64::from(m), pow);
    let e2 = e2 + mul_by_log10_log2(q) - 63 + 57;
    (((hi << 7) | (lo >> 57)) as u32, e2, (lo << 7) == 0)
}

fn mult128bit_pow10(m: u64, e2: isize, q: isize) -> (u64, isize, bool) {
    if q == 0 {
        return (m << 8, e2 - 8, true);
    }
    if !(DETAILED_POWERS_OF_TEN_MIN_EXP10..=DETAILED_POWERS_OF_TEN_MAX_EXP10).contains(&q) {
        panic!("power of ten out of range");
    }
    let mut pow = DETAILED_POWERS_OF_TEN[(q - DETAILED_POWERS_OF_TEN_MIN_EXP10) as usize];
    if q < 0 {
        pow[0] = pow[0].wrapping_add(1);
    }
    let e2 = e2 + mul_by_log10_log2(q) - 127 + 119;

    let (l1, l0) = mul64(m, pow[0]);
    let (h1, h0) = mul64(m, pow[1]);
    let (mid, carry) = add64(l1, h0, 0);
    let h1 = h1.wrapping_add(carry);
    ((h1 << 9) | (mid >> 55), e2, (mid << 9) == 0 && l0 == 0)
}

fn divisible_by_power5(m: u64, k: isize) -> bool {
    if m == 0 {
        return true;
    }
    let mut m = m;
    for _ in 0..k {
        if !m.is_multiple_of(5) {
            return false;
        }
        m /= 5;
    }
    true
}

fn divmod1e9(x: u64) -> (u32, u32) {
    ((x / 1_000_000_000) as u32, (x % 1_000_000_000) as u32)
}
