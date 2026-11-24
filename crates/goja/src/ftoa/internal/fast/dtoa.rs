use super::cachedpower::get_cached_power_for_binary_exponent_range;
use super::common::{_dcheck, DcheckFailure};
use super::diyfp::{DIY_FP_K_SIGNIFICAND_SIZE, Diyfp, Double, shl64, shr64};
use gocompat::strconv;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};

const K_MINIMAL_TARGET_EXPONENT: i64 = -60;
const K_MAXIMAL_TARGET_EXPONENT: i64 = -32;

const K_TEN4: u32 = 10000;
const K_TEN5: u32 = 100000;
const K_TEN6: u32 = 1000000;
const K_TEN7: u32 = 10000000;
const K_TEN8: u32 = 100000000;
const K_TEN9: u32 = 1000000000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Mode(pub i64);

pub const MODE_SHORTEST: Mode = Mode(0);
pub const MODE_PRECISION: Mode = Mode(1);

fn last_digit(buffer: &mut [u8]) -> &mut u8 {
    let n = buffer.len();
    &mut buffer[n - 1]
}

fn round_weed(
    buffer: &mut [u8],
    distance_too_high_w: u64,
    unsafe_interval: u64,
    mut rest: u64,
    ten_kappa: u64,
    unit: u64,
) -> bool {
    let small_distance = distance_too_high_w.wrapping_sub(unit);
    let big_distance = distance_too_high_w.wrapping_add(unit);
    _dcheck(rest <= unsafe_interval);
    while rest < small_distance
        && unsafe_interval.wrapping_sub(rest) >= ten_kappa
        && (rest.wrapping_add(ten_kappa) < small_distance
            || small_distance.wrapping_sub(rest)
                >= rest.wrapping_add(ten_kappa).wrapping_sub(small_distance))
    {
        let last = last_digit(buffer);
        *last = last.wrapping_sub(1);
        rest = rest.wrapping_add(ten_kappa);
    }
    if rest < big_distance
        && unsafe_interval.wrapping_sub(rest) >= ten_kappa
        && (rest.wrapping_add(ten_kappa) < big_distance
            || big_distance.wrapping_sub(rest)
                > rest.wrapping_add(ten_kappa).wrapping_sub(big_distance))
    {
        return false;
    }
    unit.wrapping_mul(2) <= rest && rest <= unsafe_interval.wrapping_sub(unit.wrapping_mul(4))
}

fn round_weed_counted(
    buffer: &mut [u8],
    rest: u64,
    ten_kappa: u64,
    unit: u64,
    kappa: &mut i64,
) -> bool {
    _dcheck(rest < ten_kappa);
    if unit >= ten_kappa {
        return false;
    }
    if ten_kappa.wrapping_sub(unit) <= unit {
        return false;
    }
    if ten_kappa.wrapping_sub(rest) > rest
        && ten_kappa.wrapping_sub(rest.wrapping_mul(2)) >= unit.wrapping_mul(2)
    {
        return true;
    }
    if rest > unit && ten_kappa.wrapping_sub(rest.wrapping_sub(unit)) <= rest.wrapping_sub(unit) {
        let last = last_digit(buffer);
        *last = last.wrapping_add(1);
        let mut i = buffer.len() - 1;
        while i > 0 {
            if buffer[i] != b'0' + 10 {
                break;
            }
            buffer[i] = b'0';
            buffer[i - 1] = buffer[i - 1].wrapping_add(1);
            i -= 1;
        }
        if buffer[0] == b'0' + 10 {
            buffer[0] = b'1';
            *kappa += 1;
        }
        return true;
    }
    false
}

fn biggest_power_ten(number: u32, number_bits: i64) -> (u32, i64) {
    const POWERS: [u32; 10] = [
        1, 10, 100, 1000, K_TEN4, K_TEN5, K_TEN6, K_TEN7, K_TEN8, K_TEN9,
    ];
    let mut exponent: i64 = match number_bits {
        30..=32 => 9,
        27..=29 => 8,
        24..=26 => 7,
        20..=23 => 6,
        17..=19 => 5,
        14..=16 => 4,
        10..=13 => 3,
        7..=9 => 2,
        4..=6 => 1,
        1..=3 => 0,
        0 => -1,
        _ => return (0, 0),
    };
    while exponent >= 0 {
        let power = POWERS[exponent as usize];
        if power <= number {
            return (power, exponent);
        }
        exponent -= 1;
    }
    (0, -1)
}

