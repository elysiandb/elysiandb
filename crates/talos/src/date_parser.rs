#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Date {
    pub year: i64,
    pub month: i64,
    pub day: i64,
    pub hour: i64,
    pub min: i64,
    pub sec: i64,
    pub msec: i64,
    pub time_zone_offset: i64,
    pub is_local: bool,
}

pub fn skip(s: &[u8], c: u8) -> (&[u8], bool) {
    if !s.is_empty() && s[0] == c {
        return (&s[1..], true);
    }
    (s, false)
}

pub fn skip_spaces(mut s: &[u8]) -> &[u8] {
    while !s.is_empty() && s[0] == b' ' {
        s = &s[1..];
    }
    s
}

pub fn skip_until<'a>(mut s: &'a [u8], stop_list: &[u8]) -> &'a [u8] {
    while !s.is_empty() && !stop_list.contains(&s[0]) {
        s = &s[1..];
    }
    s
}

pub fn r#match<'a>(s: &'a [u8], lower: &[u8]) -> (&'a [u8], bool) {
    if s.len() < lower.len() {
        return (s, false);
    }
    for i in 0..lower.len() {
        let mut c1 = s[i];
        let c2 = lower[i];
        if c1 != c2 {
            c1 |= b'a' - b'A';
            if c1 != c2 || !c1.is_ascii_lowercase() {
                return (s, false);
            }
        }
    }
    (&s[lower.len()..], true)
}

pub fn get_digits(s: &[u8], min_digits: usize, max_digits: usize) -> (i64, &[u8], bool) {
    let mut i = 0;
    let mut v: i64 = 0;
    while i < s.len() && i < max_digits && s[i].is_ascii_digit() {
        v = v * 10 + i64::from(s[i] - b'0');
        i += 1;
    }
    if i < min_digits {
        return (0, s, false);
    }
    (v, &s[i..], true)
}

pub fn get_milliseconds(s: &[u8]) -> (i64, &[u8]) {
    let (mut mul, mut v) = (100i64, 0i64);
    if !s.is_empty() && (s[0] == b'.' || s[0] == b',') {
        const I_START: usize = 1;
        let mut i = I_START;
        while i < s.len() && i - I_START < 9 && s[i].is_ascii_digit() {
            v += i64::from(s[i] - b'0') * mul;
            mul /= 10;
            i += 1;
        }
        if i > I_START {
            return (v, &s[i..]);
        }
    }
    (0, s)
}

pub fn get_time_zone_offset(s: &[u8], strict: bool) -> (i64, &[u8], bool) {
    if s.is_empty() {
        return (0, s, false);
    }
    let sign = s[0];
    if sign == b'+' || sign == b'-' {
        let mut mm = 0;
        let t = &s[1..];
        let mut n = t.len();
        let (mut hh, mut t, ok) = get_digits(t, 1, 9);
        if !ok {
            return (0, s, false);
        }
        n -= t.len();
        if strict && n != 2 && n != 4 {
            return (0, s, false);
        }
        while n > 4 {
            n -= 2;
            hh /= 100;
        }
        if n > 2 {
            mm = hh % 100;
            hh /= 100;
        } else {
            let ok;
            (t, ok) = skip(t, b':');
            if ok {
                let ok;
                (mm, t, ok) = get_digits(t, 2, 2);
                if !ok {
                    return (0, s, false);
                }
            }
        }
        if hh > 23 || mm > 59 {
            return (0, s, false);
        }
        let mut v = hh * 60 + mm;
        if sign == b'-' {
            v = -v;
        }
        return (v, t, true);
    } else if sign == b'Z' {
        return (0, &s[1..], true);
    }
    (0, s, false)
}

pub struct TzAbbr {
    pub name_lower: &'static [u8],
    pub offset: i64,
}

