use super::time::{
    Month, SECONDS_PER_DAY, SECONDS_PER_HOUR, SECONDS_PER_MINUTE, UNIX_TO_ABSOLUTE, abs_year_yday,
    days_before, days_in, is_leap,
};
use super::zoneinfo_read::{load_location_sources, load_tzinfo_from_dir_or_zip, parse_tzdata};
use super::zoneinfo_unix::{PLATFORM_ZONE_SOURCES, local_data};
use crate::json::quote_text;
use std::ffi::{OsStr, OsString};
use std::fmt;
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, OnceLock};

pub(crate) const ALPHA: i64 = i64::MIN;
pub(crate) const OMEGA: i64 = i64::MAX;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Zone {
    pub(crate) name: String,
    pub(crate) offset: i32,
    pub(crate) is_dst: bool,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ZoneTrans {
    pub(crate) when: i64,
    pub(crate) index: u8,
}

#[derive(Clone, Debug)]
pub(crate) struct LocationData {
    pub(crate) name: String,
    pub(crate) zone: Vec<Zone>,
    pub(crate) tx: Vec<ZoneTrans>,
    pub(crate) extend: String,
    pub(crate) cache_start: i64,
    pub(crate) cache_end: i64,
    pub(crate) cache_zone: Option<Zone>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ZoneLookup<'a> {
    pub(crate) name: &'a str,
    pub(crate) offset: i32,
    pub(crate) start: i64,
    pub(crate) end: i64,
    pub(crate) is_dst: bool,
}

static UTC_DATA: LazyLock<LocationData> = LazyLock::new(|| LocationData::empty("UTC"));

#[derive(Clone, Debug)]
pub struct Location(Inner);

#[derive(Clone, Debug)]
enum Inner {
    Utc,
    Local,
    Data(Arc<LocationData>),
}

impl Default for Location {
    fn default() -> Location {
        Location::utc()
    }
}

impl Location {
    pub const fn utc() -> Location {
        Location(Inner::Utc)
    }

    pub const fn local() -> Location {
        Location(Inner::Local)
    }

    pub fn fixed(name: &str, offset: i32) -> Location {
        Location::from_data(LocationData::fixed(name, offset))
    }

    pub(crate) fn from_data(data: LocationData) -> Location {
        Location(Inner::Data(Arc::new(data)))
    }

    pub(crate) fn data(&self) -> &LocationData {
        match &self.0 {
            Inner::Utc => &UTC_DATA,
            Inner::Local => local_data(),
            Inner::Data(data) => data.as_ref(),
        }
    }

    pub fn name(&self) -> &str {
        &self.data().name
    }

    pub fn is_utc(&self) -> bool {
        matches!(self.0, Inner::Utc)
    }

    pub fn is_local(&self) -> bool {
        matches!(self.0, Inner::Local)
    }

    pub(crate) fn lookup(&self, sec: i64) -> ZoneLookup<'_> {
        self.data().lookup(sec)
    }

    pub(crate) fn lookup_name(&self, name: &str, unix: i64) -> Option<i32> {
        self.data().lookup_name(name, unix)
    }
}

impl fmt::Display for Location {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

impl LocationData {
    pub(crate) fn empty(name: &str) -> LocationData {
        LocationData {
            name: name.to_string(),
            zone: Vec::new(),
            tx: Vec::new(),
            extend: String::new(),
            cache_start: 0,
            cache_end: 0,
            cache_zone: None,
        }
    }

    pub(crate) fn fixed(name: &str, offset: i32) -> LocationData {
        let zone = Zone {
            name: name.to_string(),
            offset,
            is_dst: false,
        };
        LocationData {
            name: name.to_string(),
            zone: vec![zone.clone()],
            tx: vec![ZoneTrans {
                when: ALPHA,
                index: 0,
            }],
            extend: String::new(),
            cache_start: ALPHA,
            cache_end: OMEGA,
            cache_zone: Some(zone),
        }
    }

