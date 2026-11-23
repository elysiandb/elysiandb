use super::duration::{Duration, less_than_half};
use super::format::{LONG_DAY_NAMES, LONG_MONTH_NAMES};
use super::zoneinfo::{ALPHA, Location, OMEGA};
use std::cmp::Ordering;
use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const SECONDS_PER_MINUTE: i64 = 60;
pub(crate) const SECONDS_PER_HOUR: i64 = 60 * SECONDS_PER_MINUTE;
pub(crate) const SECONDS_PER_DAY: i64 = 24 * SECONDS_PER_HOUR;
const MARCH_THRU_DECEMBER: u64 = 31 + 30 + 31 + 30 + 31 + 31 + 30 + 31 + 30 + 31;
const ABSOLUTE_YEARS: u64 = 292277022400;
const ABSOLUTE_TO_INTERNAL: i64 =
    -((ABSOLUTE_YEARS as i64 / 400 * 146097 + MARCH_THRU_DECEMBER as i64) * SECONDS_PER_DAY);
const INTERNAL_TO_ABSOLUTE: i64 = -ABSOLUTE_TO_INTERNAL;
pub(crate) const UNIX_TO_INTERNAL: i64 =
    (1969 * 365 + 1969 / 4 - 1969 / 100 + 1969 / 400) * SECONDS_PER_DAY;
const INTERNAL_TO_UNIX: i64 = -UNIX_TO_INTERNAL;
const ABSOLUTE_TO_UNIX: i64 = ABSOLUTE_TO_INTERNAL + INTERNAL_TO_UNIX;
pub(crate) const UNIX_TO_ABSOLUTE: i64 = UNIX_TO_INTERNAL + INTERNAL_TO_ABSOLUTE;
const SECOND: i64 = 1_000_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Month(pub i64);

impl Month {
    pub const JANUARY: Month = Month(1);
    pub const FEBRUARY: Month = Month(2);
    pub const MARCH: Month = Month(3);
    pub const APRIL: Month = Month(4);
    pub const MAY: Month = Month(5);
    pub const JUNE: Month = Month(6);
    pub const JULY: Month = Month(7);
    pub const AUGUST: Month = Month(8);
    pub const SEPTEMBER: Month = Month(9);
    pub const OCTOBER: Month = Month(10);
    pub const NOVEMBER: Month = Month(11);
    pub const DECEMBER: Month = Month(12);

    pub const fn number(self) -> i64 {
        self.0
    }

    pub(crate) fn name(self) -> Option<&'static str> {
        if (1..=12).contains(&self.0) {
            Some(LONG_MONTH_NAMES[(self.0 - 1) as usize])
        } else {
            None
        }
    }
}

impl fmt::Display for Month {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "{}", self.0),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Weekday(pub i64);

impl Weekday {
    pub const SUNDAY: Weekday = Weekday(0);
    pub const MONDAY: Weekday = Weekday(1);
    pub const TUESDAY: Weekday = Weekday(2);
    pub const WEDNESDAY: Weekday = Weekday(3);
    pub const THURSDAY: Weekday = Weekday(4);
    pub const FRIDAY: Weekday = Weekday(5);
    pub const SATURDAY: Weekday = Weekday(6);

    pub const fn number(self) -> i64 {
        self.0
    }

    pub(crate) fn name(self) -> Option<&'static str> {
        if (0..=6).contains(&self.0) {
            Some(LONG_DAY_NAMES[self.0 as usize])
        } else {
            None
        }
    }
}

impl fmt::Display for Weekday {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.name() {
            Some(name) => f.write_str(name),
            None => write!(f, "{}", self.0),
        }
    }
}

