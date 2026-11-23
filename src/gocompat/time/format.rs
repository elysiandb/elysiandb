use super::format_rfc3339::parse_rfc3339;
use super::time::{
    Month, SECONDS_PER_DAY, Time, abs_clock, abs_date, abs_weekday, abs_year_yday, days_before,
    days_in, is_leap,
};
use super::zoneinfo::Location;
use crate::gocompat::json::quote_text;
use std::fmt;

pub const LAYOUT: &str = "01/02 03:04:05PM '06 -0700";
pub const ANSIC: &str = "Mon Jan _2 15:04:05 2006";
pub const UNIX_DATE: &str = "Mon Jan _2 15:04:05 MST 2006";
pub const RUBY_DATE: &str = "Mon Jan 02 15:04:05 -0700 2006";
pub const RFC822: &str = "02 Jan 06 15:04 MST";
pub const RFC822Z: &str = "02 Jan 06 15:04 -0700";
pub const RFC850: &str = "Monday, 02-Jan-06 15:04:05 MST";
pub const RFC1123: &str = "Mon, 02 Jan 2006 15:04:05 MST";
pub const RFC1123Z: &str = "Mon, 02 Jan 2006 15:04:05 -0700";
pub const RFC3339: &str = "2006-01-02T15:04:05Z07:00";
pub const RFC3339_NANO: &str = "2006-01-02T15:04:05.999999999Z07:00";
pub const KITCHEN: &str = "3:04PM";
pub const STAMP: &str = "Jan _2 15:04:05";
pub const STAMP_MILLI: &str = "Jan _2 15:04:05.000";
pub const STAMP_MICRO: &str = "Jan _2 15:04:05.000000";
pub const STAMP_NANO: &str = "Jan _2 15:04:05.000000000";
pub const DATE_TIME: &str = "2006-01-02 15:04:05";
pub const DATE_ONLY: &str = "2006-01-02";
pub const TIME_ONLY: &str = "15:04:05";

const STD_NEED_DATE: i32 = 1 << 8;
const STD_NEED_YDAY: i32 = 1 << 9;
const STD_NEED_CLOCK: i32 = 1 << 10;
const STD_ARG_SHIFT: i32 = 16;
const STD_SEPARATOR_SHIFT: i32 = 28;
const STD_MASK: i32 = (1 << STD_ARG_SHIFT) - 1;

const STD_LONG_MONTH: i32 = 1 + STD_NEED_DATE;
const STD_MONTH: i32 = 2 + STD_NEED_DATE;
const STD_NUM_MONTH: i32 = 3 + STD_NEED_DATE;
const STD_ZERO_MONTH: i32 = 4 + STD_NEED_DATE;
const STD_LONG_WEEK_DAY: i32 = 5 + STD_NEED_DATE;
const STD_WEEK_DAY: i32 = 6 + STD_NEED_DATE;
const STD_DAY: i32 = 7 + STD_NEED_DATE;
const STD_UNDER_DAY: i32 = 8 + STD_NEED_DATE;
const STD_ZERO_DAY: i32 = 9 + STD_NEED_DATE;
const STD_UNDER_YEAR_DAY: i32 = 10 + STD_NEED_YDAY;
const STD_ZERO_YEAR_DAY: i32 = 11 + STD_NEED_YDAY;
const STD_HOUR: i32 = 12 + STD_NEED_CLOCK;
const STD_HOUR12: i32 = 13 + STD_NEED_CLOCK;
const STD_ZERO_HOUR12: i32 = 14 + STD_NEED_CLOCK;
const STD_MINUTE: i32 = 15 + STD_NEED_CLOCK;
const STD_ZERO_MINUTE: i32 = 16 + STD_NEED_CLOCK;
const STD_SECOND: i32 = 17 + STD_NEED_CLOCK;
const STD_ZERO_SECOND: i32 = 18 + STD_NEED_CLOCK;
const STD_LONG_YEAR: i32 = 19 + STD_NEED_DATE;
const STD_YEAR: i32 = 20 + STD_NEED_DATE;
const STD_PM: i32 = 21 + STD_NEED_CLOCK;
const STD_PM_LOWER: i32 = 22 + STD_NEED_CLOCK;
const STD_TZ: i32 = 23;
const STD_ISO8601_TZ: i32 = 24;
const STD_ISO8601_SECONDS_TZ: i32 = 25;
const STD_ISO8601_SHORT_TZ: i32 = 26;
const STD_ISO8601_COLON_TZ: i32 = 27;
const STD_ISO8601_COLON_SECONDS_TZ: i32 = 28;
const STD_NUM_TZ: i32 = 29;
const STD_NUM_SECONDS_TZ: i32 = 30;
const STD_NUM_SHORT_TZ: i32 = 31;
const STD_NUM_COLON_TZ: i32 = 32;
const STD_NUM_COLON_SECONDS_TZ: i32 = 33;
pub(crate) const STD_FRAC_SECOND0: i32 = 34;
pub(crate) const STD_FRAC_SECOND9: i32 = 35;

const STD0X: [i32; 6] = [
    STD_ZERO_MONTH,
    STD_ZERO_DAY,
    STD_ZERO_HOUR12,
    STD_ZERO_MINUTE,
    STD_ZERO_SECOND,
    STD_YEAR,
];

