use super::goreflect::{
    GoKind, GoMethod, GoMethods, arg_bytes, arg_int, arg_location, arg_string, arg_time, go_error,
};
use super::{DocValue, HostRef, TypedSlice};
use crate::object::FunctionCall;
use crate::runtime::{JsResult, Runtime};
use crate::value::{Value, float_to_value, int_to_value};
use gocompat::time::{Duration, Location, Month, Time, Weekday, parse_strict_rfc3339};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

const UNIX_TO_INTERNAL: i64 = 62_135_596_800;

macro_rules! methods {
    ($($name:literal $n:literal),* $(,)?) => {
        &[$(GoMethod { name: $name, num_in: $n }),*]
    };
}

pub static TIME_METHODS: &[GoMethod] = methods![
    "Add" 1, "AddDate" 3, "After" 1, "AppendBinary" 1, "AppendFormat" 2, "AppendText" 1,
    "Before" 1, "Clock" 0, "Compare" 1, "Date" 0, "Day" 0, "Equal" 1, "Format" 1,
    "GoString" 0, "GobDecode" 1, "GobEncode" 0, "Hour" 0, "ISOWeek" 0, "In" 1, "IsDST" 0,
    "IsZero" 0, "Local" 0, "Location" 0, "MarshalBinary" 0, "MarshalJSON" 0,
    "MarshalText" 0, "Minute" 0, "Month" 0, "Nanosecond" 0, "Round" 1, "Second" 0,
    "String" 0, "Sub" 1, "Truncate" 1, "UTC" 0, "Unix" 0, "UnixMicro" 0, "UnixMilli" 0,
    "UnixNano" 0, "UnmarshalBinary" 1, "UnmarshalJSON" 1, "UnmarshalText" 1, "Weekday" 0,
    "Year" 0, "YearDay" 0, "Zone" 0, "ZoneBounds" 0,
];

pub static DURATION_METHODS: &[GoMethod] = methods![
    "Abs" 0, "Hours" 0, "Microseconds" 0, "Milliseconds" 0, "Minutes" 0, "Nanoseconds" 0,
    "Round" 1, "Seconds" 0, "String" 0, "Truncate" 1,
];

pub static STRING_METHODS: &[GoMethod] = methods!["String" 0];

pub const MISSING_LOCATION_PANIC: &str = "missing location in call to In";

pub struct GoTime(pub Rc<RefCell<Time>>);
pub struct GoDuration(pub i64);
pub struct GoMonth(pub i64);
pub struct GoWeekday(pub i64);
pub struct GoLocation(pub Location);

fn host(v: impl GoMethods) -> DocValue {
    let h: HostRef = Rc::new(v);
    DocValue::Host(h)
}

pub fn time_value(t: Time) -> DocValue {
    DocValue::Time(t)
}

pub fn duration_value(d: i64) -> DocValue {
    host(GoDuration(d))
}

fn ret(rt: &mut Runtime, v: DocValue) -> JsResult<Value> {
    rt.to_value(&v)
}

fn ret_multi(rt: &mut Runtime, items: Vec<DocValue>) -> JsResult<Value> {
    rt.to_value(&DocValue::new_slice(items))
}

fn ret_bytes(rt: &mut Runtime, b: Vec<u8>) -> JsResult<Value> {
    let n = b.len();
    let cap = b.capacity().max(n);
    rt.to_value(&DocValue::Bytes(TypedSlice::with_cap(b, cap)))
}

pub fn append_binary(t: &Time, b: &mut Vec<u8>) -> Result<(), String> {
    let mut version = 1u8;
    let mut offset_sec = 0i8;
    let offset_min: i16 = if t.location().is_utc() {
        -1
    } else {
        let (_, mut offset) = t.zone();
        if offset % 60 != 0 {
            version = 2;
            offset_sec = (offset % 60) as i8;
        }
        offset /= 60;
        if !(-32768..=32767).contains(&offset) || offset == -1 {
            return Err("unexpected zone offset".to_string());
        }
        offset as i16
    };
    let sec = t.unix().wrapping_add(UNIX_TO_INTERNAL);
    let nsec = t.nanosecond() as i32;
    b.push(version);
    b.extend_from_slice(&sec.to_be_bytes());
    b.extend_from_slice(&nsec.to_be_bytes());
    b.extend_from_slice(&offset_min.to_be_bytes());
    if version == 2 {
        b.push(offset_sec as u8);
    }
    Ok(())
}