fn digit_gen(low: Diyfp, w: Diyfp, high: Diyfp, buffer: &mut Vec<u8>) -> (i64, bool) {
    _dcheck(low.e == w.e && w.e == high.e);
    _dcheck(low.f.wrapping_add(1) <= high.f.wrapping_sub(1));
    _dcheck(K_MINIMAL_TARGET_EXPONENT <= w.e && w.e <= K_MAXIMAL_TARGET_EXPONENT);
    let mut unit: u64 = 1;
    let too_low = Diyfp {
        f: low.f.wrapping_sub(unit),
        e: low.e,
    };
    let too_high = Diyfp {
        f: high.f.wrapping_add(unit),
        e: high.e,
    };
    let mut unsafe_interval = too_high.minus(too_low);
    let one = Diyfp {
        f: shl64(1, -w.e),
        e: w.e,
    };
    let mut integrals = shr64(too_high.f, -one.e) as u32;
    let mut fractionals = too_high.f & one.f.wrapping_sub(1);
    let (mut divisor, divisor_exponent) =
        biggest_power_ten(integrals, DIY_FP_K_SIGNIFICAND_SIZE - (-one.e));
    let mut kappa = divisor_exponent + 1;
    while kappa > 0 {
        let digit = (integrals / divisor) as i64;
        buffer.push((i64::from(b'0') + digit) as u8);
        integrals %= divisor;
        kappa -= 1;
        let rest = shl64(u64::from(integrals), -one.e).wrapping_add(fractionals);
        if rest < unsafe_interval.f {
            let res = round_weed(
                buffer,
                too_high.minus(w).f,
                unsafe_interval.f,
                rest,
                shl64(u64::from(divisor), -one.e),
                unit,
            );
            return (kappa, res);
        }
        divisor /= 10;
    }
    _dcheck(one.e >= -60);
    _dcheck(fractionals < one.f);
    _dcheck(0xFFFF_FFFF_FFFF_FFFF / 10 >= one.f);
    loop {
        fractionals = fractionals.wrapping_mul(10);
        unit = unit.wrapping_mul(10);
        unsafe_interval.f = unsafe_interval.f.wrapping_mul(10);
        let digit = shr64(fractionals, -one.e) as u8;
        buffer.push(b'0'.wrapping_add(digit));
        fractionals &= one.f.wrapping_sub(1);
        kappa -= 1;
        if fractionals < unsafe_interval.f {
            let res = round_weed(
                buffer,
                too_high.minus(w).f.wrapping_mul(unit),
                unsafe_interval.f,
                fractionals,
                one.f,
                unit,
            );
            return (kappa, res);
        }
    }
}

fn digit_gen_counted(w: Diyfp, mut requested_digits: i64, buffer: &mut Vec<u8>) -> (i64, bool) {
    _dcheck(K_MINIMAL_TARGET_EXPONENT <= w.e && w.e <= K_MAXIMAL_TARGET_EXPONENT);
    let mut w_error: u64 = 1;
    let one = Diyfp {
        f: shl64(1, -w.e),
        e: w.e,
    };
    let mut integrals = shr64(w.f, -one.e) as u32;
    let mut fractionals = w.f & one.f.wrapping_sub(1);
    let (mut divisor, divisor_exponent) =
        biggest_power_ten(integrals, DIY_FP_K_SIGNIFICAND_SIZE - (-one.e));
    let mut kappa = divisor_exponent + 1;
    while kappa > 0 {
        let digit = (integrals / divisor) as u8;
        buffer.push(b'0'.wrapping_add(digit));
        requested_digits -= 1;
        integrals %= divisor;
        kappa -= 1;
        if requested_digits == 0 {
            break;
        }
        divisor /= 10;
    }
    if requested_digits == 0 {
        let rest = shl64(u64::from(integrals), -one.e).wrapping_add(fractionals);
        let res = round_weed_counted(
            buffer,
            rest,
            shl64(u64::from(divisor), -one.e),
            w_error,
            &mut kappa,
        );
        return (kappa, res);
    }
    _dcheck(one.e >= -60);
    _dcheck(fractionals < one.f);
    _dcheck(0xFFFF_FFFF_FFFF_FFFF / 10 >= one.f);
    while requested_digits > 0 && fractionals > w_error {
        fractionals = fractionals.wrapping_mul(10);
        w_error = w_error.wrapping_mul(10);
        let digit = shr64(fractionals, -one.e) as u8;
        buffer.push(b'0'.wrapping_add(digit));
        requested_digits -= 1;
        fractionals &= one.f.wrapping_sub(1);
        kappa -= 1;
    }
    let res = if requested_digits != 0 {
        false
    } else {
        round_weed_counted(buffer, fractionals, one.f, w_error, &mut kappa)
    };
    (kappa, res)
}