pub(crate) const LONG_DAY_NAMES: [&str; 7] = [
    "Sunday",
    "Monday",
    "Tuesday",
    "Wednesday",
    "Thursday",
    "Friday",
    "Saturday",
];

const SHORT_DAY_NAMES: [&str; 7] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];

const SHORT_MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

pub(crate) const LONG_MONTH_NAMES: [&str; 12] = [
    "January",
    "February",
    "March",
    "April",
    "May",
    "June",
    "July",
    "August",
    "September",
    "October",
    "November",
    "December",
];

fn starts_with_lower_case(s: &[u8]) -> bool {
    s.first().is_some_and(u8::is_ascii_lowercase)
}

fn next_std_chunk(layout: &[u8]) -> (&[u8], i32, &[u8]) {
    for (i, &c) in layout.iter().enumerate() {
        let rest = &layout[i..];
        match c {
            b'J' => {
                if rest.starts_with(b"Jan") {
                    if rest.starts_with(b"January") {
                        return (&layout[..i], STD_LONG_MONTH, &layout[i + 7..]);
                    }
                    if !starts_with_lower_case(&layout[i + 3..]) {
                        return (&layout[..i], STD_MONTH, &layout[i + 3..]);
                    }
                }
            }
            b'M' => {
                if rest.starts_with(b"Mon") {
                    if rest.starts_with(b"Monday") {
                        return (&layout[..i], STD_LONG_WEEK_DAY, &layout[i + 6..]);
                    }
                    if !starts_with_lower_case(&layout[i + 3..]) {
                        return (&layout[..i], STD_WEEK_DAY, &layout[i + 3..]);
                    }
                }
                if rest.starts_with(b"MST") {
                    return (&layout[..i], STD_TZ, &layout[i + 3..]);
                }
            }
            b'0' => {
                if rest.len() >= 2 && (b'1'..=b'6').contains(&rest[1]) {
                    return (
                        &layout[..i],
                        STD0X[usize::from(rest[1] - b'1')],
                        &layout[i + 2..],
                    );
                }
                if rest.starts_with(b"002") {
                    return (&layout[..i], STD_ZERO_YEAR_DAY, &layout[i + 3..]);
                }
            }
            b'1' => {
                if rest.starts_with(b"15") {
                    return (&layout[..i], STD_HOUR, &layout[i + 2..]);
                }
                return (&layout[..i], STD_NUM_MONTH, &layout[i + 1..]);
            }
            b'2' => {
                if rest.starts_with(b"2006") {
                    return (&layout[..i], STD_LONG_YEAR, &layout[i + 4..]);
                }
                return (&layout[..i], STD_DAY, &layout[i + 1..]);
            }
            b'_' => {
                if rest.starts_with(b"_2") {
                    if rest.starts_with(b"_2006") {
                        return (&layout[..i + 1], STD_LONG_YEAR, &layout[i + 5..]);
                    }
                    return (&layout[..i], STD_UNDER_DAY, &layout[i + 2..]);
                }
                if rest.starts_with(b"__2") {
                    return (&layout[..i], STD_UNDER_YEAR_DAY, &layout[i + 3..]);
                }
            }
            b'3' => return (&layout[..i], STD_HOUR12, &layout[i + 1..]),
            b'4' => return (&layout[..i], STD_MINUTE, &layout[i + 1..]),
            b'5' => return (&layout[..i], STD_SECOND, &layout[i + 1..]),
            b'P' => {
                if rest.starts_with(b"PM") {
                    return (&layout[..i], STD_PM, &layout[i + 2..]);
                }
            }
            b'p' => {
                if rest.starts_with(b"pm") {
                    return (&layout[..i], STD_PM_LOWER, &layout[i + 2..]);
                }
            }
            b'-' => {
                if rest.starts_with(b"-070000") {
                    return (&layout[..i], STD_NUM_SECONDS_TZ, &layout[i + 7..]);
                }
                if rest.starts_with(b"-07:00:00") {
                    return (&layout[..i], STD_NUM_COLON_SECONDS_TZ, &layout[i + 9..]);
                }
                if rest.starts_with(b"-0700") {
                    return (&layout[..i], STD_NUM_TZ, &layout[i + 5..]);
                }
                if rest.starts_with(b"-07:00") {
                    return (&layout[..i], STD_NUM_COLON_TZ, &layout[i + 6..]);
                }
                if rest.starts_with(b"-07") {
                    return (&layout[..i], STD_NUM_SHORT_TZ, &layout[i + 3..]);
                }
            }
            b'Z' => {
                if rest.starts_with(b"Z070000") {
                    return (&layout[..i], STD_ISO8601_SECONDS_TZ, &layout[i + 7..]);
                }
                if rest.starts_with(b"Z07:00:00") {
                    return (&layout[..i], STD_ISO8601_COLON_SECONDS_TZ, &layout[i + 9..]);
                }
                if rest.starts_with(b"Z0700") {
                    return (&layout[..i], STD_ISO8601_TZ, &layout[i + 5..]);
                }
                if rest.starts_with(b"Z07:00") {
                    return (&layout[..i], STD_ISO8601_COLON_TZ, &layout[i + 6..]);
                }
                if rest.starts_with(b"Z07") {
                    return (&layout[..i], STD_ISO8601_SHORT_TZ, &layout[i + 3..]);
                }
            }
            b'.' | b',' => {
                if i + 1 < layout.len() && (layout[i + 1] == b'0' || layout[i + 1] == b'9') {
                    let ch = layout[i + 1];
                    let mut j = i + 1;
                    while j < layout.len() && layout[j] == ch {
                        j += 1;
                    }
                    if !is_digit(layout, j) {
                        let code = if layout[i + 1] == b'9' {
                            STD_FRAC_SECOND9
                        } else {
                            STD_FRAC_SECOND0
                        };
                        let std = std_frac_second(code, (j - (i + 1)) as i32, c);
                        return (&layout[..i], std, &layout[j..]);
                    }
                }
            }
            _ => {}
        }
    }
    (layout, 0, &layout[layout.len()..])
}