    pub(crate) fn lookup(&self, sec: i64) -> ZoneLookup<'_> {
        if self.zone.is_empty() {
            return ZoneLookup {
                name: "UTC",
                offset: 0,
                start: ALPHA,
                end: OMEGA,
                is_dst: false,
            };
        }
        if let Some(zone) = &self.cache_zone
            && (self.cache_start..self.cache_end).contains(&sec)
        {
            return ZoneLookup {
                name: &zone.name,
                offset: zone.offset,
                start: self.cache_start,
                end: self.cache_end,
                is_dst: zone.is_dst,
            };
        }
        if self.tx.is_empty() || sec < self.tx[0].when {
            let zone = &self.zone[self.lookup_first_zone()];
            let end = if self.tx.is_empty() {
                OMEGA
            } else {
                self.tx[0].when
            };
            return ZoneLookup {
                name: &zone.name,
                offset: zone.offset,
                start: ALPHA,
                end,
                is_dst: zone.is_dst,
            };
        }
        let tx = &self.tx;
        let mut end = OMEGA;
        let mut lo = 0usize;
        let mut hi = tx.len();
        while hi - lo > 1 {
            let m = (lo + hi) >> 1;
            let lim = tx[m].when;
            if sec < lim {
                end = lim;
                hi = m;
            } else {
                lo = m;
            }
        }
        let zone = &self.zone[usize::from(tx[lo].index)];
        let start = tx[lo].when;
        if lo == tx.len() - 1
            && !self.extend.is_empty()
            && let Some(found) = tzset(&self.extend, start, sec)
        {
            return found;
        }
        ZoneLookup {
            name: &zone.name,
            offset: zone.offset,
            start,
            end,
            is_dst: zone.is_dst,
        }
    }

    fn lookup_first_zone(&self) -> usize {
        if !self.first_zone_used() {
            return 0;
        }
        if let Some(first) = self.tx.first()
            && self.zone[usize::from(first.index)].is_dst
            && let Some(zi) = self.zone[..usize::from(first.index)]
                .iter()
                .rposition(|zone| !zone.is_dst)
        {
            return zi;
        }
        self.zone.iter().position(|zone| !zone.is_dst).unwrap_or(0)
    }

    fn first_zone_used(&self) -> bool {
        self.tx.iter().any(|tx| tx.index == 0)
    }

    pub(crate) fn lookup_name(&self, name: &str, unix: i64) -> Option<i32> {
        for zone in &self.zone {
            if zone.name == name {
                let found = self.lookup(unix.wrapping_sub(i64::from(zone.offset)));
                if found.name == zone.name {
                    return Some(found.offset);
                }
            }
        }
        self.zone
            .iter()
            .find(|zone| zone.name == name)
            .map(|zone| zone.offset)
    }
}

pub(crate) fn tzset(s: &str, last_tx_sec: i64, sec: i64) -> Option<ZoneLookup<'_>> {
    let (mut std_name, s) = tzset_name(s)?;
    let (std_offset, s) = tzset_offset(s)?;
    let mut std_offset = -std_offset;
    if s.is_empty() || s.as_bytes()[0] == b',' {
        return Some(ZoneLookup {
            name: std_name,
            offset: std_offset as i32,
            start: last_tx_sec,
            end: OMEGA,
            is_dst: false,
        });
    }
    let (mut dst_name, mut s) = tzset_name(s)?;
    let mut dst_offset = if s.is_empty() || s.as_bytes()[0] == b',' {
        std_offset + SECONDS_PER_HOUR
    } else {
        let (offset, rest) = tzset_offset(s)?;
        s = rest;
        -offset
    };
    if s.is_empty() {
        s = ",M3.2.0,M11.1.0";
    }
    if s.as_bytes()[0] != b',' && s.as_bytes()[0] != b';' {
        return None;
    }
    let (start_rule, s) = tzset_rule(&s[1..])?;
    if s.is_empty() || s.as_bytes()[0] != b',' {
        return None;
    }
    let (end_rule, s) = tzset_rule(&s[1..])?;
    if !s.is_empty() {
        return None;
    }

    let abs = sec.wrapping_add(UNIX_TO_ABSOLUTE) as u64;
    let (year, yday) = abs_year_yday(abs / SECONDS_PER_DAY as u64);
    let ysec = (yday - 1) * SECONDS_PER_DAY + sec % SECONDS_PER_DAY;
    let ystart = sec.wrapping_sub(ysec);

    let mut start_sec = tzrule_time(year, start_rule, std_offset);
    let mut end_sec = tzrule_time(year, end_rule, dst_offset);
    let (mut dst_is_dst, mut std_is_dst) = (true, false);
    if end_sec < start_sec {
        std::mem::swap(&mut start_sec, &mut end_sec);
        std::mem::swap(&mut std_name, &mut dst_name);
        std::mem::swap(&mut std_offset, &mut dst_offset);
        std::mem::swap(&mut std_is_dst, &mut dst_is_dst);
    }

    let found = if ysec < start_sec {
        ZoneLookup {
            name: std_name,
            offset: std_offset as i32,
            start: ystart,
            end: start_sec.wrapping_add(ystart),
            is_dst: std_is_dst,
        }
    } else if ysec >= end_sec {
        ZoneLookup {
            name: std_name,
            offset: std_offset as i32,
            start: end_sec.wrapping_add(ystart),
            end: ystart.wrapping_add(365 * SECONDS_PER_DAY),
            is_dst: std_is_dst,
        }
    } else {
        ZoneLookup {
            name: dst_name,
            offset: dst_offset as i32,
            start: start_sec.wrapping_add(ystart),
            end: end_sec.wrapping_add(ystart),
            is_dst: dst_is_dst,
        }
    };
    Some(found)
}

