use super::format::leading_int;
use crate::gocompat::json::quote_text;
use std::fmt;
use std::ops::{Add, Mul, Neg, Sub};

const NANOSECOND: i64 = 1;
const MICROSECOND: i64 = 1_000;
const MILLISECOND: i64 = 1_000 * MICROSECOND;
const SECOND: i64 = 1_000 * MILLISECOND;
const MINUTE: i64 = 60 * SECOND;
const HOUR: i64 = 60 * MINUTE;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration(pub i64);

impl Duration {
    pub const NANOSECOND: Duration = Duration(NANOSECOND);
    pub const MICROSECOND: Duration = Duration(MICROSECOND);
    pub const MILLISECOND: Duration = Duration(MILLISECOND);
    pub const SECOND: Duration = Duration(SECOND);
    pub const MINUTE: Duration = Duration(MINUTE);
    pub const HOUR: Duration = Duration(HOUR);
    pub const MIN: Duration = Duration(i64::MIN);
    pub const MAX: Duration = Duration(i64::MAX);

    pub const fn nanoseconds(self) -> i64 {
        self.0
    }

    pub const fn microseconds(self) -> i64 {
        self.0 / MICROSECOND
    }

    pub const fn milliseconds(self) -> i64 {
        self.0 / MILLISECOND
    }

    pub fn seconds(self) -> f64 {
        let sec = self.0 / SECOND;
        let nsec = self.0 % SECOND;
        sec as f64 + nsec as f64 / 1e9
    }

    pub fn minutes(self) -> f64 {
        let min = self.0 / MINUTE;
        let nsec = self.0 % MINUTE;
        min as f64 + nsec as f64 / (60.0 * 1e9)
    }

    pub fn hours(self) -> f64 {
        let hour = self.0 / HOUR;
        let nsec = self.0 % HOUR;
        hour as f64 + nsec as f64 / (60.0 * 60.0 * 1e9)
    }

    pub fn truncate(self, m: Duration) -> Duration {
        if m.0 <= 0 {
            return self;
        }
        Duration(self.0 - self.0 % m.0)
    }

    pub fn round(self, m: Duration) -> Duration {
        if m.0 <= 0 {
            return self;
        }
        let d = self.0;
        let m = m.0;
        let mut r = d % m;
        if d < 0 {
            r = -r;
            if less_than_half(r, m) {
                return Duration(d + r);
            }
            let d1 = d.wrapping_sub(m).wrapping_add(r);
            if d1 < d {
                return Duration(d1);
            }
            return Duration::MIN;
        }
        if less_than_half(r, m) {
            return Duration(d - r);
        }
        let d1 = d.wrapping_add(m).wrapping_sub(r);
        if d1 > d {
            return Duration(d1);
        }
        Duration::MAX
    }

    pub fn abs(self) -> Duration {
        if self.0 >= 0 {
            self
        } else if self.0 == i64::MIN {
            Duration::MAX
        } else {
            Duration(-self.0)
        }
    }

    pub fn append_to(self, b: &mut Vec<u8>) {
        let mut buf = [0u8; 32];
        let w = self.format(&mut buf);
        b.extend_from_slice(&buf[w..]);
    }

    fn format(self, buf: &mut [u8; 32]) -> usize {
        let mut w = buf.len();
        let mut u = self.0 as u64;
        let neg = self.0 < 0;
        if neg {
            u = u.wrapping_neg();
        }
        if u < SECOND as u64 {
            let prec;
            w -= 1;
            buf[w] = b's';
            w -= 1;
            if u == 0 {
                buf[w] = b'0';
                return w;
            } else if u < MICROSECOND as u64 {
                prec = 0;
                buf[w] = b'n';
            } else if u < MILLISECOND as u64 {
                prec = 3;
                w -= 1;
                buf[w..w + 2].copy_from_slice("\u{b5}".as_bytes());
            } else {
                prec = 6;
                buf[w] = b'm';
            }
            (w, u) = fmt_frac(&mut buf[..w], u, prec);
            w = fmt_int(&mut buf[..w], u);
        } else {
            w -= 1;
            buf[w] = b's';
            (w, u) = fmt_frac(&mut buf[..w], u, 9);
            w = fmt_int(&mut buf[..w], u % 60);
            u /= 60;
            if u > 0 {
                w -= 1;
                buf[w] = b'm';
                w = fmt_int(&mut buf[..w], u % 60);
                u /= 60;
                if u > 0 {
                    w -= 1;
                    buf[w] = b'h';
                    w = fmt_int(&mut buf[..w], u);
                }
            }
        }
        if neg {
            w -= 1;
            buf[w] = b'-';
        }
        w
    }
}

fn fmt_frac(buf: &mut [u8], v: u64, prec: usize) -> (usize, u64) {
    let mut w = buf.len();
    let mut v = v;
    let mut print = false;
    for _ in 0..prec {
        let digit = v % 10;
        print = print || digit != 0;
        if print {
            w -= 1;
            buf[w] = digit as u8 + b'0';
        }
        v /= 10;
    }
    if print {
        w -= 1;
        buf[w] = b'.';
    }
    (w, v)
}