fn match_fold(s1: &[u8], s2: &[u8]) -> bool {
    for (&a, &b) in s1.iter().zip(s2) {
        if a != b {
            let c1 = a | (b'a' - b'A');
            let c2 = b | (b'a' - b'A');
            if c1 != c2 || !c1.is_ascii_lowercase() {
                return false;
            }
        }
    }
    true
}

fn lookup<'a>(tab: &[&str], val: &'a [u8]) -> Option<(usize, &'a [u8])> {
    for (i, v) in tab.iter().enumerate() {
        let v = v.as_bytes();
        if val.len() >= v.len() && match_fold(&val[..v.len()], v) {
            return Some((i, &val[v.len()..]));
        }
    }
    None
}

pub(crate) fn append_int(b: &mut Vec<u8>, x: i64, width: usize) {
    let mut u = x as u64;
    if x < 0 {
        b.push(b'-');
        u = u.wrapping_neg();
    }
    let mut digits = [0u8; 20];
    let mut i = digits.len();
    loop {
        i -= 1;
        digits[i] = b'0' + (u % 10) as u8;
        u /= 10;
        if u == 0 {
            break;
        }
    }
    let n = digits.len() - i;
    b.resize(b.len() + width.saturating_sub(n), b'0');
    b.extend_from_slice(&digits[i..]);
}

fn atoi(s: &[u8]) -> Option<i64> {
    let mut s = s;
    let mut neg = false;
    if let Some(&c) = s.first()
        && (c == b'-' || c == b'+')
    {
        neg = c == b'-';
        s = &s[1..];
    }
    let (q, rem) = leading_int(s)?;
    if !rem.is_empty() {
        return None;
    }
    let x = q as i64;
    Some(if neg { x.wrapping_neg() } else { x })
}

pub(crate) fn std_frac_second(code: i32, n: i32, c: u8) -> i32 {
    if c == b'.' {
        return code | ((n & 0xfff) << STD_ARG_SHIFT);
    }
    code | ((n & 0xfff) << STD_ARG_SHIFT) | (1 << STD_SEPARATOR_SHIFT)
}

fn digits_len(std: i32) -> usize {
    ((std >> STD_ARG_SHIFT) & 0xfff) as usize
}

fn separator(std: i32) -> u8 {
    if (std >> STD_SEPARATOR_SHIFT) == 0 {
        b'.'
    } else {
        b','
    }
}

pub(crate) fn append_nano(b: &mut Vec<u8>, nanosec: i64, std: i32) {
    let trim = std & STD_MASK == STD_FRAC_SECOND9;
    let n = digits_len(std);
    if trim && (n == 0 || nanosec == 0) {
        return;
    }
    let dot = separator(std);
    b.push(dot);
    append_int(b, nanosec, 9);
    if n < 9 {
        b.truncate(b.len() - 9 + n);
    }
    if trim {
        while b.last() == Some(&b'0') {
            b.pop();
        }
        if b.last() == Some(&dot) {
            b.pop();
        }
    }
}