fn tzset_name(s: &str) -> Option<(&str, &str)> {
    let b = s.as_bytes();
    if b.is_empty() {
        return None;
    }
    if b[0] != b'<' {
        for (i, &c) in b.iter().enumerate() {
            if matches!(c, b'0'..=b'9' | b',' | b'-' | b'+') {
                if i < 3 {
                    return None;
                }
                return Some((&s[..i], &s[i..]));
            }
        }
        if b.len() < 3 {
            return None;
        }
        Some((s, ""))
    } else {
        let i = b.iter().position(|&c| c == b'>')?;
        Some((&s[1..i], &s[i + 1..]))
    }
}

fn tzset_offset(s: &str) -> Option<(i64, &str)> {
    let mut s = s;
    let mut neg = false;
    match s.as_bytes().first() {
        None => return None,
        Some(b'+') => s = &s[1..],
        Some(b'-') => {
            s = &s[1..];
            neg = true;
        }
        Some(_) => {}
    }
    let (hours, rest) = tzset_num(s, 0, 24 * 7)?;
    s = rest;
    let mut off = hours * SECONDS_PER_HOUR;
    if s.as_bytes().first() != Some(&b':') {
        return Some((if neg { -off } else { off }, s));
    }
    let (mins, rest) = tzset_num(&s[1..], 0, 59)?;
    s = rest;
    off += mins * SECONDS_PER_MINUTE;
    if s.as_bytes().first() != Some(&b':') {
        return Some((if neg { -off } else { off }, s));
    }
    let (secs, rest) = tzset_num(&s[1..], 0, 59)?;
    s = rest;
    off += secs;
    Some((if neg { -off } else { off }, s))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RuleKind {
    Julian,
    Doy,
    MonthWeekDay,
}

#[derive(Clone, Copy, Debug)]
struct Rule {
    kind: RuleKind,
    day: i64,
    week: i64,
    mon: i64,
    time: i64,
}

fn tzset_rule(s: &str) -> Option<(Rule, &str)> {
    let mut r = Rule {
        kind: RuleKind::Julian,
        day: 0,
        week: 0,
        mon: 0,
        time: 0,
    };
    let first = *s.as_bytes().first()?;
    let mut s = s;
    if first == b'J' {
        let (jday, rest) = tzset_num(&s[1..], 1, 365)?;
        s = rest;
        r.kind = RuleKind::Julian;
        r.day = jday;
    } else if first == b'M' {
        let (mon, rest) = tzset_num(&s[1..], 1, 12)?;
        if rest.as_bytes().first() != Some(&b'.') {
            return None;
        }
        let (week, rest) = tzset_num(&rest[1..], 1, 5)?;
        if rest.as_bytes().first() != Some(&b'.') {
            return None;
        }
        let (day, rest) = tzset_num(&rest[1..], 0, 6)?;
        s = rest;
        r.kind = RuleKind::MonthWeekDay;
        r.day = day;
        r.week = week;
        r.mon = mon;
    } else {
        let (day, rest) = tzset_num(s, 0, 365)?;
        s = rest;
        r.kind = RuleKind::Doy;
        r.day = day;
    }
    if s.as_bytes().first() != Some(&b'/') {
        r.time = 2 * SECONDS_PER_HOUR;
        return Some((r, s));
    }
    let (offset, rest) = tzset_offset(&s[1..])?;
    r.time = offset;
    Some((r, rest))
}

fn tzset_num(s: &str, min: i64, max: i64) -> Option<(i64, &str)> {
    if s.is_empty() {
        return None;
    }
    let mut num: i64 = 0;
    for (i, &c) in s.as_bytes().iter().enumerate() {
        if !c.is_ascii_digit() {
            if i == 0 || num < min {
                return None;
            }
            return Some((num, &s[i..]));
        }
        num = num * 10 + i64::from(c - b'0');
        if num > max {
            return None;
        }
    }
    if num < min {
        return None;
    }
    Some((num, ""))
}

fn tzrule_time(year: i64, r: Rule, off: i64) -> i64 {
    let s = match r.kind {
        RuleKind::Julian => {
            let mut s = (r.day - 1) * SECONDS_PER_DAY;
            if is_leap(year) && r.day >= 60 {
                s += SECONDS_PER_DAY;
            }
            s
        }
        RuleKind::Doy => r.day * SECONDS_PER_DAY,
        RuleKind::MonthWeekDay => {
            let m1 = (r.mon + 9) % 12 + 1;
            let yy0 = if r.mon <= 2 { year - 1 } else { year };
            let yy1 = yy0 / 100;
            let yy2 = yy0 % 100;
            let mut dow = ((26 * m1 - 2) / 10 + 1 + yy2 + yy2 / 4 + yy1 / 4 - 2 * yy1) % 7;
            if dow < 0 {
                dow += 7;
            }
            let mut d = r.day - dow;
            if d < 0 {
                d += 7;
            }
            for _ in 1..r.week {
                if d + 7 >= days_in(Month(r.mon), year) {
                    break;
                }
                d += 7;
            }
            d += days_before(Month(r.mon));
            if is_leap(year) && r.mon > 2 {
                d += 1;
            }
            d * SECONDS_PER_DAY
        }
    };
    s + r.time - off
}

#[derive(Debug)]
pub enum ZoneError {
    InvalidName(String),
    Malformed,
    Unknown(String),
    NotFound,
    TooLarge(PathBuf),
    CorruptZip(PathBuf),
    UnsupportedCompression {
        name: String,
        zip: PathBuf,
    },
    Io {
        op: &'static str,
        path: PathBuf,
        error: io::Error,
    },
}

impl ZoneError {
    pub fn is_not_found(&self) -> bool {
        match self {
            ZoneError::NotFound => true,
            ZoneError::Io { error, .. } => error.kind() == io::ErrorKind::NotFound,
            _ => false,
        }
    }
}

impl fmt::Display for ZoneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ZoneError::InvalidName(name) => {
                write!(
                    f,
                    "{} is not a valid time zone name",
                    quote_text(name.as_bytes())
                )
            }
            ZoneError::Malformed => f.write_str("invalid time zone data"),
            ZoneError::Unknown(name) => {
                write!(f, "unknown time zone {}", quote_text(name.as_bytes()))
            }
            ZoneError::NotFound => f.write_str("time zone not found"),
            ZoneError::TooLarge(path) => {
                write!(f, "read {}: the file is too large", path.display())
            }
            ZoneError::CorruptZip(path) => {
                write!(f, "read {}: corrupt zip file", path.display())
            }
            ZoneError::UnsupportedCompression { name, zip } => write!(
                f,
                "read {}: unsupported compression for {}",
                zip.display(),
                quote_text(name.as_bytes())
            ),
            ZoneError::Io { op, path, error } => {
                write!(f, "{op} {}: {error}", path.display())
            }
        }
    }
}