fn fmt_int(buf: &mut [u8], v: u64) -> usize {
    let mut w = buf.len();
    let mut v = v;
    if v == 0 {
        w -= 1;
        buf[w] = b'0';
    } else {
        while v > 0 {
            w -= 1;
            buf[w] = (v % 10) as u8 + b'0';
            v /= 10;
        }
    }
    w
}

pub(crate) fn less_than_half(x: i64, y: i64) -> bool {
    (x as u64).wrapping_add(x as u64) < y as u64
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut buf = [0u8; 32];
        let w = self.format(&mut buf);
        f.write_str(&String::from_utf8_lossy(&buf[w..]))
    }
}

impl Add for Duration {
    type Output = Duration;

    fn add(self, rhs: Duration) -> Duration {
        Duration(self.0.wrapping_add(rhs.0))
    }
}

impl Sub for Duration {
    type Output = Duration;

    fn sub(self, rhs: Duration) -> Duration {
        Duration(self.0.wrapping_sub(rhs.0))
    }
}

impl Neg for Duration {
    type Output = Duration;

    fn neg(self) -> Duration {
        Duration(self.0.wrapping_neg())
    }
}

impl Mul<i64> for Duration {
    type Output = Duration;

    fn mul(self, rhs: i64) -> Duration {
        Duration(self.0.wrapping_mul(rhs))
    }
}

impl Mul<Duration> for i64 {
    type Output = Duration;

    fn mul(self, rhs: Duration) -> Duration {
        Duration(self.wrapping_mul(rhs.0))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DurationError {
    pub input: Vec<u8>,
}

impl fmt::Display for DurationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} is not a valid duration", quote_text(&self.input))
    }
}

impl std::error::Error for DurationError {}

fn leading_fraction(s: &[u8]) -> (u64, f64, &[u8]) {
    let mut i = 0;
    let mut x: u64 = 0;
    let mut scale = 1.0;
    let mut overflow = false;
    while i < s.len() {
        let c = s[i];
        if !c.is_ascii_digit() {
            break;
        }
        i += 1;
        if overflow {
            continue;
        }
        if x > ((1u64 << 63) - 1) / 10 {
            overflow = true;
            continue;
        }
        let y = x * 10 + u64::from(c - b'0');
        if y > 1u64 << 63 {
            overflow = true;
            continue;
        }
        x = y;
        scale *= 10.0;
    }
    (x, scale, &s[i..])
}

fn unit_of(u: &[u8]) -> Option<u64> {
    match u {
        b"ns" => Some(NANOSECOND as u64),
        b"us" | b"\xc2\xb5s" | b"\xce\xbcs" => Some(MICROSECOND as u64),
        b"ms" => Some(MILLISECOND as u64),
        b"s" => Some(SECOND as u64),
        b"m" => Some(MINUTE as u64),
        b"h" => Some(HOUR as u64),
        _ => None,
    }
}

pub fn parse_duration(s: impl AsRef<[u8]>) -> Result<Duration, DurationError> {
    let orig = s.as_ref();
    let invalid = || DurationError {
        input: orig.to_vec(),
    };
    let mut s = orig;
    let mut d: u64 = 0;
    let mut neg = false;
    if let Some(&c) = s.first()
        && (c == b'-' || c == b'+')
    {
        neg = c == b'-';
        s = &s[1..];
    }
    if s == b"0" {
        return Ok(Duration(0));
    }
    if s.is_empty() {
        return Err(invalid());
    }
    while !s.is_empty() {
        let mut f: u64 = 0;
        let mut scale: f64 = 1.0;
        if s[0] != b'.' && !s[0].is_ascii_digit() {
            return Err(invalid());
        }
        let pl = s.len();
        let (mut v, rest) = leading_int(s).ok_or_else(invalid)?;
        s = rest;
        let pre = pl != s.len();
        let mut post = false;
        if !s.is_empty() && s[0] == b'.' {
            s = &s[1..];
            let pl = s.len();
            let (ff, sc, rest) = leading_fraction(s);
            f = ff;
            scale = sc;
            s = rest;
            post = pl != s.len();
        }
        if !pre && !post {
            return Err(invalid());
        }
        let mut i = 0;
        while i < s.len() {
            let c = s[i];
            if c == b'.' || c.is_ascii_digit() {
                break;
            }
            i += 1;
        }
        if i == 0 {
            return Err(invalid());
        }
        let u = &s[..i];
        s = &s[i..];
        let unit = unit_of(u).ok_or_else(invalid)?;
        if v > (1u64 << 63) / unit {
            return Err(invalid());
        }
        v *= unit;
        if f > 0 {
            v += (f as f64 * (unit as f64 / scale)) as u64;
            if v > 1u64 << 63 {
                return Err(invalid());
            }
        }
        d = d.wrapping_add(v);
        if d > 1u64 << 63 {
            return Err(invalid());
        }
    }
    if neg {
        return Ok(Duration((d as i64).wrapping_neg()));
    }
    if d > (1u64 << 63) - 1 {
        return Err(invalid());
    }
    Ok(Duration(d as i64))
}
