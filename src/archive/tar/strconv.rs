use super::common::{PAX_GNAME, PAX_LINKPATH, PAX_PATH, PAX_UNAME};
use gocompat::strconv::{format_int, parse_int, parse_uint};
use gocompat::time::Time;

pub(super) fn has_nul(s: &[u8]) -> bool {
    s.contains(&0)
}

pub(super) fn is_ascii(s: &[u8]) -> bool {
    s.iter().all(|&c| c < 0x80 && c != 0)
}

pub(super) fn to_ascii(s: &[u8]) -> Vec<u8> {
    if is_ascii(s) {
        return s.to_vec();
    }
    s.iter().copied().filter(|&c| c < 0x80 && c != 0).collect()
}

#[derive(Default)]
pub(super) struct Parser {
    pub(super) err: bool,
}

#[derive(Default)]
pub(super) struct Formatter {
    pub(super) err: bool,
}

pub(super) fn parse_string(b: &[u8]) -> Vec<u8> {
    match b.iter().position(|&c| c == 0) {
        Some(i) => b[..i].to_vec(),
        None => b.to_vec(),
    }
}

fn trim_space_nul(b: &[u8]) -> &[u8] {
    let is_cut = |c: &u8| *c == b' ' || *c == 0;
    let start = b.iter().position(|c| !is_cut(c)).unwrap_or(b.len());
    let end = b.iter().rposition(|c| !is_cut(c)).map_or(start, |i| i + 1);
    &b[start..end.max(start)]
}

impl Formatter {
    pub(super) fn format_string(&mut self, b: &mut [u8], s: &[u8]) {
        if s.len() > b.len() {
            self.err = true;
        }
        let n = s.len().min(b.len());
        b[..n].copy_from_slice(&s[..n]);
        if s.len() < b.len() {
            b[s.len()] = 0;
        }

        if s.len() > b.len() && b[b.len() - 1] == b'/' {
            let head = &s[..b.len() - 1];
            let mut n = head.len();
            while n > 0 && head[n - 1] == b'/' {
                n -= 1;
            }
            b[n] = 0;
        }
    }

    pub(super) fn format_numeric(&mut self, b: &mut [u8], x: i64) {
        if fits_in_octal(b.len(), x) {
            self.format_octal(b, x);
            return;
        }

        if fits_in_base256(b.len(), x) {
            let mut x = x;
            for i in (0..b.len()).rev() {
                b[i] = x as u8;
                x >>= 8;
            }
            b[0] |= 0x80;
            return;
        }

        self.format_octal(b, 0);
        self.err = true;
    }

    pub(super) fn format_octal(&mut self, b: &mut [u8], x: i64) {
        let mut x = x;
        if !fits_in_octal(b.len(), x) {
            x = 0;
            self.err = true;
        }

        let mut s = format_int(x, 8).into_bytes();
        let width = b.len() as i64 - s.len() as i64 - 1;
        if width > 0 {
            let mut padded = vec![b'0'; width as usize];
            padded.extend_from_slice(&s);
            s = padded;
        }
        self.format_string(b, &s);
    }
}

pub(super) fn fits_in_base256(n: usize, x: i64) -> bool {
    let bin_bits = (n as u32).wrapping_sub(1).wrapping_mul(8);
    let limit = 1i64.checked_shl(bin_bits).unwrap_or(0);
    n >= 9 || (x >= limit.wrapping_neg() && x < limit)
}

impl Parser {
    pub(super) fn parse_numeric(&mut self, b: &[u8]) -> i64 {
        if !b.is_empty() && b[0] & 0x80 != 0 {
            let inv: u8 = if b[0] & 0x40 != 0 { 0xff } else { 0 };

            let mut x: u64 = 0;
            for (i, &c) in b.iter().enumerate() {
                let mut c = c ^ inv;
                if i == 0 {
                    c &= 0x7f;
                }
                if (x >> 56) > 0 {
                    self.err = true;
                    return 0;
                }
                x = (x << 8) | u64::from(c);
            }
            if (x >> 63) > 0 {
                self.err = true;
                return 0;
            }
            if inv == 0xff {
                return !(x as i64);
            }
            return x as i64;
        }

        self.parse_octal(b)
    }

