use super::format::{
    STD_FRAC_SECOND9, append_int, append_nano, parse_nanoseconds, std_frac_second,
};
use super::time::{Month, Time, abs_clock, abs_date, abs_days, days_in};
use super::zoneinfo::Location;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StrictRfc3339Error {
    YearOutOfRange,
    ZoneHourOutOfRange,
}

impl fmt::Display for StrictRfc3339Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StrictRfc3339Error::YearOutOfRange => {
                f.write_str("the year is outside the range 0 to 9999")
            }
            StrictRfc3339Error::ZoneHourOutOfRange => {
                f.write_str("the time zone offset hour is outside the range 0 to 23")
            }
        }
    }
}

impl std::error::Error for StrictRfc3339Error {}

fn num2(b: &[u8]) -> u8 {
    b[0].wrapping_sub(b'0')
        .wrapping_mul(10)
        .wrapping_add(b[1].wrapping_sub(b'0'))
}

impl Time {
    pub(crate) fn append_format_rfc3339(&self, b: &mut Vec<u8>, nanos: bool) {
        let (_, offset, abs) = self.locabs();
        let (year, month, day) = abs_date(abs_days(abs));
        append_int(b, year, 4);
        b.push(b'-');
        append_int(b, month.0, 2);
        b.push(b'-');
        append_int(b, day, 2);
        b.push(b'T');
        let (hour, min, sec) = abs_clock(abs);
        append_int(b, hour, 2);
        b.push(b':');
        append_int(b, min, 2);
        b.push(b':');
        append_int(b, sec, 2);
        if nanos {
            let std = std_frac_second(STD_FRAC_SECOND9, 9, b'.');
            append_nano(b, self.nanosecond(), std);
        }
        if offset == 0 {
            b.push(b'Z');
            return;
        }
        let mut zone = offset / 60;
        if zone < 0 {
            b.push(b'-');
            zone = -zone;
        } else {
            b.push(b'+');
        }
        append_int(b, i64::from(zone / 60), 2);
        b.push(b':');
        append_int(b, i64::from(zone % 60), 2);
    }

    pub fn append_strict_rfc3339(&self, b: &mut Vec<u8>) -> Result<(), StrictRfc3339Error> {
        let n0 = b.len();
        self.append_format_rfc3339(b, true);
        if b[n0 + 4] != b'-' {
            return Err(StrictRfc3339Error::YearOutOfRange);
        }
        let n = b.len();
        if b[n - 1] != b'Z' {
            let c = b[n - 6];
            if c.is_ascii_digit() || num2(&b[n - 5..]) >= 24 {
                return Err(StrictRfc3339Error::ZoneHourOutOfRange);
            }
        }
        Ok(())
    }
}

fn parse_uint(s: &[u8], min: i64, max: i64, ok: &mut bool) -> i64 {
    let mut x: i64 = 0;
    for &c in s {
        if !c.is_ascii_digit() {
            *ok = false;
            return min;
        }
        x = x * 10 + i64::from(c - b'0');
    }
    if !(min..=max).contains(&x) {
        *ok = false;
        return min;
    }
    x
}

pub fn parse_strict_rfc3339(s: &[u8]) -> Option<Time> {
    parse_rfc3339(s, &Location::local())
}

pub(crate) fn parse_rfc3339(s: &[u8], local: &Location) -> Option<Time> {
    let mut ok = true;
    if s.len() < 19 {
        return None;
    }
    let year = parse_uint(&s[0..4], 0, 9999, &mut ok);
    let month = parse_uint(&s[5..7], 1, 12, &mut ok);
    let day = parse_uint(&s[8..10], 1, days_in(Month(month), year), &mut ok);
    let hour = parse_uint(&s[11..13], 0, 23, &mut ok);
    let min = parse_uint(&s[14..16], 0, 59, &mut ok);
    let sec = parse_uint(&s[17..19], 0, 59, &mut ok);
    if !ok || s[4] != b'-' || s[7] != b'-' || s[10] != b'T' || s[13] != b':' || s[16] != b':' {
        return None;
    }
    let mut s = &s[19..];
    let mut nsec = 0;
    if s.len() >= 2 && s[0] == b'.' && s[1].is_ascii_digit() {
        let mut n = 2;
        while n < s.len() && s[n].is_ascii_digit() {
            n += 1;
        }
        nsec = parse_nanoseconds(s, n).map_or(0, |(ns, _)| ns);
        s = &s[n..];
    }
    let mut t = Time::from_date(
        year,
        Month(month),
        day,
        hour,
        min,
        sec,
        nsec,
        &Location::utc(),
    );
    if s.len() != 1 || s[0] != b'Z' {
        if s.len() != 6 {
            return None;
        }
        let hr = parse_uint(&s[1..3], 0, 23, &mut ok);
        let mm = parse_uint(&s[4..6], 0, 59, &mut ok);
        if !ok || (s[0] != b'-' && s[0] != b'+') || s[3] != b':' {
            return None;
        }
        let mut zone_offset = (hr * 60 + mm) * 60;
        if s[0] == b'-' {
            zone_offset = -zone_offset;
        }
        t.add_sec(-zone_offset);
        if i64::from(local.lookup(t.unix_sec()).offset) == zone_offset {
            t.set_loc(local);
        } else {
            t.set_loc(&Location::fixed("", zone_offset as i32));
        }
    }
    Some(t)
}