pub static TZ_ABBRS: [TzAbbr; 18] = [
    TzAbbr {
        name_lower: b"gmt",
        offset: 0,
    },
    TzAbbr {
        name_lower: b"utc",
        offset: 0,
    },
    TzAbbr {
        name_lower: b"ut",
        offset: 0,
    },
    TzAbbr {
        name_lower: b"z",
        offset: 0,
    },
    TzAbbr {
        name_lower: b"edt",
        offset: -4 * 60,
    },
    TzAbbr {
        name_lower: b"est",
        offset: -5 * 60,
    },
    TzAbbr {
        name_lower: b"cdt",
        offset: -5 * 60,
    },
    TzAbbr {
        name_lower: b"cst",
        offset: -6 * 60,
    },
    TzAbbr {
        name_lower: b"mdt",
        offset: -6 * 60,
    },
    TzAbbr {
        name_lower: b"mst",
        offset: -7 * 60,
    },
    TzAbbr {
        name_lower: b"pdt",
        offset: -7 * 60,
    },
    TzAbbr {
        name_lower: b"pst",
        offset: -8 * 60,
    },
    TzAbbr {
        name_lower: b"wet",
        offset: 0,
    },
    TzAbbr {
        name_lower: b"west",
        offset: 60,
    },
    TzAbbr {
        name_lower: b"cet",
        offset: 60,
    },
    TzAbbr {
        name_lower: b"cest",
        offset: 2 * 60,
    },
    TzAbbr {
        name_lower: b"eet",
        offset: 2 * 60,
    },
    TzAbbr {
        name_lower: b"eest",
        offset: 3 * 60,
    },
];

pub fn get_time_zone_abbr(s: &[u8]) -> (i64, &[u8], bool) {
    for tz_abbr in &TZ_ABBRS {
        let (rest, ok) = r#match(s, tz_abbr.name_lower);
        if ok {
            return (tz_abbr.offset, rest, true);
        }
    }
    (0, s, false)
}

pub static MONTH_NAMES_LOWER: [&[u8]; 12] = [
    b"jan", b"feb", b"mar", b"apr", b"may", b"jun", b"jul", b"aug", b"sep", b"oct", b"nov", b"dec",
];

pub fn get_month(s: &[u8]) -> (i64, &[u8], bool) {
    for (i, month_name_lower) in MONTH_NAMES_LOWER.iter().enumerate() {
        let (rest, ok) = r#match(s, month_name_lower);
        if ok {
            return (i as i64 + 1, rest, true);
        }
    }
    (0, s, false)
}

pub fn parse_date_iso_string(mut s: &[u8]) -> (Date, bool) {
    if s.is_empty() {
        return (Date::default(), false);
    }
    let mut d = Date {
        month: 1,
        day: 1,
        ..Date::default()
    };
    let mut ok;

    let sign = s[0];
    if sign == b'-' || sign == b'+' {
        s = &s[1..];
        (d.year, s, ok) = get_digits(s, 6, 6);
        if !ok {
            return (Date::default(), false);
        }
        if sign == b'-' {
            if d.year == 0 {
                return (Date::default(), false);
            }
            d.year = -d.year;
        }
    } else {
        (d.year, s, ok) = get_digits(s, 4, 4);
        if !ok {
            return (Date::default(), false);
        }
    }
    (s, ok) = skip(s, b'-');
    if ok {
        (d.month, s, ok) = get_digits(s, 2, 2);
        if !ok || d.month < 1 {
            return (Date::default(), false);
        }
        (s, ok) = skip(s, b'-');
        if ok {
            (d.day, s, ok) = get_digits(s, 2, 2);
            if !ok || d.day < 1 {
                return (Date::default(), false);
            }
        }
    }
    (s, ok) = skip(s, b'T');
    if ok {
        (d.hour, s, ok) = get_digits(s, 2, 2);
        if !ok {
            return (Date::default(), false);
        }
        (s, ok) = skip(s, b':');
        if !ok {
            return (Date::default(), false);
        }
        (d.min, s, ok) = get_digits(s, 2, 2);
        if !ok {
            return (Date::default(), false);
        }
        (s, ok) = skip(s, b':');
        if ok {
            (d.sec, s, ok) = get_digits(s, 2, 2);
            if !ok {
                return (Date::default(), false);
            }
            (d.msec, s) = get_milliseconds(s);
        }
        d.is_local = true;
    }
    if !s.is_empty() {
        (d.time_zone_offset, s, ok) = get_time_zone_offset(s, true);
        if !ok {
            return (Date::default(), false);
        }
        d.is_local = false;
    }
    (d, s.is_empty())
}

