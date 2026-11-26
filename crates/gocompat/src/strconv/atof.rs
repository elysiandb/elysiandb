use super::atoi::{ErrorKind, NumError, Target, lower, underscore_ok};
use super::decimal::Decimal;
use super::eisel_lemire::{eisel_lemire32, eisel_lemire64};
use super::ftoa::{FLOAT32INFO, FLOAT64INFO, FloatInfo};

const GO_NAN64: u64 = 0x7FF8000000000001;
const GO_NAN32: u32 = 0x7FC00000;

fn common_prefix_len_ignore_case(s: &[u8], prefix: &[u8]) -> usize {
    for (i, (&c, &p)) in s.iter().zip(prefix).enumerate() {
        let c = if c.is_ascii_uppercase() {
            c + (b'a' - b'A')
        } else {
            c
        };
        if c != p {
            return i;
        }
    }
    prefix.len().min(s.len())
}

fn special(s: &[u8]) -> Option<(f64, usize)> {
    if s.is_empty() {
        return None;
    }
    let mut sign = 1;
    let mut nsign = 0;
    match s[0] {
        b'+' | b'-' | b'i' | b'I' => {
            let mut s = s;
            if s[0] == b'+' || s[0] == b'-' {
                if s[0] == b'-' {
                    sign = -1;
                }
                nsign = 1;
                s = &s[1..];
            }
            let mut n = common_prefix_len_ignore_case(s, b"infinity");
            if (4..8).contains(&n) {
                n = 3;
            }
            if n == 3 || n == 8 {
                let f = if sign < 0 {
                    f64::NEG_INFINITY
                } else {
                    f64::INFINITY
                };
                return Some((f, nsign + n));
            }
        }
        b'n' | b'N' => {
            if common_prefix_len_ignore_case(s, b"nan") == 3 {
                return Some((f64::from_bits(GO_NAN64), 3));
            }
        }
        _ => {}
    }
    None
}

impl Decimal {
    pub(super) fn set(&mut self, s: &[u8]) -> bool {
        let mut i = 0;
        self.neg = false;
        self.trunc = false;

        if i >= s.len() {
            return false;
        }
        if s[i] == b'+' {
            i += 1;
        } else if s[i] == b'-' {
            self.neg = true;
            i += 1;
        }

        let mut sawdot = false;
        let mut sawdigits = false;
        while i < s.len() {
            let c = s[i];
            if c == b'_' {
                i += 1;
                continue;
            }
            if c == b'.' {
                if sawdot {
                    return false;
                }
                sawdot = true;
                self.dp = self.nd;
                i += 1;
                continue;
            }
            if c.is_ascii_digit() {
                sawdigits = true;
                if c == b'0' && self.nd == 0 {
                    self.dp -= 1;
                    i += 1;
                    continue;
                }
                if (self.nd as usize) < self.d.len() {
                    self.d[self.nd as usize] = c;
                    self.nd += 1;
                } else if c != b'0' {
                    self.trunc = true;
                }
                i += 1;
                continue;
            }
            break;
        }
        if !sawdigits {
            return false;
        }
        if !sawdot {
            self.dp = self.nd;
        }

        if i < s.len() && lower(s[i]) == b'e' {
            i += 1;
            if i >= s.len() {
                return false;
            }
            let mut esign: isize = 1;
            if s[i] == b'+' {
                i += 1;
            } else if s[i] == b'-' {
                i += 1;
                esign = -1;
            }
            if i >= s.len() || !s[i].is_ascii_digit() {
                return false;
            }
            let mut e: isize = 0;
            while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'_') {
                if s[i] == b'_' {
                    i += 1;
                    continue;
                }
                if e < 10000 {
                    e = e * 10 + isize::from(s[i] - b'0');
                }
                i += 1;
            }
            self.dp += e * esign;
        }

