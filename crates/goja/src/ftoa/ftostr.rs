use super::ftoa::{DTOA_MODES, ftoa};
use super::internal::fast;
use gocompat::strconv;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FToStrMode(pub i64);

pub const MODE_STANDARD: FToStrMode = FToStrMode(0);
pub const MODE_STANDARD_EXPONENTIAL: FToStrMode = FToStrMode(1);
pub const MODE_FIXED: FToStrMode = FToStrMode(2);
pub const MODE_EXPONENTIAL: FToStrMode = FToStrMode(3);
pub const MODE_PRECISION: FToStrMode = FToStrMode(4);

fn insert(b: &mut Vec<u8>, p: usize, c: u8) {
    b.insert(p, c);
}

fn expand(b: &mut Vec<u8>, delta: usize) {
    b.resize(b.len() + delta, 0);
}

pub fn f_to_str(d: f64, mode: FToStrMode, precision: i64, buffer: &mut Vec<u8>) {
    if d.is_nan() {
        buffer.extend_from_slice(b"NaN");
        return;
    }
    if d.is_infinite() {
        if d.is_sign_negative() {
            buffer.push(b'-');
        }
        buffer.extend_from_slice(b"Infinity");
        return;
    }

    let mut d = d;
    let mut mode = mode;
    if mode == MODE_FIXED && (d >= 1e21 || d <= -1e21) {
        mode = MODE_STANDARD;
    }

    let mut dec_pt: i64 = 0;
    let mut ok = false;
    let mut start_pos = buffer.len();

    if d != 0.0 {
        if d < 0.0 {
            buffer.push(b'-');
            d = -d;
            start_pos += 1;
        }
        match mode {
            MODE_STANDARD | MODE_STANDARD_EXPONENTIAL => {
                (dec_pt, ok) = fast::dtoa(d, fast::MODE_SHORTEST, 0, buffer);
            }
            MODE_EXPONENTIAL | MODE_PRECISION => {
                (dec_pt, ok) = fast::dtoa(d, fast::MODE_PRECISION, precision, buffer);
            }
            _ => {}
        }
    } else {
        buffer.push(b'0');
        dec_pt = 1;
        ok = true;
    }
    if !ok {
        dec_pt = ftoa(
            d,
            DTOA_MODES[mode.0 as usize],
            mode >= MODE_FIXED,
            precision,
            buffer,
        );
    }
    let mut exponential_notation = false;
    let mut min_n_digits: i64 = 0;
    let mut n_digits = (buffer.len() - start_pos) as i64;

    match mode {
        MODE_STANDARD => {
            if !(-5..=21).contains(&dec_pt) {
                exponential_notation = true;
            } else {
                min_n_digits = dec_pt;
            }
        }
        MODE_FIXED => {
            if precision >= 0 {
                min_n_digits = dec_pt.wrapping_add(precision);
            } else {
                min_n_digits = dec_pt;
            }
        }
        MODE_EXPONENTIAL => {
            min_n_digits = precision;
            exponential_notation = true;
        }
        MODE_STANDARD_EXPONENTIAL => {
            exponential_notation = true;
        }
        MODE_PRECISION => {
            min_n_digits = precision;
            if dec_pt < -5 || dec_pt > precision {
                exponential_notation = true;
            }
        }
        _ => {}
    }

    while n_digits < min_n_digits {
        buffer.push(b'0');
        n_digits += 1;
    }

    if exponential_notation {
        if n_digits != 1 {
            insert(buffer, start_pos + 1, b'.');
        }
        buffer.push(b'e');
        if dec_pt > 0 {
            buffer.push(b'+');
        }
        strconv::append_int(buffer, dec_pt.wrapping_sub(1), 10);
    } else if dec_pt != n_digits {
        if dec_pt > 0 {
            insert(buffer, start_pos + dec_pt as usize, b'.');
        } else {
            let shift = 2i64.wrapping_sub(dec_pt) as usize;
            let old_len = buffer.len();
            expand(buffer, shift);
            buffer.copy_within(start_pos..old_len, start_pos + shift);
            buffer[start_pos] = b'0';
            buffer[start_pos + 1] = b'.';
            for b in &mut buffer[start_pos + 2..start_pos + shift] {
                *b = b'0';
            }
        }
    }
}
