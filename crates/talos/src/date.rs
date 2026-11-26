use crate::date_parser::{parse_date_iso_string, parse_date_other_string};
use crate::heap::{ObjRef, Tracer};
use crate::object::{BASE_OBJECT_CLASS, BaseObject, CLASS_DATE, ObjData, ObjExt};
use crate::runtime::Runtime;
use crate::value::{Value, int_to_value};
use gocompat::time::{Location, Month, Time};
use std::any::Any;
use std::sync::LazyLock;

pub const DATE_TIME_LAYOUT: &str = "Mon Jan 02 2006 15:04:05 GMT-0700 (MST)";
pub const UTC_DATE_TIME_LAYOUT: &str = "Mon, 02 Jan 2006 15:04:05 GMT";
pub const ISO_DATE_TIME_LAYOUT: &str = "2006-01-02T15:04:05.000Z";
pub const DATE_LAYOUT: &str = "Mon Jan 02 2006";
pub const TIME_LAYOUT: &str = "15:04:05 GMT-0700 (MST)";
pub const DATETIME_LAYOUT_EN_GB: &str = "01/02/2006, 15:04:05";
pub const DATE_LAYOUT_EN_GB: &str = "01/02/2006";
pub const TIME_LAYOUT_EN_GB: &str = "15:04:05";

pub const MAX_TIME: i64 = 8_640_000_000_000_000;
pub const TIME_UNSET: i64 = i64::MIN;

pub struct DateObject {
    pub msec: i64,
}

impl ObjData for DateObject {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

const HOURS_BEFORE_UTC: i32 = 12;
const HOURS_AFTER_UTC: i32 = 14;

static UNNAMED_FIXED_ZONES: LazyLock<Vec<Location>> = LazyLock::new(|| {
    (-HOURS_BEFORE_UTC..=HOURS_AFTER_UTC)
        .map(|hour| Location::fixed("", hour * 60 * 60))
        .collect()
});

fn fixed_zone(name: &str, offset: i32) -> Location {
    let hour = offset / 60 / 60;
    if name.is_empty()
        && (-HOURS_BEFORE_UTC..=HOURS_AFTER_UTC).contains(&hour)
        && hour * 60 * 60 == offset
    {
        return UNNAMED_FIXED_ZONES[(hour + HOURS_BEFORE_UTC) as usize].clone();
    }
    Location::fixed(name, offset)
}

pub fn date_parse(date: &[u8]) -> (Time, bool) {
    let (mut d, mut ok) = parse_date_iso_string(date);
    if !ok {
        (d, ok) = parse_date_other_string(date);
    }
    if !ok {
        return (Time::default(), false);
    }
    if d.month > 12
        || d.day > 31
        || d.hour > 24
        || d.min > 59
        || d.sec > 59
        || (d.hour == 24 && (d.min != 0 || d.sec != 0 || d.msec != 0))
    {
        return (Time::default(), false);
    }
    let loc = if d.is_local {
        Location::local()
    } else {
        fixed_zone("", (d.time_zone_offset * 60) as i32)
    };
    let t = Time::from_date(
        d.year,
        Month(d.month),
        d.day,
        d.hour,
        d.min,
        d.sec,
        d.msec * 1_000_000,
        &loc,
    );
    let unix_milli = t.unix_milli();
    ok = (-MAX_TIME..=MAX_TIME).contains(&unix_milli);
    (t, ok)
}

impl Runtime {
    pub fn new_date_object(&mut self, t: &Time, is_set: bool, proto: Option<ObjRef>) -> ObjRef {
        let msec = if is_set { time_to_msec(t) } else { TIME_UNSET };
        self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_DATE, proto),
            ObjExt::Dyn(Box::new(DateObject { msec })),
        )
    }
}

pub fn date_format(t: &Time) -> String {
    t.local().format(DATE_TIME_LAYOUT)
}

pub fn time_from_msec(msec: i64) -> Time {
    let sec = msec / 1000;
    let nsec = (msec % 1000) * 1_000_000;
    Time::from_unix(sec, nsec)
}

pub fn time_to_msec(t: &Time) -> i64 {
    t.unix()
        .wrapping_mul(1000)
        .wrapping_add(t.nanosecond() / 1_000_000)
}

impl DateObject {
    pub fn export(&self) -> Option<Time> {
        if self.is_set() {
            return Some(self.time());
        }
        None
    }

    pub fn set_time_ms(&mut self, ms: i64) -> Value {
        if (0..=MAX_TIME).contains(&ms) || (-MAX_TIME..0).contains(&ms) {
            self.msec = ms;
            return int_to_value(ms);
        }
        self.unset();
        Value::Float(f64::NAN)
    }

    pub fn is_set(&self) -> bool {
        self.msec != TIME_UNSET
    }

    pub fn unset(&mut self) {
        self.msec = TIME_UNSET;
    }

    pub fn time(&self) -> Time {
        time_from_msec(self.msec)
    }

    pub fn time_utc(&self) -> Time {
        time_from_msec(self.msec).in_location(&Location::utc())
    }
}