        i == s.len()
    }

    pub(super) fn float_bits(&mut self, flt: &FloatInfo) -> (u64, bool) {
        let mut exp: isize;
        let mut mant: u64;
        let mut overflow = false;

        'out: {
            'overflow: {
                if self.nd == 0 {
                    mant = 0;
                    exp = flt.bias;
                    break 'out;
                }

                if self.dp > 310 {
                    break 'overflow;
                }
                if self.dp < -330 {
                    mant = 0;
                    exp = flt.bias;
                    break 'out;
                }

                exp = 0;
                while self.dp > 0 {
                    let n = if self.dp >= POWTAB.len() as isize {
                        27
                    } else {
                        POWTAB[self.dp as usize]
                    };
                    self.shift(-n);
                    exp += n;
                }
                while self.dp < 0 || (self.dp == 0 && self.d[0] < b'5') {
                    let n = if -self.dp >= POWTAB.len() as isize {
                        27
                    } else {
                        POWTAB[(-self.dp) as usize]
                    };
                    self.shift(n);
                    exp -= n;
                }

                exp -= 1;

                if exp < flt.bias + 1 {
                    let n = flt.bias + 1 - exp;
                    self.shift(-n);
                    exp += n;
                }

                if exp - flt.bias >= (1isize << flt.expbits) - 1 {
                    break 'overflow;
                }

                self.shift(1 + flt.mantbits as isize);
                mant = self.rounded_integer();

                if mant == 2u64 << flt.mantbits {
                    mant >>= 1;
                    exp += 1;
                    if exp - flt.bias >= (1isize << flt.expbits) - 1 {
                        break 'overflow;
                    }
                }

                if (mant & (1u64 << flt.mantbits)) == 0 {
                    exp = flt.bias;
                }
                break 'out;
            }
            mant = 0;
            exp = (1isize << flt.expbits) - 1 + flt.bias;
            overflow = true;
        }

        let mut bits = mant & ((1u64 << flt.mantbits) - 1);
        bits |= (((exp - flt.bias) & ((1isize << flt.expbits) - 1)) as u64) << flt.mantbits;
        if self.neg {
            bits |= (1u64 << flt.mantbits) << flt.expbits;
        }
        (bits, overflow)
    }
}

static POWTAB: [isize; 9] = [1, 3, 6, 9, 13, 16, 19, 23, 26];

struct ReadFloat {
    mantissa: u64,
    exp: isize,
    neg: bool,
    trunc: bool,
    hex: bool,
    n: usize,
    ok: bool,
}

fn read_float(s: &[u8]) -> ReadFloat {
    let mut r = ReadFloat {
        mantissa: 0,
        exp: 0,
        neg: false,
        trunc: false,
        hex: false,
        n: 0,
        ok: false,
    };
    let mut underscores = false;
    let mut i = 0usize;

    if i >= s.len() {
        return r;
    }
    if s[i] == b'+' {
        i += 1;
    } else if s[i] == b'-' {
        r.neg = true;
        i += 1;
    }

    let mut base: u64 = 10;
    let mut max_mant_digits: isize = 19;
    let mut exp_char = b'e';
    if i + 2 < s.len() && s[i] == b'0' && lower(s[i + 1]) == b'x' {
        base = 16;
        max_mant_digits = 16;
        i += 2;
        exp_char = b'p';
        r.hex = true;
    }
    let mut sawdot = false;
    let mut sawdigits = false;
    let mut nd: isize = 0;
    let mut nd_mant: isize = 0;
    let mut dp: isize = 0;
    while i < s.len() {
        let c = s[i];
        if c == b'_' {
            underscores = true;
            i += 1;
            continue;
        }
        if c == b'.' {
            if sawdot {
                break;
            }
            sawdot = true;
            dp = nd;
            i += 1;
            continue;
        }
        if c.is_ascii_digit() {
            sawdigits = true;
            if c == b'0' && nd == 0 {
                dp -= 1;
                i += 1;
                continue;
            }
            nd += 1;
            if nd_mant < max_mant_digits {
                r.mantissa *= base;
                r.mantissa += u64::from(c - b'0');
                nd_mant += 1;
            } else if c != b'0' {
                r.trunc = true;
            }
            i += 1;
            continue;
        }
        if base == 16 && (b'a'..=b'f').contains(&lower(c)) {
            sawdigits = true;
            nd += 1;
            if nd_mant < max_mant_digits {
                r.mantissa *= 16;
                r.mantissa += u64::from(lower(c) - b'a' + 10);
                nd_mant += 1;
            } else {
                r.trunc = true;
            }
            i += 1;
            continue;
        }
        break;
    }
    r.n = i;
    if !sawdigits {
        return r;
    }
    if !sawdot {
        dp = nd;
    }

    if base == 16 {
        dp *= 4;
        nd_mant *= 4;
    }

    if i < s.len() && lower(s[i]) == exp_char {
        i += 1;
        r.n = i;
        if i >= s.len() {
            return r;
        }
        let mut esign: isize = 1;
        if s[i] == b'+' {
            i += 1;
        } else if s[i] == b'-' {
            i += 1;
            esign = -1;
        }
        r.n = i;
        if i >= s.len() || !s[i].is_ascii_digit() {
            return r;
        }
        let mut e: isize = 0;
        while i < s.len() && (s[i].is_ascii_digit() || s[i] == b'_') {
            if s[i] == b'_' {
                underscores = true;
                i += 1;
                continue;
            }
            if e < 10000 {
                e = e * 10 + isize::from(s[i] - b'0');
            }
            i += 1;
        }
        r.n = i;
        dp += e * esign;
    } else if base == 16 {
        return r;
    }

    if r.mantissa != 0 {
        r.exp = dp - nd_mant;
    }

    if underscores && !underscore_ok(&s[..i]) {
        return r;
    }

    r.ok = true;
    r
}