fn grisu3(f: f64, buffer: &mut Vec<u8>) -> (i64, bool) {
    let v = Double(f);
    let w = v.to_normalized_diyfp();
    let (boundary_minus, boundary_plus) = v.normalized_boundaries();
    let ten_mk_minimal_binary_exponent =
        K_MINIMAL_TARGET_EXPONENT - (w.e + DIY_FP_K_SIGNIFICAND_SIZE);
    let ten_mk_maximal_binary_exponent =
        K_MAXIMAL_TARGET_EXPONENT - (w.e + DIY_FP_K_SIGNIFICAND_SIZE);
    let (ten_mk, mk) = get_cached_power_for_binary_exponent_range(
        ten_mk_minimal_binary_exponent,
        ten_mk_maximal_binary_exponent,
    );
    _dcheck(
        K_MINIMAL_TARGET_EXPONENT <= w.e + ten_mk.e + DIY_FP_K_SIGNIFICAND_SIZE
            && K_MAXIMAL_TARGET_EXPONENT >= w.e + ten_mk.e + DIY_FP_K_SIGNIFICAND_SIZE,
    );
    let scaled_w = w.times(ten_mk);
    _dcheck(scaled_w.e == boundary_plus.e + ten_mk.e + DIY_FP_K_SIGNIFICAND_SIZE);
    let scaled_boundary_minus = boundary_minus.times(ten_mk);
    let scaled_boundary_plus = boundary_plus.times(ten_mk);
    let (kappa, result) = digit_gen(
        scaled_boundary_minus,
        scaled_w,
        scaled_boundary_plus,
        buffer,
    );
    (-mk + kappa, result)
}

fn grisu3_counted(v: f64, requested_digits: i64, buffer: &mut Vec<u8>) -> (i64, bool) {
    let w = Double(v).to_normalized_diyfp();
    let ten_mk_minimal_binary_exponent =
        K_MINIMAL_TARGET_EXPONENT - (w.e + DIY_FP_K_SIGNIFICAND_SIZE);
    let ten_mk_maximal_binary_exponent =
        K_MAXIMAL_TARGET_EXPONENT - (w.e + DIY_FP_K_SIGNIFICAND_SIZE);
    let (ten_mk, mk) = get_cached_power_for_binary_exponent_range(
        ten_mk_minimal_binary_exponent,
        ten_mk_maximal_binary_exponent,
    );
    _dcheck(
        K_MINIMAL_TARGET_EXPONENT <= w.e + ten_mk.e + DIY_FP_K_SIGNIFICAND_SIZE
            && K_MAXIMAL_TARGET_EXPONENT >= w.e + ten_mk.e + DIY_FP_K_SIGNIFICAND_SIZE,
    );
    let scaled_w = w.times(ten_mk);
    let (kappa, result) = digit_gen_counted(scaled_w, requested_digits, buffer);
    (-mk + kappa, result)
}

pub fn dtoa(v: f64, mode: Mode, requested_digits: i64, buffer: &mut Vec<u8>) -> (i64, bool) {
    let start_pos = buffer.len();
    if mode != MODE_SHORTEST && mode != MODE_PRECISION && start_pos != 0 {
        panic!("slice bounds out of range [:{start_pos}] with capacity 0");
    }
    let run = catch_unwind(AssertUnwindSafe(|| match mode {
        MODE_SHORTEST => grisu3(v, buffer),
        MODE_PRECISION => grisu3_counted(v, requested_digits, buffer),
        _ => (0, false),
    }));
    let (decimal_exponent, result) = match run {
        Ok(r) => r,
        Err(payload) => {
            if payload.is::<DcheckFailure>() {
                panic!(
                    "DCHECK assertion failed while formatting {} in mode {}",
                    strconv::format_float(v, b'e', 50, 64),
                    mode.0
                );
            }
            resume_unwind(payload)
        }
    };
    if result {
        ((buffer.len() - start_pos) as i64 + decimal_exponent, true)
    } else {
        buffer.truncate(start_pos);
        (0, false)
    }
}