pub fn parse_date_other_string(mut s: &[u8]) -> (Date, bool) {
    let mut d = Date {
        year: 2001,
        month: 1,
        day: 1,
        is_local: true,
        ..Date::default()
    };
    let mut nums = [0i64; 3];
    let mut num_index = 0usize;
    let (mut has_year, mut has_mon, mut has_time, mut ok) = (false, false, false, false);
    loop {
        s = skip_spaces(s);
        if s.is_empty() {
            break;
        }
        let c = s[0];
        let n = s.len();
        let mut val;
        if c == b'+' || c == b'-' {
            if has_time {
                (val, s, ok) = get_time_zone_offset(s, false);
                if ok {
                    d.time_zone_offset = val;
                    d.is_local = false;
                }
            }
            if !has_time || !ok {
                s = &s[1..];
                (val, s, ok) = get_digits(s, 1, 9);
                if ok {
                    d.year = val;
                    if c == b'-' {
                        if d.year == 0 {
                            return (Date::default(), false);
                        }
                        d.year = -d.year;
                    }
                    has_year = true;
                }
            }
        } else {
            (val, s, ok) = get_digits(s, 1, 9);
            if ok {
                (s, ok) = skip(s, b':');
                if ok {
                    d.hour = val;
                    (d.min, s, ok) = get_digits(s, 1, 2);
                    if !ok {
                        return (Date::default(), false);
                    }
                    (s, ok) = skip(s, b':');
                    if ok {
                        (d.sec, s, ok) = get_digits(s, 1, 2);
                        if !ok {
                            return (Date::default(), false);
                        }
                        (d.msec, s) = get_milliseconds(s);
                    }
                    has_time = true;
                    let t = skip_spaces(s);
                    if !t.is_empty() {
                        let (t, ok2) = r#match(t, b"pm");
                        ok = ok2;
                        if ok {
                            if d.hour < 12 {
                                d.hour += 12;
                            }
                            s = t;
                            continue;
                        }
                        let (t, ok2) = r#match(t, b"am");
                        ok = ok2;
                        if ok {
                            if d.hour == 12 {
                                d.hour = 0;
                            }
                            s = t;
                            continue;
                        }
                    }
                } else if n - s.len() > 2 {
                    d.year = val;
                    has_year = true;
                } else if !(1..=31).contains(&val) {
                    d.year = val;
                    if val < 100 {
                        d.year += 1900;
                    }
                    if val < 50 {
                        d.year += 100;
                    }
                    has_year = true;
                } else {
                    if num_index == 3 {
                        return (Date::default(), false);
                    }
                    nums[num_index] = val;
                    num_index += 1;
                }
            } else {
                (val, s, ok) = get_month(s);
                if ok {
                    d.month = val;
                    has_mon = true;
                    s = skip_until(s, b"0123456789 -/(");
                } else {
                    (val, s, ok) = get_time_zone_abbr(s);
                    if ok {
                        d.time_zone_offset = val;
                        if !s.is_empty() {
                            let c = s[0];
                            if c.is_ascii_lowercase() || c.is_ascii_uppercase() {
                                return (Date::default(), false);
                            }
                        }
                        d.is_local = false;
                        continue;
                    } else if c == b'(' {
                        let mut level = 1;
                        s = &s[1..];
                        while !s.is_empty() && level != 0 {
                            if s[0] == b'(' {
                                level += 1;
                            } else if s[0] == b')' {
                                level -= 1;
                            }
                            s = &s[1..];
                        }
                        if level > 0 {
                            return (Date::default(), false);
                        }
                    } else if c == b')' {
                        return (Date::default(), false);
                    } else {
                        if has_year || has_mon || has_time || num_index > 0 {
                            return (Date::default(), false);
                        }
                        s = skip_until(s, b" -/(");
                    }
                }
            }
        }
        while !s.is_empty() && b"-/.,".contains(&s[0]) {
            s = &s[1..];
        }
    }
    let mut n = num_index;
    if has_year {
        n += 1;
    }
    if has_mon {
        n += 1;
    }
    if n > 3 {
        return (Date::default(), false);
    }

    match num_index {
        0 => {
            if !has_year {
                return (Date::default(), false);
            }
        }
        1 => {
            if has_mon {
                d.day = nums[0];
            } else {
                d.month = nums[0];
            }
        }
        2 => {
            if has_year {
                d.month = nums[0];
                d.day = nums[1];
            } else if has_mon {
                d.year = nums[1];
                if nums[1] < 100 {
                    d.year += 1900;
                }
                if nums[1] < 50 {
                    d.year += 100;
                }
                d.day = nums[0];
            } else {
                d.month = nums[0];
                d.day = nums[1];
            }
        }
        3 => {
            d.year = nums[2];
            if nums[2] < 100 {
                d.year += 1900;
            }
            if nums[2] < 50 {
                d.year += 100;
            }
            d.month = nums[0];
            d.day = nums[1];
        }
        _ => return (Date::default(), false),
    }
    (d, d.month > 0 && d.day > 0)
}