pub fn unmarshal_binary(data: &[u8]) -> Result<Time, String> {
    if data.is_empty() {
        return Err("no data".to_string());
    }
    let version = data[0];
    if version != 1 && version != 2 {
        return Err("unsupported version".to_string());
    }
    let want = if version == 2 { 16 } else { 15 };
    if data.len() != want {
        return Err("invalid length".to_string());
    }
    let mut s = [0u8; 8];
    s.copy_from_slice(&data[1..9]);
    let sec = i64::from_be_bytes(s);
    let mut n = [0u8; 4];
    n.copy_from_slice(&data[9..13]);
    let nsec = i32::from_be_bytes(n);
    let mut offset = i32::from(i16::from_be_bytes([data[13], data[14]])) * 60;
    if version == 2 {
        offset += i32::from(data[15]);
    }
    let t = Time::from_unix(sec.wrapping_sub(UNIX_TO_INTERNAL), i64::from(nsec));
    if offset == -60 {
        return Ok(t.in_location(&Location::utc()));
    }
    if t.zone().1 == offset {
        return Ok(t);
    }
    Ok(t.in_location(&Location::fixed("", offset)))
}

pub fn marshal_json(t: &Time, out: &mut Vec<u8>) -> Result<(), String> {
    let mut b = vec![b'"'];
    t.append_strict_rfc3339(&mut b).map_err(|e| e.to_string())?;
    b.push(b'"');
    out.extend_from_slice(&b);
    Ok(())
}

pub fn go_string(t: &Time) -> String {
    let (year, month, day) = t.date();
    let (hour, min, sec) = t.clock();
    let mut b = format!("time.Date({year}");
    if (1..=12).contains(&month.0) {
        b.push_str(", time.");
        b.push_str(&month.to_string());
    } else {
        b.push_str(&month.0.to_string());
    }
    b.push_str(&format!(
        ", {day}, {hour}, {min}, {sec}, {}, ",
        t.nanosecond()
    ));
    let loc = t.location();
    if loc.is_utc() {
        b.push_str("time.UTC");
    } else if loc.is_local() {
        b.push_str("time.Local");
    } else {
        b.push_str("time.Location(");
        b.push_str(&gocompat::strconv::quote(loc.name().as_bytes()));
        b.push(')');
    }
    b.push(')');
    b
}

fn not_rfc3339(data: &[u8]) -> String {
    let mut q = Vec::new();
    gocompat::json::append_string(&mut q, data, false);
    format!(
        "{} is not a valid RFC 3339 time",
        String::from_utf8_lossy(&q)
    )
}

impl GoMethods for GoTime {
    fn kind(&self) -> GoKind {
        GoKind::Struct
    }