fn short_name(name: Option<&'static str>) -> &'static [u8] {
    let name = name.unwrap_or("");
    name.as_bytes().get(..3).unwrap_or(name.as_bytes())
}

impl Time {
    pub fn format(&self, layout: &str) -> String {
        let mut b = Vec::with_capacity(layout.len() + 10);
        self.append_format(&mut b, layout);
        match String::from_utf8(b) {
            Ok(s) => s,
            Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
        }
    }

    pub fn append_format(&self, b: &mut Vec<u8>, layout: &str) {
        match layout {
            RFC3339 => self.append_format_rfc3339(b, false),
            RFC3339_NANO => self.append_format_rfc3339(b, true),
            _ => self.append_format_layout(b, layout.as_bytes()),
        }
    }

    fn append_format_layout(&self, b: &mut Vec<u8>, layout: &[u8]) {
        let (name, offset, abs) = self.locabs();
        let days = abs / SECONDS_PER_DAY as u64;
        let mut year: i64 = -1;
        let mut month = Month(0);
        let mut day: i64 = 0;
        let mut yday: i64 = -1;
        let mut hour: i64 = -1;
        let mut min: i64 = 0;
        let mut sec: i64 = 0;
        let mut layout = layout;
        while !layout.is_empty() {
            let (prefix, std, suffix) = next_std_chunk(layout);
            b.extend_from_slice(prefix);
            if std == 0 {
                break;
            }
            layout = suffix;
            if year < 0 && std & STD_NEED_DATE != 0 {
                (year, month, day) = abs_date(days);
            }
            if yday < 0 && std & STD_NEED_YDAY != 0 {
                (_, yday) = abs_year_yday(days);
            }
            if hour < 0 && std & STD_NEED_CLOCK != 0 {
                (hour, min, sec) = abs_clock(abs);
            }
            match std & STD_MASK {
                STD_YEAR => {
                    let y = if year < 0 { year.wrapping_neg() } else { year };
                    append_int(b, y % 100, 2);
                }
                STD_LONG_YEAR => append_int(b, year, 4),
                STD_MONTH => b.extend_from_slice(short_name(month.name())),
                STD_LONG_MONTH => b.extend_from_slice(month.name().unwrap_or("").as_bytes()),
                STD_NUM_MONTH => append_int(b, month.0, 0),
                STD_ZERO_MONTH => append_int(b, month.0, 2),
                STD_WEEK_DAY => b.extend_from_slice(short_name(abs_weekday(days).name())),
                STD_LONG_WEEK_DAY => {
                    b.extend_from_slice(abs_weekday(days).name().unwrap_or("").as_bytes())
                }
                STD_DAY => append_int(b, day, 0),
                STD_UNDER_DAY => {
                    if day < 10 {
                        b.push(b' ');
                    }
                    append_int(b, day, 0);
                }
                STD_ZERO_DAY => append_int(b, day, 2),
                STD_UNDER_YEAR_DAY => {
                    if yday < 100 {
                        b.push(b' ');
                        if yday < 10 {
                            b.push(b' ');
                        }
                    }
                    append_int(b, yday, 0);
                }
                STD_ZERO_YEAR_DAY => append_int(b, yday, 3),
                STD_HOUR => append_int(b, hour, 2),
                STD_HOUR12 => {
                    let hr = if hour % 12 == 0 { 12 } else { hour % 12 };
                    append_int(b, hr, 0);
                }
                STD_ZERO_HOUR12 => {
                    let hr = if hour % 12 == 0 { 12 } else { hour % 12 };
                    append_int(b, hr, 2);
                }
                STD_MINUTE => append_int(b, min, 0),
                STD_ZERO_MINUTE => append_int(b, min, 2),
                STD_SECOND => append_int(b, sec, 0),
                STD_ZERO_SECOND => append_int(b, sec, 2),
                STD_PM => b.extend_from_slice(if hour >= 12 { b"PM" } else { b"AM" }),
                STD_PM_LOWER => b.extend_from_slice(if hour >= 12 { b"pm" } else { b"am" }),
                STD_ISO8601_TZ
                | STD_ISO8601_COLON_TZ
                | STD_ISO8601_SECONDS_TZ
                | STD_ISO8601_SHORT_TZ
                | STD_ISO8601_COLON_SECONDS_TZ
                | STD_NUM_TZ
                | STD_NUM_COLON_TZ
                | STD_NUM_SECONDS_TZ
                | STD_NUM_SHORT_TZ
                | STD_NUM_COLON_SECONDS_TZ => {
                    let iso = matches!(
                        std,
                        STD_ISO8601_TZ
                            | STD_ISO8601_COLON_TZ
                            | STD_ISO8601_SECONDS_TZ
                            | STD_ISO8601_SHORT_TZ
                            | STD_ISO8601_COLON_SECONDS_TZ
                    );
                    if offset == 0 && iso {
                        b.push(b'Z');
                    } else {
                        let mut zone = offset / 60;
                        let mut absoffset = offset;
                        if zone < 0 {
                            b.push(b'-');
                            zone = -zone;
                            absoffset = -absoffset;
                        } else {
                            b.push(b'+');
                        }
                        append_int(b, i64::from(zone / 60), 2);
                        if matches!(
                            std,
                            STD_ISO8601_COLON_TZ
                                | STD_NUM_COLON_TZ
                                | STD_ISO8601_COLON_SECONDS_TZ
                                | STD_NUM_COLON_SECONDS_TZ
                        ) {
                            b.push(b':');
                        }
                        if std != STD_NUM_SHORT_TZ && std != STD_ISO8601_SHORT_TZ {
                            append_int(b, i64::from(zone % 60), 2);
                        }
                        if matches!(
                            std,
                            STD_ISO8601_SECONDS_TZ
                                | STD_NUM_SECONDS_TZ
                                | STD_NUM_COLON_SECONDS_TZ
                                | STD_ISO8601_COLON_SECONDS_TZ
                        ) {
                            if std == STD_NUM_COLON_SECONDS_TZ
                                || std == STD_ISO8601_COLON_SECONDS_TZ
                            {
                                b.push(b':');
                            }
                            append_int(b, i64::from(absoffset % 60), 2);
                        }
                    }
                }
                STD_TZ => {
                    if !name.is_empty() {
                        b.extend_from_slice(name.as_bytes());
                    } else {
                        let mut zone = offset / 60;
                        if zone < 0 {
                            b.push(b'-');
                            zone = -zone;
                        } else {
                            b.push(b'+');
                        }
                        append_int(b, i64::from(zone / 60), 2);
                        append_int(b, i64::from(zone % 60), 2);
                    }
                }
                STD_FRAC_SECOND0 | STD_FRAC_SECOND9 => append_nano(b, self.nanosecond(), std),
                _ => {}
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParseErrorKind {
    Syntax,
    ExtraText,
    OutOfRange(&'static str),
    DayOfYearOutOfRange,
    DayOfYearMonthMismatch,
    DayOfYearDayMismatch,
    DayOutOfRange,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ParseError {
    pub layout: String,
    pub value: Vec<u8>,
    pub layout_elem: String,
    pub value_elem: Vec<u8>,
    pub kind: ParseErrorKind,
}

impl ParseError {
    fn new(
        layout: &str,
        value: &[u8],
        layout_elem: &[u8],
        value_elem: &[u8],
        kind: ParseErrorKind,
    ) -> ParseError {
        ParseError {
            layout: layout.to_string(),
            value: value.to_vec(),
            layout_elem: String::from_utf8_lossy(layout_elem).into_owned(),
            value_elem: value_elem.to_vec(),
            kind,
        }
    }

    pub fn is_rfc3339(&self) -> bool {
        self.layout == RFC3339 || self.layout == RFC3339_NANO
    }
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let what = if self.is_rfc3339() {
            "RFC 3339 time"
        } else {
            "time"
        };
        write!(f, "{} is not a valid {what}", quote_text(&self.value))
    }
}

impl std::error::Error for ParseError {}

fn is_digit(s: &[u8], i: usize) -> bool {
    s.get(i).is_some_and(u8::is_ascii_digit)
}

fn getnum(s: &[u8], fixed: bool) -> Option<(i64, &[u8])> {
    if !is_digit(s, 0) {
        return None;
    }
    if !is_digit(s, 1) {
        if fixed {
            return None;
        }
        return Some((i64::from(s[0] - b'0'), &s[1..]));
    }
    Some((
        i64::from(s[0] - b'0') * 10 + i64::from(s[1] - b'0'),
        &s[2..],
    ))
}

fn getnum3(s: &[u8], fixed: bool) -> Option<(i64, &[u8])> {
    let mut n: i64 = 0;
    let mut i = 0;
    while i < 3 && is_digit(s, i) {
        n = n * 10 + i64::from(s[i] - b'0');
        i += 1;
    }
    if i == 0 || (fixed && i != 3) {
        return None;
    }
    Some((n, &s[i..]))
}

fn cutspace(s: &[u8]) -> &[u8] {
    let n = s.iter().take_while(|&&c| c == b' ').count();
    &s[n..]
}

fn skip<'a>(value: &'a [u8], prefix: &[u8]) -> Result<&'a [u8], &'a [u8]> {
    let mut value = value;
    let mut prefix = prefix;
    while !prefix.is_empty() {
        if prefix[0] == b' ' {
            if !value.is_empty() && value[0] != b' ' {
                return Err(value);
            }
            prefix = cutspace(prefix);
            value = cutspace(value);
            continue;
        }
        if value.is_empty() || value[0] != prefix[0] {
            return Err(value);
        }
        prefix = &prefix[1..];
        value = &value[1..];
    }
    Ok(value)
}

pub fn parse(layout: &str, value: impl AsRef<[u8]>) -> Result<Time, ParseError> {
    let value = value.as_ref();
    let local = Location::local();
    if (layout == RFC3339 || layout == RFC3339_NANO)
        && let Some(t) = parse_rfc3339(value, &local)
    {
        return Ok(t);
    }
    parse_layout(layout, value, &Location::utc(), &local)
}

pub fn parse_in_location(
    layout: &str,
    value: impl AsRef<[u8]>,
    loc: &Location,
) -> Result<Time, ParseError> {
    let value = value.as_ref();
    if (layout == RFC3339 || layout == RFC3339_NANO)
        && let Some(t) = parse_rfc3339(value, loc)
    {
        return Ok(t);
    }
    parse_layout(layout, value, loc, loc)
}

const ZERO2: &[u8] = b"00";

type OffsetParts<'a> = (u8, &'a [u8], &'a [u8], &'a [u8], &'a [u8]);

fn offset_parts(std: i32, value: &[u8]) -> Option<OffsetParts<'_>> {
    if std == STD_ISO8601_COLON_TZ || std == STD_NUM_COLON_TZ {
        if value.len() < 6 || value[3] != b':' {
            return None;
        }
        Some((value[0], &value[1..3], &value[4..6], ZERO2, &value[6..]))
    } else if std == STD_NUM_SHORT_TZ || std == STD_ISO8601_SHORT_TZ {
        if value.len() < 3 {
            return None;
        }
        Some((value[0], &value[1..3], ZERO2, ZERO2, &value[3..]))
    } else if std == STD_ISO8601_COLON_SECONDS_TZ || std == STD_NUM_COLON_SECONDS_TZ {
        if value.len() < 9 || value[3] != b':' || value[6] != b':' {
            return None;
        }
        Some((
            value[0],
            &value[1..3],
            &value[4..6],
            &value[7..9],
            &value[9..],
        ))
    } else if std == STD_ISO8601_SECONDS_TZ || std == STD_NUM_SECONDS_TZ {
        if value.len() < 7 {
            return None;
        }
        Some((
            value[0],
            &value[1..3],
            &value[3..5],
            &value[5..7],
            &value[7..],
        ))
    } else {
        if value.len() < 5 {
            return None;
        }
        Some((value[0], &value[1..3], &value[3..5], ZERO2, &value[5..]))
    }
}

fn parse_layout(
    alayout: &str,
    avalue: &[u8],
    default_location: &Location,
    local: &Location,
) -> Result<Time, ParseError> {
    let mut layout = alayout.as_bytes();
    let mut value = avalue;
    let mut range_err: &'static str = "";
    let mut am_set = false;
    let mut pm_set = false;
    let mut year: i64 = 0;
    let mut month: i64 = -1;
    let mut day: i64 = -1;
    let mut yday: i64 = -1;
    let mut hour: i64 = 0;
    let mut min: i64 = 0;
    let mut sec: i64 = 0;
    let mut nsec: i64 = 0;
    let mut z: Option<Location> = None;
    let mut zone_offset: i64 = -1;
    let mut zone_name: &[u8] = b"";

    loop {
        let (prefix, std, suffix) = next_std_chunk(layout);
        let stdstr = &layout[prefix.len()..layout.len() - suffix.len()];
        value = match skip(value, prefix) {
            Ok(v) => v,
            Err(v) => {
                return Err(ParseError::new(
                    alayout,
                    avalue,
                    prefix,
                    v,
                    ParseErrorKind::Syntax,
                ));
            }
        };
        if std == 0 {
            if !value.is_empty() {
                return Err(ParseError::new(
                    alayout,
                    avalue,
                    b"",
                    value,
                    ParseErrorKind::ExtraText,
                ));
            }
            break;
        }
        layout = suffix;
        let hold = value;
        let mut bad = false;
        match std & STD_MASK {
            STD_YEAR => {
                if value.len() < 2 {
                    bad = true;
                } else {
                    let p = &value[..2];
                    value = &value[2..];
                    match atoi(p) {
                        Some(y) => year = if y >= 69 { y + 1900 } else { y + 2000 },
                        None => bad = true,
                    }
                }
            }
            STD_LONG_YEAR => {
                if value.len() < 4 || !is_digit(value, 0) {
                    bad = true;
                } else {
                    let p = &value[..4];
                    value = &value[4..];
                    match atoi(p) {
                        Some(y) => year = y,
                        None => bad = true,
                    }
                }
            }
            STD_MONTH | STD_LONG_MONTH => {
                let tab: &[&str] = if std == STD_MONTH {
                    &SHORT_MONTH_NAMES
                } else {
                    &LONG_MONTH_NAMES
                };
                match lookup(tab, value) {
                    Some((i, rest)) => {
                        month = i as i64 + 1;
                        value = rest;
                    }
                    None => bad = true,
                }
            }
            STD_NUM_MONTH | STD_ZERO_MONTH => match getnum(value, std == STD_ZERO_MONTH) {
                Some((m, rest)) => {
                    month = m;
                    value = rest;
                    if !(1..=12).contains(&month) {
                        range_err = "month";
                    }
                }
                None => bad = true,
            },
            STD_WEEK_DAY | STD_LONG_WEEK_DAY => {
                let tab: &[&str] = if std == STD_WEEK_DAY {
                    &SHORT_DAY_NAMES
                } else {
                    &LONG_DAY_NAMES
                };
                match lookup(tab, value) {
                    Some((_, rest)) => value = rest,
                    None => bad = true,
                }
            }
            STD_DAY | STD_UNDER_DAY | STD_ZERO_DAY => {
                if std == STD_UNDER_DAY && value.first() == Some(&b' ') {
                    value = &value[1..];
                }
                match getnum(value, std == STD_ZERO_DAY) {
                    Some((d, rest)) => {
                        day = d;
                        value = rest;
                    }
                    None => bad = true,
                }
            }
            STD_UNDER_YEAR_DAY | STD_ZERO_YEAR_DAY => {
                for _ in 0..2 {
                    if std == STD_UNDER_YEAR_DAY && value.first() == Some(&b' ') {
                        value = &value[1..];
                    }
                }
                match getnum3(value, std == STD_ZERO_YEAR_DAY) {
                    Some((d, rest)) => {
                        yday = d;
                        value = rest;
                    }
                    None => bad = true,
                }
            }
            STD_HOUR => match getnum(value, false) {
                Some((h, rest)) => {
                    hour = h;
                    value = rest;
                    if !(0..24).contains(&hour) {
                        range_err = "hour";
                    }
                }
                None => bad = true,
            },
            STD_HOUR12 | STD_ZERO_HOUR12 => match getnum(value, std == STD_ZERO_HOUR12) {
                Some((h, rest)) => {
                    hour = h;
                    value = rest;
                    if !(0..=12).contains(&hour) {
                        range_err = "hour";
                    }
                }
                None => bad = true,
            },
            STD_MINUTE | STD_ZERO_MINUTE => match getnum(value, std == STD_ZERO_MINUTE) {
                Some((m, rest)) => {
                    min = m;
                    value = rest;
                    if !(0..60).contains(&min) {
                        range_err = "minute";
                    }
                }
                None => bad = true,
            },
            STD_SECOND | STD_ZERO_SECOND => match getnum(value, std == STD_ZERO_SECOND) {
                Some((s, rest)) => {
                    sec = s;
                    value = rest;
                    if !(0..60).contains(&sec) {
                        range_err = "second";
                    } else if value.len() >= 2 && comma_or_period(value[0]) && is_digit(value, 1) {
                        let (_, next, _) = next_std_chunk(layout);
                        let next = next & STD_MASK;
                        if next != STD_FRAC_SECOND0 && next != STD_FRAC_SECOND9 {
                            let mut n = 2;
                            while n < value.len() && is_digit(value, n) {
                                n += 1;
                            }
                            match parse_nanoseconds(value, n) {
                                Some((ns, re)) => {
                                    nsec = ns;
                                    range_err = re;
                                }
                                None => bad = true,
                            }
                            value = &value[n..];
                        }
                    }
                }
                None => bad = true,
            },
            STD_PM | STD_PM_LOWER => {
                if value.len() < 2 {
                    bad = true;
                } else {
                    let p = &value[..2];
                    value = &value[2..];
                    let (pm, am) = if std == STD_PM {
                        (b"PM".as_slice(), b"AM".as_slice())
                    } else {
                        (b"pm".as_slice(), b"am".as_slice())
                    };
                    if p == pm {
                        pm_set = true;
                    } else if p == am {
                        am_set = true;
                    } else {
                        bad = true;
                    }
                }
            }
            STD_ISO8601_TZ
            | STD_ISO8601_SHORT_TZ
            | STD_ISO8601_COLON_TZ
            | STD_ISO8601_SECONDS_TZ
            | STD_ISO8601_COLON_SECONDS_TZ
            | STD_NUM_TZ
            | STD_NUM_SHORT_TZ
            | STD_NUM_COLON_TZ
            | STD_NUM_SECONDS_TZ
            | STD_NUM_COLON_SECONDS_TZ => {
                let iso = matches!(
                    std,
                    STD_ISO8601_TZ
                        | STD_ISO8601_SHORT_TZ
                        | STD_ISO8601_COLON_TZ
                        | STD_ISO8601_SECONDS_TZ
                        | STD_ISO8601_COLON_SECONDS_TZ
                );
                if iso && value.first() == Some(&b'Z') {
                    value = &value[1..];
                    z = Some(Location::utc());
                } else {
                    match offset_parts(std, value) {
                        None => bad = true,
                        Some((sign, hh, mm, ss, rest)) => {
                            value = rest;
                            let mut hr = 0;
                            let mut mn = 0;
                            let mut sc = 0;
                            match getnum(hh, true) {
                                Some((v, _)) => {
                                    hr = v;
                                    match getnum(mm, true) {
                                        Some((v, _)) => {
                                            mn = v;
                                            match getnum(ss, true) {
                                                Some((v, _)) => sc = v,
                                                None => bad = true,
                                            }
                                        }
                                        None => bad = true,
                                    }
                                }
                                None => bad = true,
                            }
                            if hr > 24 {
                                range_err = "time zone offset hour";
                            }
                            if mn > 60 {
                                range_err = "time zone offset minute";
                            }
                            if sc > 60 {
                                range_err = "time zone offset second";
                            }
                            zone_offset = (hr * 60 + mn) * 60 + sc;
                            match sign {
                                b'+' => {}
                                b'-' => zone_offset = -zone_offset,
                                _ => bad = true,
                            }
                        }
                    }
                }
            }
            STD_TZ => {
                if value.starts_with(b"UTC") {
                    z = Some(Location::utc());
                    value = &value[3..];
                } else {
                    match parse_time_zone(value) {
                        Some(n) => {
                            zone_name = &value[..n];
                            value = &value[n..];
                        }
                        None => bad = true,
                    }
                }
            }
            STD_FRAC_SECOND0 => {
                let ndigit = 1 + digits_len(std);
                if value.len() < ndigit {
                    bad = true;
                } else {
                    match parse_nanoseconds(value, ndigit) {
                        Some((ns, re)) => {
                            nsec = ns;
                            range_err = re;
                        }
                        None => bad = true,
                    }
                    value = &value[ndigit..];
                }
            }
            STD_FRAC_SECOND9 => {
                if value.len() >= 2 && comma_or_period(value[0]) && value[1].is_ascii_digit() {
                    let mut i = 0;
                    while i + 1 < value.len() && value[i + 1].is_ascii_digit() {
                        i += 1;
                    }
                    match parse_nanoseconds(value, 1 + i) {
                        Some((ns, re)) => {
                            nsec = ns;
                            range_err = re;
                        }
                        None => bad = true,
                    }
                    value = &value[1 + i..];
                }
            }
            _ => {}
        }
        if !range_err.is_empty() {
            return Err(ParseError::new(
                alayout,
                avalue,
                stdstr,
                value,
                ParseErrorKind::OutOfRange(range_err),
            ));
        }
        if bad {
            return Err(ParseError::new(
                alayout,
                avalue,
                stdstr,
                hold,
                ParseErrorKind::Syntax,
            ));
        }
    }
    if pm_set && hour < 12 {
        hour += 12;
    } else if am_set && hour == 12 {
        hour = 0;
    }

    if yday >= 0 {
        let mut d = 0;
        let mut m = 0;
        if is_leap(year) {
            if yday == 31 + 29 {
                m = 2;
                d = 29;
            } else if yday > 31 + 29 {
                yday -= 1;
            }
        }
        if !(1..=365).contains(&yday) {
            return Err(ParseError::new(
                alayout,
                avalue,
                b"",
                value,
                ParseErrorKind::DayOfYearOutOfRange,
            ));
        }
        if m == 0 {
            m = (yday - 1) / 31 + 1;
            if days_before(Month(m + 1)) < yday {
                m += 1;
            }
            d = yday - days_before(Month(m));
        }
        if month >= 0 && month != m {
            return Err(ParseError::new(
                alayout,
                avalue,
                b"",
                value,
                ParseErrorKind::DayOfYearMonthMismatch,
            ));
        }
        month = m;
        if day >= 0 && day != d {
            return Err(ParseError::new(
                alayout,
                avalue,
                b"",
                value,
                ParseErrorKind::DayOfYearDayMismatch,
            ));
        }
        day = d;
    } else {
        if month < 0 {
            month = 1;
        }
        if day < 0 {
            day = 1;
        }
    }

    if !(1..=days_in(Month(month), year)).contains(&day) {
        return Err(ParseError::new(
            alayout,
            avalue,
            b"",
            value,
            ParseErrorKind::DayOutOfRange,
        ));
    }

    if let Some(z) = z {
        return Ok(Time::from_date(
            year,
            Month(month),
            day,
            hour,
            min,
            sec,
            nsec,
            &z,
        ));
    }

    if zone_offset != -1 {
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
        t.add_sec(-zone_offset);
        let zone = local.lookup(t.unix_sec());
        let matched = i64::from(zone.offset) == zone_offset
            && (zone_name.is_empty() || zone.name.as_bytes() == zone_name);
        if matched {
            t.set_loc(local);
            return Ok(t);
        }
        t.set_loc(&Location::fixed(
            &String::from_utf8_lossy(zone_name),
            zone_offset as i32,
        ));
        return Ok(t);
    }

    if !zone_name.is_empty() {
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
        let name = String::from_utf8_lossy(zone_name);
        if let Some(offset) = local.lookup_name(&name, t.unix_sec()) {
            t.add_sec(-i64::from(offset));
            t.set_loc(local);
            return Ok(t);
        }
        let mut offset: i64 = 0;
        if zone_name.len() > 3 && zone_name.starts_with(b"GMT") {
            offset = atoi(&zone_name[3..]).unwrap_or(0);
            offset *= 3600;
        }
        t.set_loc(&Location::fixed(&name, offset as i32));
        return Ok(t);
    }

    Ok(Time::from_date(
        year,
        Month(month),
        day,
        hour,
        min,
        sec,
        nsec,
        default_location,
    ))
}

fn parse_time_zone(value: &[u8]) -> Option<usize> {
    if value.len() < 3 {
        return None;
    }
    if value.starts_with(b"ChST") || value.starts_with(b"MeST") {
        return Some(4);
    }
    if value.starts_with(b"GMT") {
        return Some(parse_gmt(value));
    }
    if value[0] == b'+' || value[0] == b'-' {
        let length = parse_signed_offset(value);
        return if length > 0 { Some(length) } else { None };
    }
    let n_upper = value
        .iter()
        .take(6)
        .take_while(|c| c.is_ascii_uppercase())
        .count();
    match n_upper {
        5 if value[4] == b'T' => Some(5),
        4 if value[3] == b'T' || value.starts_with(b"WITA") => Some(4),
        3 => Some(3),
        _ => None,
    }
}

fn parse_gmt(value: &[u8]) -> usize {
    let value = &value[3..];
    if value.is_empty() {
        return 3;
    }
    3 + parse_signed_offset(value)
}

fn parse_signed_offset(value: &[u8]) -> usize {
    let sign = value[0];
    if sign != b'-' && sign != b'+' {
        return 0;
    }
    let Some((x, rem)) = leading_int(&value[1..]) else {
        return 0;
    };
    if rem.len() == value.len() - 1 {
        return 0;
    }
    if x > 23 {
        return 0;
    }
    value.len() - rem.len()
}

fn comma_or_period(b: u8) -> bool {
    b == b'.' || b == b','
}

pub(crate) fn parse_nanoseconds(value: &[u8], nbytes: usize) -> Option<(i64, &'static str)> {
    if !comma_or_period(value[0]) {
        return None;
    }
    let (value, nbytes) = if nbytes > 10 {
        (&value[..10], 10)
    } else {
        (value, nbytes)
    };
    let mut ns = atoi(&value[1..nbytes])?;
    if ns < 0 {
        return Some((ns, "fractional second"));
    }
    for _ in 0..10 - nbytes {
        ns *= 10;
    }
    Some((ns, ""))
}

pub(crate) fn leading_int(s: &[u8]) -> Option<(u64, &[u8])> {
    let mut x: u64 = 0;
    let mut i = 0;
    while i < s.len() {
        let c = s[i];
        if !c.is_ascii_digit() {
            break;
        }
        if x > (1u64 << 63) / 10 {
            return None;
        }
        x = x * 10 + u64::from(c - b'0');
        if x > 1u64 << 63 {
            return None;
        }
        i += 1;
    }
    Some((x, &s[i..]))
}