static FLOAT64POW10: [f64; 23] = [
    1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10, 1e11, 1e12, 1e13, 1e14, 1e15, 1e16,
    1e17, 1e18, 1e19, 1e20, 1e21, 1e22,
];

static FLOAT32POW10: [f32; 11] = [1e0, 1e1, 1e2, 1e3, 1e4, 1e5, 1e6, 1e7, 1e8, 1e9, 1e10];

fn atof64exact(mantissa: u64, exp: isize, neg: bool) -> Option<f64> {
    if mantissa >> FLOAT64INFO.mantbits != 0 {
        return None;
    }
    let mut f = mantissa as f64;
    if neg {
        f = -f;
    }
    if exp == 0 {
        return Some(f);
    }
    if (1..=15 + 22).contains(&exp) {
        let mut exp = exp;
        if exp > 22 {
            f *= FLOAT64POW10[(exp - 22) as usize];
            exp = 22;
        }
        if !(-1e15..=1e15).contains(&f) {
            return None;
        }
        return Some(f * FLOAT64POW10[exp as usize]);
    }
    if (-22..0).contains(&exp) {
        return Some(f / FLOAT64POW10[(-exp) as usize]);
    }
    None
}

fn atof32exact(mantissa: u64, exp: isize, neg: bool) -> Option<f32> {
    if mantissa >> FLOAT32INFO.mantbits != 0 {
        return None;
    }
    let mut f = mantissa as f32;
    if neg {
        f = -f;
    }
    if exp == 0 {
        return Some(f);
    }
    if (1..=7 + 10).contains(&exp) {
        let mut exp = exp;
        if exp > 10 {
            f *= FLOAT32POW10[(exp - 10) as usize];
            exp = 10;
        }
        if !(-1e7..=1e7).contains(&f) {
            return None;
        }
        return Some(f * FLOAT32POW10[exp as usize]);
    }
    if (-10..0).contains(&exp) {
        return Some(f / FLOAT32POW10[(-exp) as usize]);
    }
    None
}

fn atof_hex(
    flt: &FloatInfo,
    mantissa: u64,
    exp: isize,
    neg: bool,
    trunc: bool,
) -> (f64, Option<ErrorKind>) {
    let max_exp = (1isize << flt.expbits) + flt.bias - 2;
    let min_exp = flt.bias + 1;
    let mut exp = exp + flt.mantbits as isize;
    let mut mantissa = mantissa;

    while mantissa != 0 && (mantissa >> (flt.mantbits + 2)) == 0 {
        mantissa <<= 1;
        exp -= 1;
    }
    if trunc {
        mantissa |= 1;
    }
    while (mantissa >> (1 + flt.mantbits + 2)) != 0 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exp += 1;
    }

    while mantissa > 1 && exp < min_exp - 2 {
        mantissa = (mantissa >> 1) | (mantissa & 1);
        exp += 1;
    }

    let mut round = mantissa & 3;
    mantissa >>= 2;
    round |= mantissa & 1;
    exp += 2;
    if round == 3 {
        mantissa += 1;
        if mantissa == 1u64 << (1 + flt.mantbits) {
            mantissa >>= 1;
            exp += 1;
        }
    }

    if (mantissa >> flt.mantbits) == 0 {
        exp = flt.bias;
    }
    let mut err = None;
    if exp > max_exp {
        mantissa = 1u64 << flt.mantbits;
        exp = max_exp + 1;
        err = Some(ErrorKind::Range);
    }

    let mut bits = mantissa & ((1u64 << flt.mantbits) - 1);
    bits |= (((exp - flt.bias) & ((1isize << flt.expbits) - 1)) as u64) << flt.mantbits;
    if neg {
        bits |= (1u64 << flt.mantbits) << flt.expbits;
    }
    if flt.mantbits == FLOAT32INFO.mantbits {
        return (f64::from(f32::from_bits(bits as u32)), err);
    }
    (f64::from_bits(bits), err)
}