    pub(super) fn parse_octal(&mut self, b: &[u8]) -> i64 {
        let b = trim_space_nul(b);

        if b.is_empty() {
            return 0;
        }
        match parse_uint(&parse_string(b), 8, 64) {
            Ok(x) => x as i64,
            Err(e) => {
                self.err = true;
                e.value as i64
            }
        }
    }
}

pub(super) fn fits_in_octal(n: usize, x: i64) -> bool {
    let oct_bits = (n as u32).wrapping_sub(1).wrapping_mul(3);
    x >= 0 && (n >= 22 || x < 1i64.checked_shl(oct_bits).unwrap_or(0))
}

pub(super) fn parse_pax_time(s: &[u8]) -> Result<Time, ()> {
    const MAX_NANO_SECOND_DIGITS: usize = 9;

    let (ss, sn) = match s.iter().position(|&c| c == b'.') {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, &s[s.len()..]),
    };

    let secs = parse_int(ss, 10, 64).map_err(|_| ())?;
    if sn.is_empty() {
        return Ok(Time::from_unix(secs, 0));
    }

    if !sn.iter().all(u8::is_ascii_digit) {
        return Err(());
    }
    let mut digits = sn.to_vec();
    if digits.len() < MAX_NANO_SECOND_DIGITS {
        digits.resize(MAX_NANO_SECOND_DIGITS, b'0');
    } else {
        digits.truncate(MAX_NANO_SECOND_DIGITS);
    }
    let nsecs = parse_int(&digits, 10, 64).unwrap_or(0);
    if !ss.is_empty() && ss[0] == b'-' {
        return Ok(Time::from_unix(secs, -nsecs));
    }
    Ok(Time::from_unix(secs, nsecs))
}

pub(super) fn format_pax_time(ts: &Time) -> Vec<u8> {
    let (mut secs, mut nsecs) = (ts.unix(), ts.nanosecond());
    if nsecs == 0 {
        return format_int(secs, 10).into_bytes();
    }

    let mut sign = "";
    if secs < 0 {
        sign = "-";
        secs = -(secs + 1);
        nsecs = -(nsecs - 1_000_000_000);
    }
    let s = format!("{sign}{secs}.{nsecs:09}");
    s.trim_end_matches('0').as_bytes().to_vec()
}

pub(super) type PaxRecord<'a> = (Vec<u8>, Vec<u8>, &'a [u8]);

pub(super) fn parse_pax_record(s: &[u8]) -> Result<PaxRecord<'_>, ()> {
    let sp = s.iter().position(|&c| c == b' ').ok_or(())?;
    let (n_str, rest) = (&s[..sp], &s[sp + 1..]);

    let n = match parse_int(n_str, 10, 0) {
        Ok(n) if n >= 5 && n <= s.len() as i64 => n,
        _ => return Err(()),
    };
    let n = n - (n_str.len() as i64 + 1);
    if n <= 0 {
        return Err(());
    }
    let n = n as usize;

    let (rec, nl, rem) = (&rest[..n - 1], &rest[n - 1..n], &rest[n..]);
    if nl != b"\n" {
        return Err(());
    }

    let eq = rec.iter().position(|&c| c == b'=').ok_or(())?;
    let (k, v) = (&rec[..eq], &rec[eq + 1..]);

    if !valid_pax_record(k, v) {
        return Err(());
    }
    Ok((k.to_vec(), v.to_vec(), rem))
}

pub(super) fn format_pax_record(k: &[u8], v: &[u8]) -> Result<Vec<u8>, ()> {
    if !valid_pax_record(k, v) {
        return Err(());
    }

    const PADDING: usize = 3;
    let mut size = k.len() + v.len() + PADDING;
    size += size.to_string().len();
    let build = |size: usize| {
        let mut record = size.to_string().into_bytes();
        record.push(b' ');
        record.extend_from_slice(k);
        record.push(b'=');
        record.extend_from_slice(v);
        record.push(b'\n');
        record
    };
    let mut record = build(size);

    if record.len() != size {
        size = record.len();
        record = build(size);
    }
    Ok(record)
}

pub(super) fn valid_pax_record(k: &[u8], v: &[u8]) -> bool {
    if k.is_empty() || k.contains(&b'=') {
        return false;
    }
    match k {
        PAX_PATH | PAX_LINKPATH | PAX_UNAME | PAX_GNAME => !has_nul(v),
        _ => !has_nul(k),
    }
}