pub(crate) fn date_to_abs_days(year: i64, month: Month, day: i64) -> u64 {
    let mut amonth = month.0 as u32;
    let jan_feb = u32::from(amonth < 3);
    amonth = amonth.wrapping_add(12 * jan_feb);
    let y = (year as u64)
        .wrapping_sub(u64::from(jan_feb))
        .wrapping_add(ABSOLUTE_YEARS);
    let ayday = 979u32.wrapping_mul(amonth).wrapping_sub(2919) >> 5;
    let century = y / 100;
    let cyear = (y % 100) as u32;
    let cday = 1461 * cyear / 4;
    let centurydays = 146097u64.wrapping_mul(century) / 4;
    centurydays.wrapping_add(
        i64::from(cday.wrapping_add(ayday))
            .wrapping_add(day)
            .wrapping_sub(1) as u64,
    )
}

fn split_days(days: u64) -> (u64, u32, u32) {
    let d = days.wrapping_mul(4).wrapping_add(3);
    let century = d / 146097;
    let cd = (d % 146097) as u32 | 3;
    let prod = 2939745u64 * u64::from(cd);
    let cyear = (prod >> 32) as u32;
    let ayday = (prod as u32) / 2939745 / 4;
    (century, cyear, ayday)
}

fn split_yday(ayday: u32) -> (u32, i64) {
    let d = 2141 * ayday + 197913;
    (d >> 16, 1 + i64::from((d & 0xFFFF) / 2141))
}

fn jan_feb(ayday: u32) -> u32 {
    u32::from(u64::from(ayday) >= MARCH_THRU_DECEMBER)
}

fn leap(century: u64, cyear: u32) -> i64 {
    let y4ok = i64::from(cyear.is_multiple_of(4));
    let y100ok = i64::from(cyear != 0);
    let y400ok = i64::from(century.is_multiple_of(4));
    y4ok & (y100ok | y400ok)
}

fn century_year(century: u64, cyear: u32, jan_feb: u32) -> i64 {
    (century.wrapping_mul(100).wrapping_sub(ABSOLUTE_YEARS) as i64)
        .wrapping_add(i64::from(cyear))
        .wrapping_add(i64::from(jan_feb))
}

pub(crate) fn abs_date(days: u64) -> (i64, Month, i64) {
    let (century, cyear, ayday) = split_days(days);
    let (amonth, day) = split_yday(ayday);
    let jf = jan_feb(ayday);
    let year = century_year(century, cyear, jf);
    let month = Month(i64::from(amonth) - i64::from(jf) * 12);
    (year, month, day)
}

pub(crate) fn abs_year_yday(days: u64) -> (i64, i64) {
    let (century, cyear, ayday) = split_days(days);
    let jf = jan_feb(ayday);
    let year = century_year(century, cyear, jf);
    let yday = i64::from(ayday) + (1 + 31 + 28) + (leap(century, cyear) & !i64::from(jf))
        - 365 * i64::from(jf);
    (year, yday)
}

pub(crate) fn abs_weekday(days: u64) -> Weekday {
    Weekday((days.wrapping_add(3) % 7) as i64)
}

pub(crate) fn abs_clock(abs: u64) -> (i64, i64, i64) {
    let mut sec = (abs % SECONDS_PER_DAY as u64) as i64;
    let hour = sec / SECONDS_PER_HOUR;
    sec -= hour * SECONDS_PER_HOUR;
    let min = sec / SECONDS_PER_MINUTE;
    sec -= min * SECONDS_PER_MINUTE;
    (hour, min, sec)
}

pub(crate) fn abs_days(abs: u64) -> u64 {
    abs / SECONDS_PER_DAY as u64
}

pub(crate) fn is_leap(year: i64) -> bool {
    let mask = if year % 25 != 0 { 3 } else { 0xf };
    year & mask == 0
}

pub(crate) fn days_before(m: Month) -> i64 {
    let adj = if m.0 >= 3 { -2 } else { 0 };
    (214i64.wrapping_mul(m.0).wrapping_sub(211)) / 7 + adj
}

pub(crate) fn days_in(m: Month, year: i64) -> i64 {
    if m.0 == 2 {
        if is_leap(year) { 29 } else { 28 }
    } else {
        30 + (m.0.wrapping_add(m.0 >> 3) & 1)
    }
}