fn atof32(s: &[u8]) -> (f32, usize, Option<ErrorKind>) {
    if let Some((val, n)) = special(s) {
        let f = if val.is_nan() {
            f32::from_bits(GO_NAN32)
        } else {
            val as f32
        };
        return (f, n, None);
    }

    let r = read_float(s);
    let n = r.n;
    if !r.ok {
        return (0.0, n, Some(ErrorKind::Syntax));
    }

    if r.hex {
        let (f, err) = atof_hex(&FLOAT32INFO, r.mantissa, r.exp, r.neg, r.trunc);
        return (f as f32, n, err);
    }

    if !r.trunc
        && let Some(f) = atof32exact(r.mantissa, r.exp, r.neg)
    {
        return (f, n, None);
    }
    if let Some(f) = eisel_lemire32(r.mantissa, r.exp, r.neg) {
        if !r.trunc {
            return (f, n, None);
        }
        if eisel_lemire32(r.mantissa + 1, r.exp, r.neg) == Some(f) {
            return (f, n, None);
        }
    }

    let mut d = Decimal::new();
    if !d.set(&s[..n]) {
        return (0.0, n, Some(ErrorKind::Syntax));
    }
    let (b, ovf) = d.float_bits(&FLOAT32INFO);
    let f = f32::from_bits(b as u32);
    (f, n, ovf.then_some(ErrorKind::Range))
}

fn atof64(s: &[u8]) -> (f64, usize, Option<ErrorKind>) {
    if let Some((val, n)) = special(s) {
        return (val, n, None);
    }

    let r = read_float(s);
    let n = r.n;
    if !r.ok {
        return (0.0, n, Some(ErrorKind::Syntax));
    }

    if r.hex {
        let (f, err) = atof_hex(&FLOAT64INFO, r.mantissa, r.exp, r.neg, r.trunc);
        return (f, n, err);
    }

    if !r.trunc
        && let Some(f) = atof64exact(r.mantissa, r.exp, r.neg)
    {
        return (f, n, None);
    }
    if let Some(f) = eisel_lemire64(r.mantissa, r.exp, r.neg) {
        if !r.trunc {
            return (f, n, None);
        }
        if eisel_lemire64(r.mantissa + 1, r.exp, r.neg) == Some(f) {
            return (f, n, None);
        }
    }

    let mut d = Decimal::new();
    if !d.set(&s[..n]) {
        return (0.0, n, Some(ErrorKind::Syntax));
    }
    let (b, ovf) = d.float_bits(&FLOAT64INFO);
    let f = f64::from_bits(b);
    (f, n, ovf.then_some(ErrorKind::Range))
}

pub fn parse_float(s: &[u8], bit_size: u32) -> Result<f64, NumError<f64>> {
    let (f, n, err) = parse_float_prefix(s, bit_size);
    if n != s.len() && err != Some(ErrorKind::Syntax) {
        return Err(NumError::new(ErrorKind::Syntax, Target::Number, s, 0.0));
    }
    match err {
        None => Ok(f),
        Some(kind) => Err(NumError::new(kind, Target::Number, s, f)),
    }
}

fn parse_float_prefix(s: &[u8], bit_size: u32) -> (f64, usize, Option<ErrorKind>) {
    if bit_size == 32 {
        let (f, n, err) = atof32(s);
        return (f64::from(f), n, err);
    }
    atof64(s)
}
