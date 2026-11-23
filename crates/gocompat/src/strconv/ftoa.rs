use super::atoi::lower;
use super::decimal::Decimal;
use super::ftoaryu::{DecimalSlice, ryu_ftoa_fixed32, ryu_ftoa_fixed64, ryu_ftoa_shortest};
use super::itoa::format_bits;
use super::quote::{LOWERHEX, UPPERHEX};

pub(super) struct FloatInfo {
    pub(super) mantbits: u32,
    pub(super) expbits: u32,
    pub(super) bias: isize,
}

pub(super) const FLOAT32INFO: FloatInfo = FloatInfo {
    mantbits: 23,
    expbits: 8,
    bias: -127,
};

pub(super) const FLOAT64INFO: FloatInfo = FloatInfo {
    mantbits: 52,
    expbits: 11,
    bias: -1023,
};

pub fn format_float(f: f64, fmt: u8, prec: i32, bit_size: u32) -> String {
    let mut dst = Vec::with_capacity(prec.saturating_add(4).max(24) as usize);
    generic_ftoa(&mut dst, f, fmt, prec as isize, bit_size);
    match String::from_utf8(dst) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

pub fn append_float(dst: &mut Vec<u8>, f: f64, fmt: u8, prec: i32, bit_size: u32) {
    generic_ftoa(dst, f, fmt, prec as isize, bit_size);
}

fn generic_ftoa(dst: &mut Vec<u8>, val: f64, fmt: u8, prec: isize, bit_size: u32) {
    let (bits, flt) = match bit_size {
        32 => (u64::from((val as f32).to_bits()), &FLOAT32INFO),
        64 => (val.to_bits(), &FLOAT64INFO),
        _ => panic!("illegal float bit size {bit_size}"),
    };

    let neg = bits >> (flt.expbits + flt.mantbits) != 0;
    let mut exp = ((bits >> flt.mantbits) as isize) & ((1isize << flt.expbits) - 1);
    let mut mant = bits & ((1u64 << flt.mantbits) - 1);

    if exp == (1isize << flt.expbits) - 1 {
        let s: &[u8] = if mant != 0 {
            b"NaN"
        } else if neg {
            b"-Inf"
        } else {
            b"+Inf"
        };
        dst.extend_from_slice(s);
        return;
    } else if exp == 0 {
        exp += 1;
    } else {
        mant |= 1u64 << flt.mantbits;
    }
    exp += flt.bias;

    if fmt == b'b' {
        fmt_b(dst, neg, mant, exp, flt);
        return;
    }
    if fmt == b'x' || fmt == b'X' {
        fmt_x(dst, prec, fmt, neg, mant, exp, flt);
        return;
    }

    let mut buf = [0u8; 32];
    let mut digs = DecimalSlice {
        d: &mut buf,
        nd: 0,
        dp: 0,
    };
    let mut ok = false;
    let mut prec = prec;
    let shortest = prec < 0;
    if shortest {
        ryu_ftoa_shortest(&mut digs, mant, exp - flt.mantbits as isize, flt);
        ok = true;
        match fmt {
            b'e' | b'E' => prec = (digs.nd - 1).max(0),
            b'f' => prec = (digs.nd - digs.dp).max(0),
            b'g' | b'G' => prec = digs.nd,
            _ => {}
        }
    } else if fmt != b'f' {
        let mut digits = prec;
        match fmt {
            b'e' | b'E' => digits += 1,
            b'g' | b'G' => {
                if prec == 0 {
                    prec = 1;
                }
                digits = prec;
            }
            _ => digits = 1,
        }
        if bit_size == 32 && digits <= 9 {
            ryu_ftoa_fixed32(&mut digs, mant as u32, exp - flt.mantbits as isize, digits);
            ok = true;
        } else if digits <= 18 {
            ryu_ftoa_fixed64(&mut digs, mant, exp - flt.mantbits as isize, digits);
            ok = true;
        }
    }
    if !ok {
        big_ftoa(dst, prec, fmt, neg, mant, exp, flt);
        return;
    }
    format_digits(dst, shortest, neg, digs.d, digs.nd, digs.dp, prec, fmt);
}

fn big_ftoa(
    dst: &mut Vec<u8>,
    prec: isize,
    fmt: u8,
    neg: bool,
    mant: u64,
    exp: isize,
    flt: &FloatInfo,
) {
    let mut d = Decimal::new();
    d.assign(mant);
    d.shift(exp - flt.mantbits as isize);
    let mut prec = prec;
    let shortest = prec < 0;
    if shortest {
        round_shortest(&mut d, mant, exp, flt);
        match fmt {
            b'e' | b'E' => prec = d.nd - 1,
            b'f' => prec = (d.nd - d.dp).max(0),
            b'g' | b'G' => prec = d.nd,
            _ => {}
        }
    } else {
        match fmt {
            b'e' | b'E' => d.round(prec + 1),
            b'f' => d.round(d.dp + prec),
            b'g' | b'G' => {
                if prec == 0 {
                    prec = 1;
                }
                d.round(prec);
            }
            _ => {}
        }
    }
    format_digits(dst, shortest, neg, &d.d, d.nd, d.dp, prec, fmt);
}

#[allow(clippy::too_many_arguments)]
fn format_digits(
    dst: &mut Vec<u8>,
    shortest: bool,
    neg: bool,
    d: &[u8],
    nd: isize,
    dp: isize,
    prec: isize,
    fmt: u8,
) {
    match fmt {
        b'e' | b'E' => fmt_e(dst, neg, d, nd, dp, prec, fmt),
        b'f' => fmt_f(dst, neg, d, nd, dp, prec),
        b'g' | b'G' => {
            let mut eprec = prec;
            if eprec > nd && nd >= dp {
                eprec = nd;
            }
            if shortest {
                eprec = 6;
            }
            let exp = dp - 1;
            let mut prec = prec;
            if exp < -4 || exp >= eprec {
                if prec > nd {
                    prec = nd;
                }
                let efmt = if fmt == b'g' { b'e' } else { b'E' };
                fmt_e(dst, neg, d, nd, dp, prec - 1, efmt);
                return;
            }
            if prec > dp {
                prec = nd;
            }
            fmt_f(dst, neg, d, nd, dp, (prec - dp).max(0));
        }
        _ => {
            dst.push(b'%');
            dst.push(fmt);
        }
    }
}

fn round_shortest(d: &mut Decimal, mant: u64, exp: isize, flt: &FloatInfo) {
    if mant == 0 {
        d.nd = 0;
        return;
    }

    let minexp = flt.bias + 1;
    if exp > minexp && 332 * (d.dp - d.nd) >= 100 * (exp - flt.mantbits as isize) {
        return;
    }

    let mut upper = Decimal::new();
    upper.assign(mant * 2 + 1);
    upper.shift(exp - flt.mantbits as isize - 1);

    let (mantlo, explo) = if mant > (1u64 << flt.mantbits) || exp == minexp {
        (mant - 1, exp)
    } else {
        (mant * 2 - 1, exp - 1)
    };
    let mut lower = Decimal::new();
    lower.assign(mantlo * 2 + 1);
    lower.shift(explo - flt.mantbits as isize - 1);

    let inclusive = mant.is_multiple_of(2);

    let mut upperdelta: u8 = 0;

    let mut ui: isize = 0;
    loop {
        let mi = ui - upper.dp + d.dp;
        if mi >= d.nd {
            break;
        }
        let li = ui - upper.dp + lower.dp;
        let mut l = b'0';
        if li >= 0 && li < lower.nd {
            l = lower.d[li as usize];
        }
        let mut m = b'0';
        if mi >= 0 {
            m = d.d[mi as usize];
        }
        let mut u = b'0';
        if ui < upper.nd {
            u = upper.d[ui as usize];
        }

        let okdown = l != m || (inclusive && li + 1 == lower.nd);

        if upperdelta == 0 && m + 1 < u {
            upperdelta = 2;
        } else if upperdelta == 0 && m != u {
            upperdelta = 1;
        } else if upperdelta == 1 && (m != b'9' || u != b'0') {
            upperdelta = 2;
        }
        let okup = upperdelta > 0 && (inclusive || upperdelta > 1 || ui + 1 < upper.nd);

        if okdown && okup {
            d.round(mi + 1);
            return;
        } else if okdown {
            d.round_down(mi + 1);
            return;
        } else if okup {
            d.round_up(mi + 1);
            return;
        }
        ui += 1;
    }
}

fn fmt_e(dst: &mut Vec<u8>, neg: bool, d: &[u8], nd: isize, dp: isize, prec: isize, fmt: u8) {
    if neg {
        dst.push(b'-');
    }

    let mut ch = b'0';
    if nd != 0 {
        ch = d[0];
    }
    dst.push(ch);

    if prec > 0 {
        dst.push(b'.');
        let mut i: isize = 1;
        let m = nd.min(prec + 1);
        if i < m {
            dst.extend_from_slice(&d[i as usize..m as usize]);
            i = m;
        }
        while i <= prec {
            dst.push(b'0');
            i += 1;
        }
    }

    dst.push(fmt);
    let mut exp = dp - 1;
    if nd == 0 {
        exp = 0;
    }
    if exp < 0 {
        ch = b'-';
        exp = -exp;
    } else {
        ch = b'+';
    }
    dst.push(ch);

    if exp < 10 {
        dst.push(b'0');
        dst.push(exp as u8 + b'0');
    } else if exp < 100 {
        dst.push((exp / 10) as u8 + b'0');
        dst.push((exp % 10) as u8 + b'0');
    } else {
        dst.push((exp / 100) as u8 + b'0');
        dst.push(((exp / 10) % 10) as u8 + b'0');
        dst.push((exp % 10) as u8 + b'0');
    }
}

fn fmt_f(dst: &mut Vec<u8>, neg: bool, d: &[u8], nd: isize, dp: isize, prec: isize) {
    if neg {
        dst.push(b'-');
    }

    if dp > 0 {
        let mut m = nd.min(dp);
        dst.extend_from_slice(&d[..m as usize]);
        while m < dp {
            dst.push(b'0');
            m += 1;
        }
    } else {
        dst.push(b'0');
    }

    if prec > 0 {
        dst.push(b'.');
        for i in 0..prec {
            let mut ch = b'0';
            let j = dp + i;
            if 0 <= j && j < nd {
                ch = d[j as usize];
            }
            dst.push(ch);
        }
    }
}

fn fmt_b(dst: &mut Vec<u8>, neg: bool, mant: u64, exp: isize, flt: &FloatInfo) {
    if neg {
        dst.push(b'-');
    }

    format_bits(dst, mant, 10, false);

    dst.push(b'p');

    let exp = exp - flt.mantbits as isize;
    if exp >= 0 {
        dst.push(b'+');
    }
    format_bits(dst, exp as i64 as u64, 10, exp < 0);
}

fn fmt_x(
    dst: &mut Vec<u8>,
    prec: isize,
    fmt: u8,
    neg: bool,
    mant: u64,
    exp: isize,
    flt: &FloatInfo,
) {
    let mut mant = mant;
    let mut exp = exp;
    if mant == 0 {
        exp = 0;
    }

    mant <<= 60 - flt.mantbits;
    while mant != 0 && (mant & (1u64 << 60)) == 0 {
        mant <<= 1;
        exp -= 1;
    }

    if (0..15).contains(&prec) {
        let shift = (prec * 4) as u32;
        let extra = (mant << shift) & ((1u64 << 60) - 1);
        mant >>= 60 - shift;
        if (extra | (mant & 1)) > (1u64 << 59) {
            mant += 1;
        }
        mant <<= 60 - shift;
        if (mant & (1u64 << 61)) != 0 {
            mant >>= 1;
            exp += 1;
        }
    }

    let hex = if fmt == b'X' { UPPERHEX } else { LOWERHEX };

    if neg {
        dst.push(b'-');
    }
    dst.push(b'0');
    dst.push(fmt);
    dst.push(b'0' + ((mant >> 60) & 1) as u8);

    mant <<= 4;
    if prec < 0 && mant != 0 {
        dst.push(b'.');
        while mant != 0 {
            dst.push(hex[((mant >> 60) & 15) as usize]);
            mant <<= 4;
        }
    } else if prec > 0 {
        dst.push(b'.');
        for _ in 0..prec {
            dst.push(hex[((mant >> 60) & 15) as usize]);
            mant <<= 4;
        }
    }

    let mut ch = b'P';
    if fmt == lower(fmt) {
        ch = b'p';
    }
    dst.push(ch);
    if exp < 0 {
        ch = b'-';
        exp = -exp;
    } else {
        ch = b'+';
    }
    dst.push(ch);

    if exp < 100 {
        dst.push((exp / 10) as u8 + b'0');
        dst.push((exp % 10) as u8 + b'0');
    } else if exp < 1000 {
        dst.push((exp / 100) as u8 + b'0');
        dst.push(((exp / 10) % 10) as u8 + b'0');
        dst.push((exp % 10) as u8 + b'0');
    } else {
        dst.push((exp / 1000) as u8 + b'0');
        dst.push(((exp / 100) % 10) as u8 + b'0');
        dst.push(((exp / 10) % 10) as u8 + b'0');
        dst.push((exp % 10) as u8 + b'0');
    }
}