fn norm(hi: i64, lo: i64, base: i64) -> (i64, i64) {
    let (mut hi, mut lo) = (hi, lo);
    if lo < 0 {
        let n = lo.wrapping_neg().wrapping_sub(1) / base + 1;
        hi = hi.wrapping_sub(n);
        lo = lo.wrapping_add(n.wrapping_mul(base));
    }
    if lo >= base {
        let n = lo / base;
        hi = hi.wrapping_add(n);
        lo = lo.wrapping_sub(n.wrapping_mul(base));
    }
    (hi, lo)
}

#[derive(Clone, Debug, Default)]
pub struct Time {
    pub(super) sec: i64,
    pub(super) nsec: i32,
    pub(super) loc: Location,
}

pub(crate) fn unix_time(sec: i64, nsec: i32) -> Time {
    Time {
        sec: sec.wrapping_add(UNIX_TO_INTERNAL),
        nsec,
        loc: Location::local(),
    }
}

impl Time {
    pub fn now() -> Time {
        match SystemTime::now().duration_since(UNIX_EPOCH) {
            Ok(d) => Time::from_unix(d.as_secs() as i64, i64::from(d.subsec_nanos())),
            Err(e) => {
                let d = e.duration();
                Time::from_unix(-(d.as_secs() as i64), -i64::from(d.subsec_nanos()))
            }
        }
    }

    pub fn from_unix(sec: i64, nsec: i64) -> Time {
        let (mut sec, mut nsec) = (sec, nsec);
        if !(0..SECOND).contains(&nsec) {
            let n = nsec / SECOND;
            sec = sec.wrapping_add(n);
            nsec -= n * SECOND;
            if nsec < 0 {
                nsec += SECOND;
                sec = sec.wrapping_sub(1);
            }
        }
        unix_time(sec, nsec as i32)
    }

    pub fn from_unix_milli(msec: i64) -> Time {
        Time::from_unix(msec / 1_000, (msec % 1_000) * 1_000_000)
    }