    fn methods(&self) -> &'static [GoMethod] {
        TIME_METHODS
    }

    fn call(&self, rt: &mut Runtime, idx: usize, call: FunctionCall<'_>) -> JsResult<Value> {
        let t = self.0.borrow().clone();
        match TIME_METHODS[idx].name {
            "Add" => {
                let d = arg_int(rt, &call, 0)?;
                ret(rt, DocValue::Time(t.add(Duration(d))))
            }
            "AddDate" => {
                let y = arg_int(rt, &call, 0)?;
                let m = arg_int(rt, &call, 1)?;
                let d = arg_int(rt, &call, 2)?;
                ret(rt, DocValue::Time(t.add_date(y, m, d)))
            }
            "After" => {
                let u = arg_time(rt, &call, 0)?;
                Ok(Value::Bool(t.after(&u)))
            }
            "Before" => {
                let u = arg_time(rt, &call, 0)?;
                Ok(Value::Bool(t.before(&u)))
            }
            "Compare" => {
                let u = arg_time(rt, &call, 0)?;
                Ok(Value::Int(t.compare(&u) as i64))
            }
            "Equal" => {
                let u = arg_time(rt, &call, 0)?;
                Ok(Value::Bool(t.equal(&u)))
            }
            "AppendBinary" => {
                let mut b = arg_bytes(rt, &call, 0)?;
                match append_binary(&t, &mut b) {
                    Ok(()) => ret_bytes(rt, b),
                    Err(e) => Err(go_error(rt, &e)),
                }
            }
            "AppendFormat" => {
                let mut b = arg_bytes(rt, &call, 0)?;
                let layout = arg_string(rt, &call, 1)?;
                t.append_format(&mut b, &layout);
                ret_bytes(rt, b)
            }
            "AppendText" => {
                let mut b = arg_bytes(rt, &call, 0)?;
                match t.append_strict_rfc3339(&mut b) {
                    Ok(()) => ret_bytes(rt, b),
                    Err(e) => Err(go_error(rt, &e.to_string())),
                }
            }
            "Clock" => {
                let (h, m, s) = t.clock();
                ret_multi(
                    rt,
                    vec![DocValue::Int64(h), DocValue::Int64(m), DocValue::Int64(s)],
                )
            }
            "Date" => {
                let (y, m, d) = t.date();
                ret_multi(
                    rt,
                    vec![DocValue::Int64(y), host(GoMonth(m.0)), DocValue::Int64(d)],
                )
            }
            "Day" => Ok(int_to_value(t.day())),
            "Format" => {
                let layout = arg_string(rt, &call, 0)?;
                ret(rt, DocValue::String(t.format(&layout).into()))
            }
            "GoString" => ret(rt, DocValue::String(go_string(&t).into())),
            "GobDecode" | "UnmarshalBinary" => {
                let data = arg_bytes(rt, &call, 0)?;
                match unmarshal_binary(&data) {
                    Ok(nt) => {
                        *self.0.borrow_mut() = nt;
                        Ok(Value::Undefined)
                    }
                    Err(e) => Err(go_error(rt, &e)),
                }
            }
            "GobEncode" | "MarshalBinary" => {
                let mut b = Vec::with_capacity(16);
                match append_binary(&t, &mut b) {
                    Ok(()) => ret_bytes(rt, b),
                    Err(e) => Err(go_error(rt, &e)),
                }
            }
            "Hour" => Ok(int_to_value(t.hour())),
            "ISOWeek" => {
                let (y, w) = t.iso_week();
                ret_multi(rt, vec![DocValue::Int64(y), DocValue::Int64(w)])
            }
            "In" => {
                let loc = arg_location(rt, &call, 0)?;
                let Some(loc) = loc else {
                    panic!("{}", MISSING_LOCATION_PANIC);
                };
                ret(rt, DocValue::Time(t.in_location(&loc)))
            }
            "IsDST" => Ok(Value::Bool(t.is_dst())),
            "IsZero" => Ok(Value::Bool(t.is_zero())),
            "Local" => ret(rt, DocValue::Time(t.local())),
            "UTC" => ret(rt, DocValue::Time(t.utc())),
            "Location" => ret(rt, host(GoLocation(t.location()))),
            "MarshalJSON" => {
                let mut b = Vec::new();
                match marshal_json(&t, &mut b) {
                    Ok(()) => ret_bytes(rt, b),
                    Err(e) => Err(go_error(rt, &e)),
                }
            }
            "MarshalText" => {
                let mut b = Vec::with_capacity(35);
                match t.append_strict_rfc3339(&mut b) {
                    Ok(()) => ret_bytes(rt, b),
                    Err(e) => Err(go_error(rt, &e.to_string())),
                }
            }
            "Minute" => Ok(int_to_value(t.minute())),
            "Month" => ret(rt, host(GoMonth(t.month().0))),
            "Nanosecond" => Ok(int_to_value(t.nanosecond())),
            "Round" => {
                let d = arg_int(rt, &call, 0)?;
                ret(rt, DocValue::Time(t.round(Duration(d))))
            }
            "Truncate" => {
                let d = arg_int(rt, &call, 0)?;
                ret(rt, DocValue::Time(t.truncate(Duration(d))))
            }
            "Second" => Ok(int_to_value(t.second())),
            "String" => ret(rt, DocValue::String(t.to_string().into())),
            "Sub" => {
                let u = arg_time(rt, &call, 0)?;
                ret(rt, duration_value(t.sub(&u).0))
            }
            "Unix" => Ok(int_to_value(t.unix())),
            "UnixMicro" => Ok(int_to_value(t.unix_micro())),
            "UnixMilli" => Ok(int_to_value(t.unix_milli())),
            "UnixNano" => Ok(int_to_value(t.unix_nano())),
            "UnmarshalJSON" => {
                let data = arg_bytes(rt, &call, 0)?;
                if data == b"null" {
                    return Ok(Value::Undefined);
                }
                if data.len() < 2 || data[0] != b'"' || data[data.len() - 1] != b'"' {
                    return Err(go_error(rt, "input is not a JSON string"));
                }
                let inner = &data[1..data.len() - 1];
                match parse_strict_rfc3339(inner) {
                    Some(nt) => {
                        *self.0.borrow_mut() = nt;
                        Ok(Value::Undefined)
                    }
                    None => {
                        *self.0.borrow_mut() = Time::default();
                        Err(go_error(rt, &not_rfc3339(inner)))
                    }
                }
            }
            "UnmarshalText" => {
                let data = arg_bytes(rt, &call, 0)?;
                match parse_strict_rfc3339(&data) {
                    Some(nt) => {
                        *self.0.borrow_mut() = nt;
                        Ok(Value::Undefined)
                    }
                    None => {
                        *self.0.borrow_mut() = Time::default();
                        Err(go_error(rt, &not_rfc3339(&data)))
                    }
                }
            }
            "Weekday" => ret(rt, host(GoWeekday(t.weekday().0))),
            "Year" => Ok(int_to_value(t.year())),
            "YearDay" => Ok(int_to_value(t.year_day())),
            "Zone" => {
                let (name, offset) = t.zone();
                let name = name.to_string();
                ret_multi(
                    rt,
                    vec![
                        DocValue::String(name.into()),
                        DocValue::Int64(i64::from(offset)),
                    ],
                )
            }
            "ZoneBounds" => {
                let (start, end) = t.zone_bounds();
                ret_multi(
                    rt,
                    vec![
                        DocValue::Time(start.unwrap_or_default()),
                        DocValue::Time(end.unwrap_or_default()),
                    ],
                )
            }
            _ => Ok(Value::Undefined),
        }
    }

    fn stringer(&self) -> Option<String> {
        Some(self.0.borrow().to_string())
    }

    fn export_value(&self, _this: &HostRef) -> DocValue {
        DocValue::Time(self.0.borrow().clone())
    }

    fn same(&self, other: &dyn GoMethods) -> bool {
        match other.as_any().downcast_ref::<GoTime>() {
            Some(o) => {
                let (a, b) = (self.0.borrow(), o.0.borrow());
                a.unix_nano() == b.unix_nano()
                    && a.nanosecond() == b.nanosecond()
                    && a.location().name() == b.location().name()
            }
            None => false,
        }
    }

    fn marshal_json(&self, out: &mut Vec<u8>) -> Result<(), String> {
        marshal_json(&self.0.borrow(), out)
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl GoMethods for GoDuration {
    fn kind(&self) -> GoKind {
        GoKind::Int
    }

    fn methods(&self) -> &'static [GoMethod] {
        DURATION_METHODS
    }

    fn call(&self, rt: &mut Runtime, idx: usize, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = Duration(self.0);
        match DURATION_METHODS[idx].name {
            "Abs" => ret(rt, duration_value(d.abs().0)),
            "Hours" => Ok(float_to_value(d.hours())),
            "Microseconds" => Ok(int_to_value(d.microseconds())),
            "Milliseconds" => Ok(int_to_value(d.milliseconds())),
            "Minutes" => Ok(float_to_value(d.minutes())),
            "Nanoseconds" => Ok(int_to_value(d.nanoseconds())),
            "Round" => {
                let m = arg_int(rt, &call, 0)?;
                ret(rt, duration_value(d.round(Duration(m)).0))
            }
            "Seconds" => Ok(float_to_value(d.seconds())),
            "String" => ret(rt, DocValue::String(d.to_string().into())),
            "Truncate" => {
                let m = arg_int(rt, &call, 0)?;
                ret(rt, duration_value(d.truncate(Duration(m)).0))
            }
            _ => Ok(Value::Undefined),
        }
    }

    fn stringer(&self) -> Option<String> {
        Some(Duration(self.0).to_string())
    }

    fn value_of(&self) -> Option<Value> {
        Some(int_to_value(self.0))
    }

    fn same(&self, other: &dyn GoMethods) -> bool {
        other
            .as_any()
            .downcast_ref::<GoDuration>()
            .is_some_and(|o| o.0 == self.0)
    }

    fn marshal_json(&self, out: &mut Vec<u8>) -> Result<(), String> {
        out.extend_from_slice(self.0.to_string().as_bytes());
        Ok(())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

macro_rules! int_stringer {
    ($t:ident, $show:expr) => {
        impl GoMethods for $t {
            fn kind(&self) -> GoKind {
                GoKind::Int
            }

            fn methods(&self) -> &'static [GoMethod] {
                STRING_METHODS
            }

            fn call(
                &self,
                rt: &mut Runtime,
                _idx: usize,
                _call: FunctionCall<'_>,
            ) -> JsResult<Value> {
                ret(rt, DocValue::String($show(self.0).into()))
            }

            fn stringer(&self) -> Option<String> {
                Some($show(self.0))
            }

            fn value_of(&self) -> Option<Value> {
                Some(int_to_value(self.0))
            }

            fn same(&self, other: &dyn GoMethods) -> bool {
                other
                    .as_any()
                    .downcast_ref::<$t>()
                    .is_some_and(|o| o.0 == self.0)
            }

            fn marshal_json(&self, out: &mut Vec<u8>) -> Result<(), String> {
                out.extend_from_slice(self.0.to_string().as_bytes());
                Ok(())
            }

            fn as_any(&self) -> &dyn Any {
                self
            }
        }
    };
}

fn month_name(m: i64) -> String {
    if (1..=12).contains(&m) {
        return Month(m).to_string();
    }
    format!("%!Month({m})")
}

fn weekday_name(d: i64) -> String {
    if (0..=6).contains(&d) {
        return Weekday(d).to_string();
    }
    format!("%!Weekday({d})")
}

int_stringer!(GoMonth, month_name);
int_stringer!(GoWeekday, weekday_name);

impl GoMethods for GoLocation {
    fn methods(&self) -> &'static [GoMethod] {
        STRING_METHODS
    }

    fn call(&self, rt: &mut Runtime, _idx: usize, _call: FunctionCall<'_>) -> JsResult<Value> {
        ret(rt, DocValue::String(self.0.name().into()))
    }

    fn stringer(&self) -> Option<String> {
        Some(self.0.name().to_string())
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl crate::host::goreflect::GoReflect {
    pub fn new_time(rt: &mut Runtime, t: Time) -> crate::heap::ObjRef {
        let h: HostRef = Rc::new(GoTime(Rc::new(RefCell::new(t))));
        rt.new_go_reflect(h)
    }
}
