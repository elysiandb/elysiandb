use crate::gocompat::json::quote_text;
use std::fmt;

pub(super) fn lower(c: u8) -> u8 {
    c | (b'x' - b'X')
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    Syntax,
    Range,
    InvalidBase(u32),
    InvalidBitSize(u32),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Target {
    Integer,
    Number,
    Boolean,
}

#[derive(Clone, Debug, PartialEq)]
pub struct NumError<T> {
    pub kind: ErrorKind,
    pub target: Target,
    pub num: Vec<u8>,
    pub value: T,
}

impl<T> NumError<T> {
    pub(super) fn new(kind: ErrorKind, target: Target, num: &[u8], value: T) -> NumError<T> {
        NumError {
            kind,
            target,
            num: num.to_vec(),
            value,
        }
    }
}

impl<T> fmt::Display for NumError<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let num = quote_text(&self.num);
        match self.kind {
            ErrorKind::Syntax => match self.target {
                Target::Integer => write!(f, "{num} is not a valid integer"),
                Target::Number => write!(f, "{num} is not a valid number"),
                Target::Boolean => write!(f, "{num} is not a valid boolean"),
            },
            ErrorKind::Range => write!(f, "{num} is out of range"),
            ErrorKind::InvalidBase(base) => write!(f, "{num} cannot be read in base {base}"),
            ErrorKind::InvalidBitSize(size) => {
                write!(f, "{num} cannot be read with a bit size of {size}")
            }
        }
    }
}

impl<T: fmt::Debug> std::error::Error for NumError<T> {}

fn parse_uint_raw(s: &[u8], base: u32, bit_size: u32) -> (u64, Option<ErrorKind>) {
    if s.is_empty() {
        return (0, Some(ErrorKind::Syntax));
    }

    let base0 = base == 0;

    let s0 = s;
    let mut s = s;
    let mut base = base;
    match base {
        2..=36 => {}
        0 => {
            base = 10;
            if s[0] == b'0' {
                if s.len() >= 3 && lower(s[1]) == b'b' {
                    base = 2;
                    s = &s[2..];
                } else if s.len() >= 3 && lower(s[1]) == b'o' {
                    base = 8;
                    s = &s[2..];
                } else if s.len() >= 3 && lower(s[1]) == b'x' {
                    base = 16;
                    s = &s[2..];
                } else {
                    base = 8;
                    s = &s[1..];
                }
            }
        }
        _ => return (0, Some(ErrorKind::InvalidBase(base))),
    }

    let bit_size = match bit_size {
        0 => 64,
        1..=64 => bit_size,
        _ => return (0, Some(ErrorKind::InvalidBitSize(bit_size))),
    };

    let cutoff = u64::MAX / u64::from(base) + 1;

    let max_val = if bit_size == 64 {
        u64::MAX
    } else {
        (1u64 << bit_size) - 1
    };

    let mut underscores = false;
    let mut n: u64 = 0;
    for &c in s {
        let d = if c == b'_' && base0 {
            underscores = true;
            continue;
        } else if c.is_ascii_digit() {
            c - b'0'
        } else if lower(c).is_ascii_lowercase() {
            lower(c) - b'a' + 10
        } else {
            return (0, Some(ErrorKind::Syntax));
        };

        if u32::from(d) >= base {
            return (0, Some(ErrorKind::Syntax));
        }

        if n >= cutoff {
            return (max_val, Some(ErrorKind::Range));
        }
        n *= u64::from(base);

        let n1 = n.wrapping_add(u64::from(d));
        if n1 < n || n1 > max_val {
            return (max_val, Some(ErrorKind::Range));
        }
        n = n1;
    }

    if underscores && !underscore_ok(s0) {
        return (0, Some(ErrorKind::Syntax));
    }

    (n, None)
}

pub fn parse_uint(s: &[u8], base: u32, bit_size: u32) -> Result<u64, NumError<u64>> {
    match parse_uint_raw(s, base, bit_size) {
        (n, None) => Ok(n),
        (n, Some(kind)) => Err(NumError::new(kind, Target::Integer, s, n)),
    }
}

fn parse_int_raw(s: &[u8], base: u32, bit_size: u32) -> (i64, Option<ErrorKind>) {
    if s.is_empty() {
        return (0, Some(ErrorKind::Syntax));
    }

    let mut t = s;
    let mut neg = false;
    if s[0] == b'+' {
        t = &s[1..];
    } else if s[0] == b'-' {
        neg = true;
        t = &s[1..];
    }

    let (un, err) = parse_uint_raw(t, base, bit_size);
    if let Some(kind) = err.filter(|&k| k != ErrorKind::Range) {
        return (0, Some(kind));
    }

    let bit_size = if bit_size == 0 { 64 } else { bit_size };

    let cutoff = 1u64 << (bit_size - 1);
    if !neg && un >= cutoff {
        return ((cutoff - 1) as i64, Some(ErrorKind::Range));
    }
    if neg && un > cutoff {
        return ((cutoff as i64).wrapping_neg(), Some(ErrorKind::Range));
    }
    let mut n = un as i64;
    if neg {
        n = n.wrapping_neg();
    }
    (n, None)
}

pub fn parse_int(s: &[u8], base: u32, bit_size: u32) -> Result<i64, NumError<i64>> {
    match parse_int_raw(s, base, bit_size) {
        (n, None) => Ok(n),
        (n, Some(kind)) => Err(NumError::new(kind, Target::Integer, s, n)),
    }
}

pub fn atoi(s: &[u8]) -> Result<i64, NumError<i64>> {
    if (1..19).contains(&s.len()) {
        let mut t = s;
        if s[0] == b'-' || s[0] == b'+' {
            t = &s[1..];
            if t.is_empty() {
                return Err(NumError::new(ErrorKind::Syntax, Target::Integer, s, 0));
            }
        }

        let mut n: i64 = 0;
        for &ch in t {
            let ch = ch.wrapping_sub(b'0');
            if ch > 9 {
                return Err(NumError::new(ErrorKind::Syntax, Target::Integer, s, 0));
            }
            n = n * 10 + i64::from(ch);
        }
        if s[0] == b'-' {
            n = -n;
        }
        return Ok(n);
    }

    parse_int(s, 10, 0)
}

pub(super) fn underscore_ok(s: &[u8]) -> bool {
    let mut saw = b'^';
    let mut i = 0;

    let mut s = s;
    if !s.is_empty() && (s[0] == b'-' || s[0] == b'+') {
        s = &s[1..];
    }

    let mut hex = false;
    if s.len() >= 2
        && s[0] == b'0'
        && (lower(s[1]) == b'b' || lower(s[1]) == b'o' || lower(s[1]) == b'x')
    {
        i = 2;
        saw = b'0';
        hex = lower(s[1]) == b'x';
    }

    while i < s.len() {
        if s[i].is_ascii_digit() || (hex && (b'a'..=b'f').contains(&lower(s[i]))) {
            saw = b'0';
            i += 1;
            continue;
        }
        if s[i] == b'_' {
            if saw != b'0' {
                return false;
            }
            saw = b'_';
            i += 1;
            continue;
        }
        if saw == b'_' {
            return false;
        }
        saw = b'!';
        i += 1;
    }
    saw != b'_'
}