    pub fn from_unix_micro(usec: i64) -> Time {
        Time::from_unix(usec / 1_000_000, (usec % 1_000_000) * 1_000)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn from_date(
        year: i64,
        month: Month,
        day: i64,
        hour: i64,
        min: i64,
        sec: i64,
        nsec: i64,
        loc: &Location,
    ) -> Time {
        let m = month.0.wrapping_sub(1);
        let (year, m) = norm(year, m, 12);
        let month = Month(m + 1);
        let (sec, nsec) = norm(sec, nsec, SECOND);
        let (min, sec) = norm(min, sec, 60);
        let (hour, min) = norm(hour, min, 60);
        let (day, hour) = norm(day, hour, 24);
        let mut unix = (date_to_abs_days(year, month, day) as i64)
            .wrapping_mul(SECONDS_PER_DAY)
            .wrapping_add(
                hour.wrapping_mul(SECONDS_PER_HOUR)
                    .wrapping_add(min.wrapping_mul(SECONDS_PER_MINUTE))
                    .wrapping_add(sec),
            )
            .wrapping_add(ABSOLUTE_TO_UNIX);
        let zone = loc.lookup(unix);
        let mut offset = zone.offset;
        if offset != 0 {
            let utc = unix.wrapping_sub(i64::from(offset));
            if !(zone.start..zone.end).contains(&utc) {
                offset = loc.lookup(utc).offset;
            }
            unix = unix.wrapping_sub(i64::from(offset));
        }
        let mut t = unix_time(unix, nsec as i32);
        t.set_loc(loc);
        t
    }

    pub(crate) fn unix_sec(&self) -> i64 {
        self.sec.wrapping_add(INTERNAL_TO_UNIX)
    }

    pub(crate) fn add_sec(&mut self, d: i64) {
        let sum = self.sec.wrapping_add(d);
        if (sum > self.sec) == (d > 0) {
            self.sec = sum;
        } else if d > 0 {
            self.sec = i64::MAX;
        } else {
            self.sec = -i64::MAX;
        }
    }

    pub(crate) fn set_loc(&mut self, loc: &Location) {
        self.loc = loc.clone();
    }

    pub fn is_zero(&self) -> bool {
        self.sec == 0 && self.nsec == 0
    }

    pub fn after(&self, u: &Time) -> bool {
        self.compare(u) == Ordering::Greater
    }

    pub fn before(&self, u: &Time) -> bool {
        self.compare(u) == Ordering::Less
    }

    pub fn compare(&self, u: &Time) -> Ordering {
        (self.sec, self.nsec).cmp(&(u.sec, u.nsec))
    }

    pub fn equal(&self, u: &Time) -> bool {
        self.sec == u.sec && self.nsec == u.nsec
    }

    fn abs_sec(&self) -> u64 {
        let mut sec = self.unix_sec();
        if !self.loc.is_utc() {
            sec = sec.wrapping_add(i64::from(self.loc.lookup(sec).offset));
        }
        sec.wrapping_add(UNIX_TO_ABSOLUTE) as u64
    }

    pub(crate) fn locabs(&self) -> (&str, i32, u64) {
        let mut sec = self.unix_sec();
        let (name, offset) = if self.loc.is_utc() {
            ("UTC", 0)
        } else {
            let zone = self.loc.lookup(sec);
            sec = sec.wrapping_add(i64::from(zone.offset));
            (zone.name, zone.offset)
        };
        (name, offset, sec.wrapping_add(UNIX_TO_ABSOLUTE) as u64)
    }

    pub fn date(&self) -> (i64, Month, i64) {
        abs_date(abs_days(self.abs_sec()))
    }

    pub fn year(&self) -> i64 {
        self.date().0
    }

    pub fn month(&self) -> Month {
        self.date().1
    }

    pub fn day(&self) -> i64 {
        self.date().2
    }

    pub fn weekday(&self) -> Weekday {
        abs_weekday(abs_days(self.abs_sec()))
    }

    pub fn iso_week(&self) -> (i64, i64) {
        let days = abs_days(self.abs_sec());
        let wd = abs_weekday(days.wrapping_sub(1)).0;
        let thu = days.wrapping_add((Weekday::THURSDAY.0 - (wd + 1)) as u64);
        let (year, yday) = abs_year_yday(thu);
        (year, (yday - 1) / 7 + 1)
    }

    pub fn clock(&self) -> (i64, i64, i64) {
        abs_clock(self.abs_sec())
    }

    pub fn hour(&self) -> i64 {
        self.clock().0
    }

    pub fn minute(&self) -> i64 {
        self.clock().1
    }

    pub fn second(&self) -> i64 {
        self.clock().2
    }

    pub fn nanosecond(&self) -> i64 {
        i64::from(self.nsec)
    }

    pub fn year_day(&self) -> i64 {
        abs_year_yday(abs_days(self.abs_sec())).1
    }

    pub fn add(&self, d: Duration) -> Time {
        let mut dsec = d.0 / SECOND;
        let mut nsec = self.nsec + (d.0 % SECOND) as i32;
        if nsec >= SECOND as i32 {
            dsec += 1;
            nsec -= SECOND as i32;
        } else if nsec < 0 {
            dsec -= 1;
            nsec += SECOND as i32;
        }
        let mut t = self.clone();
        t.nsec = nsec;
        t.add_sec(dsec);
        t
    }

    pub fn sub(&self, u: &Time) -> Duration {
        let d = Duration(
            self.sec
                .wrapping_sub(u.sec)
                .wrapping_mul(SECOND)
                .wrapping_add(i64::from(self.nsec - u.nsec)),
        );
        if u.add(d).equal(self) {
            d
        } else if self.before(u) {
            Duration::MIN
        } else {
            Duration::MAX
        }
    }

    pub fn since(t: &Time) -> Duration {
        Time::now().sub(t)
    }

    pub fn until(t: &Time) -> Duration {
        t.sub(&Time::now())
    }

    pub fn add_date(&self, years: i64, months: i64, days: i64) -> Time {
        let (year, month, day) = self.date();
        let (hour, min, sec) = self.clock();
        Time::from_date(
            year.wrapping_add(years),
            Month(month.0.wrapping_add(months)),
            day.wrapping_add(days),
            hour,
            min,
            sec,
            i64::from(self.nsec),
            &self.location(),
        )
    }

    pub fn utc(&self) -> Time {
        let mut t = self.clone();
        t.set_loc(&Location::utc());
        t
    }

    pub fn local(&self) -> Time {
        let mut t = self.clone();
        t.set_loc(&Location::local());
        t
    }

    pub fn in_location(&self, loc: &Location) -> Time {
        let mut t = self.clone();
        t.set_loc(loc);
        t
    }

    pub fn location(&self) -> Location {
        self.loc.clone()
    }

    pub fn zone(&self) -> (&str, i32) {
        let zone = self.loc.lookup(self.unix_sec());
        (zone.name, zone.offset)
    }

    pub fn zone_bounds(&self) -> (Option<Time>, Option<Time>) {
        let zone = self.loc.lookup(self.unix_sec());
        let start = (zone.start != ALPHA).then(|| {
            let mut t = unix_time(zone.start, 0);
            t.set_loc(&self.loc);
            t
        });
        let end = (zone.end != OMEGA).then(|| {
            let mut t = unix_time(zone.end, 0);
            t.set_loc(&self.loc);
            t
        });
        (start, end)
    }

    pub fn is_dst(&self) -> bool {
        self.loc.lookup(self.unix()).is_dst
    }

    pub fn unix(&self) -> i64 {
        self.unix_sec()
    }

    pub fn unix_milli(&self) -> i64 {
        self.unix_sec()
            .wrapping_mul(1_000)
            .wrapping_add(i64::from(self.nsec) / 1_000_000)
    }

    pub fn unix_micro(&self) -> i64 {
        self.unix_sec()
            .wrapping_mul(1_000_000)
            .wrapping_add(i64::from(self.nsec) / 1_000)
    }

    pub fn unix_nano(&self) -> i64 {
        self.unix_sec()
            .wrapping_mul(SECOND)
            .wrapping_add(i64::from(self.nsec))
    }

    pub fn truncate(&self, d: Duration) -> Time {
        if d.0 <= 0 {
            return self.clone();
        }
        let r = div(self, d);
        self.add(Duration(-r.0))
    }

    pub fn round(&self, d: Duration) -> Time {
        if d.0 <= 0 {
            return self.clone();
        }
        let r = div(self, d);
        if less_than_half(r.0, d.0) {
            self.add(Duration(-r.0))
        } else {
            self.add(Duration(d.0 - r.0))
        }
    }
}

fn div(t: &Time, d: Duration) -> Duration {
    let mut neg = false;
    let mut nsec = t.nsec;
    let mut sec = t.sec;
    if sec < 0 {
        neg = true;
        sec = sec.wrapping_neg();
        nsec = -nsec;
        if nsec < 0 {
            nsec += SECOND as i32;
            sec = sec.wrapping_sub(1);
        }
    }
    let d = d.0;
    let mut r = if d < SECOND && SECOND % (d + d) == 0 {
        i64::from(nsec % d as i32)
    } else if d % SECOND == 0 {
        let d1 = d / SECOND;
        (sec % d1)
            .wrapping_mul(SECOND)
            .wrapping_add(i64::from(nsec))
    } else {
        let u = u128::from(sec as u64) * SECOND as u128 + u128::from(nsec as u32);
        (u % u128::from(d as u64)) as i64
    };
    if neg && r != 0 {
        r = d - r;
    }
    Duration(r)
}

impl fmt::Display for Time {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.format("2006-01-02 15:04:05.999999999 -0700 MST"))
    }
}