impl std::error::Error for ZoneError {}

static ZONEINFO: OnceLock<Option<OsString>> = OnceLock::new();

pub fn load_location(name: &str) -> Result<Location, ZoneError> {
    let zoneinfo = ZONEINFO.get_or_init(|| std::env::var_os("ZONEINFO"));
    let platform: Vec<&OsStr> = PLATFORM_ZONE_SOURCES.iter().map(OsStr::new).collect();
    load_location_with(name, zoneinfo.as_deref(), &platform)
}

pub fn load_location_with(
    name: &str,
    zoneinfo: Option<&OsStr>,
    sources: &[&OsStr],
) -> Result<Location, ZoneError> {
    if name.is_empty() || name == "UTC" {
        return Ok(Location::utc());
    }
    if name == "Local" {
        return Ok(Location::local());
    }
    if name.contains("..") || name.starts_with('/') || name.starts_with('\\') {
        return Err(ZoneError::InvalidName(name.to_string()));
    }
    let mut first_err = None;
    if let Some(dir) = zoneinfo
        && !dir.is_empty()
    {
        match load_tzinfo_from_dir_or_zip(dir, OsStr::new(name)) {
            Ok(data) => {
                if let Ok(z) = parse_tzdata(name, &data) {
                    return Ok(Location::from_data(z));
                }
            }
            Err(e) => {
                if !e.is_not_found() {
                    first_err = Some(e);
                }
            }
        }
    }
    match load_location_sources(OsStr::new(name), sources) {
        Ok(z) => Ok(Location::from_data(z)),
        Err(e) => Err(first_err.unwrap_or(e)),
    }
}
