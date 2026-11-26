use crate::builtin_symbol::SYM_TO_PRIMITIVE;
use crate::date::{
    DATE_LAYOUT, DATE_LAYOUT_EN_GB, DATE_TIME_LAYOUT, DATETIME_LAYOUT_EN_GB, DateObject,
    ISO_DATE_TIME_LAYOUT, MAX_TIME, TIME_LAYOUT, TIME_LAYOUT_EN_GB, UTC_DATE_TIME_LAYOUT,
    date_format, date_parse, time_from_msec, time_to_msec,
};
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::FunctionCall;
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw, limit_call_args};
use crate::string::{JsString, STRING_INVALID_DATE, STRING_NUMBER, STRING_STRING, ascii_string};
use crate::value::{Value, float_to_value, go_float_to_int, int_to_value, value_prop};
use gocompat::math::abs;
use gocompat::time::{Location, Month, Time};
use std::sync::{LazyLock, OnceLock};

static STRING_DEFAULT: LazyLock<JsString> = LazyLock::new(|| ascii_string("default"));

fn nan() -> Value {
    Value::Float(f64::NAN)
}

fn is_nan(v: &Value) -> bool {
    matches!(v, Value::Float(f) if f.is_nan())
}

fn runtime_now(r: &mut Runtime) -> Time {
    match &mut r.now {
        Some(now) => now(),
        None => Time::now(),
    }
}

fn this_date(r: &mut Runtime, this: &Value, method: &str) -> JsResult<(ObjRef, DateObject)> {
    let obj = r.to_object(this)?;
    if let Some(d) = r.ext_dyn::<DateObject>(obj) {
        return Ok((obj, DateObject { msec: d.msec }));
    }
    Err(Throw::Value(r.new_type_error(&format!(
        "Method Date.prototype.{method} is called on incompatible receiver"
    ))))
}

fn date_is_set(r: &Runtime, obj: ObjRef) -> bool {
    r.ext_dyn::<DateObject>(obj).is_some_and(DateObject::is_set)
}

fn date_unset(r: &mut Runtime, obj: ObjRef) {
    if let Some(d) = r.ext_dyn_mut::<DateObject>(obj) {
        d.unset();
    }
}

fn date_set_time_ms(r: &mut Runtime, obj: ObjRef, ms: i64) -> Value {
    match r.ext_dyn_mut::<DateObject>(obj) {
        Some(d) => d.set_time_ms(ms),
        None => nan(),
    }
}

fn date_msec(r: &Runtime, obj: ObjRef) -> i64 {
    r.ext_dyn::<DateObject>(obj).map_or(i64::MIN, |d| d.msec)
}

impl Runtime {
    pub fn make_date(&mut self, args: &[Value], utc: bool) -> JsResult<(Time, bool)> {
        let mut t = Time::default();
        let mut valid = false;
        if args.len() >= 2 {
            t = Time::from_date(1970, Month::JANUARY, 1, 0, 0, 0, 0, &Location::local());
            (t, valid) = _date_set_year(
                self,
                &t,
                &FunctionCall::new(&Value::Undefined, args),
                0,
                utc,
            )?;
        } else if args.is_empty() {
            t = runtime_now(self);
            valid = true;
        } else {
            if let Value::Object(o) = &args[0]
                && let Some(d) = self.ext_dyn::<DateObject>(*o)
            {
                t = d.time();
                valid = true;
            }
            if !valid {
                let pv = match &args[0] {
                    Value::Object(o) => o.to_primitive(self)?,
                    v => v.clone(),
                };
                if let Value::String(val) = &pv {
                    return Ok(date_parse(&val.go_string()));
                }
                let pv = pv.to_number(self)?;
                let n = match pv {
                    Value::Int(i) => i,
                    Value::Float(f) => {
                        if f.is_nan() || f.is_infinite() {
                            return Ok((t, valid));
                        }
                        if abs(f) > MAX_TIME as f64 {
                            return Ok((t, valid));
                        }
                        go_float_to_int(f)
                    }
                    other => other.prim_to_integer(),
                };
                t = time_from_msec(n);
                valid = true;
            }
        }
        if valid {
            let mut msec = t
                .unix()
                .wrapping_mul(1000)
                .wrapping_add(t.nanosecond() / 1_000_000);
            if msec < 0 {
                msec = msec.wrapping_neg();
            }
            if msec > MAX_TIME {
                valid = false;
            }
        }
        Ok((t, valid))
    }

    pub fn new_date_time(&mut self, args: &[Value], proto: Option<ObjRef>) -> JsResult<ObjRef> {
        let (t, is_set) = self.make_date(args, false)?;
        Ok(self.new_date_object(&t, is_set, proto))
    }

    pub fn builtin_new_date(&mut self, args: &[Value], proto: Option<ObjRef>) -> JsResult<ObjRef> {
        self.new_date_time(args, proto)
    }

    pub fn builtin_date(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        let now = runtime_now(self);
        Ok(Value::String(ascii_string(&date_format(&now))))
    }

    pub fn date_parse(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let s = call.argument(0).to_string(self)?;
        let (t, set) = date_parse(&s.go_string());
        if set {
            return Ok(int_to_value(time_to_msec(&t)));
        }
        Ok(nan())
    }

    pub fn date_utc(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let short;
        let args = if call.arguments.len() < 2 {
            short = [call.argument(0), Value::Int(0)];
            &short[..]
        } else {
            call.arguments
        };
        let (t, valid) = self.make_date(args, true)?;
        if !valid {
            return Ok(nan());
        }
        Ok(int_to_value(time_to_msec(&t)))
    }

    pub fn date_now(&mut self, _call: FunctionCall<'_>) -> JsResult<Value> {
        let now = runtime_now(self);
        Ok(int_to_value(time_to_msec(&now)))
    }

    pub fn dateproto_to_iso_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toISOString")?.1;
        if d.is_set() {
            let utc = d.time_utc();
            let year = utc.year();
            if (-9999..=9999).contains(&year) {
                return Ok(Value::String(ascii_string(
                    &utc.format(ISO_DATE_TIME_LAYOUT),
                )));
            }
            let s = format!("{year:+06}-") + &utc.format(&ISO_DATE_TIME_LAYOUT[5..]);
            return Ok(Value::String(ascii_string(&s)));
        }
        Err(Throw::Value(self.new_range_error("Invalid time value")))
    }

    pub fn dateproto_to_json(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.this.to_object(self)?;
        let tv = obj.to_primitive_number(self)?;
        if let Value::Float(f) = tv
            && (f.is_nan() || f.is_infinite())
        {
            return Ok(Value::Null);
        }

        if let Some(Value::Object(to_iso)) = obj.get_str(self, &"toISOString".into(), None)?
            && let Some(to_iso) = to_iso.assert_callable(self)
        {
            return to_iso.call(self, Value::Object(obj), &[]);
        }

        Err(Throw::Value(
            self.new_type_error("toISOString is not a function"),
        ))
    }

    pub fn dateproto_to_primitive(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.to_object(call.this)?;
        let arg = call.argument(0);

        if STRING_STRING.strict_equals(&arg) || STRING_DEFAULT.strict_equals(&arg) {
            return o.ordinary_to_primitive_string(self);
        }
        if STRING_NUMBER.strict_equals(&arg) {
            return o.ordinary_to_primitive_number(self);
        }
        let s = arg.go_string(self)?;
        Err(Throw::Value(
            self.new_type_error(&format!("Invalid hint: {s}")),
        ))
    }

    pub fn dateproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "valueOf")?.1;
        if d.is_set() {
            Ok(int_to_value(d.msec))
        } else {
            Ok(nan())
        }
    }

    pub fn dateproto_get_time(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getTime")?.1;
        if d.is_set() {
            Ok(int_to_value(d.msec))
        } else {
            Ok(nan())
        }
    }

    pub fn dateproto_get_timezone_offset(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getTimezoneOffset")?.1;
        if d.is_set() {
            let offset = d.time().zone().1;
            Ok(float_to_value(f64::from(-offset) / 60.0))
        } else {
            Ok(nan())
        }
    }

    pub fn dateproto_set_time(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = this_date(self, call.this, "setTime")?.0;
        let n = call.argument(0).to_number(self)?;
        if is_nan(&n) {
            date_unset(self, obj);
            return Ok(nan());
        }
        Ok(date_set_time_ms(self, obj, n.prim_to_integer()))
    }

    pub fn dateproto_set_milliseconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.date_set_ms(call, "setMilliseconds")
    }

    pub fn dateproto_set_utc_milliseconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.date_set_ms(call, "setUTCMilliseconds")
    }

    fn date_set_ms(&mut self, call: FunctionCall<'_>, method: &str) -> JsResult<Value> {
        let obj = this_date(self, call.this, method)?.0;
        let n = call.argument(0).to_number(self)?;
        if is_nan(&n) {
            date_unset(self, obj);
            return Ok(nan());
        }
        let msec = n.prim_to_integer();
        let sec = date_msec(self, obj) / 1000;
        let (sec, msec, ok) = _norm(sec, msec, 1000);
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(
                self,
                obj,
                sec.wrapping_mul(1000).wrapping_add(msec),
            ))
        } else {
            Ok(nan())
        }
    }

    pub fn dateproto_set_full_year(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setFullYear")?;
        let t = if d.is_set() {
            d.time()
        } else {
            Time::from_date(1970, Month::JANUARY, 1, 0, 0, 0, 0, &Location::local())
        };
        let (t, ok) = _date_set_full_year(self, &t, &limit_call_args(call, 3), 0, false)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
    }

    pub fn dateproto_set_utc_full_year(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setUTCFullYear")?;
        let t = if d.is_set() {
            d.time_utc()
        } else {
            Time::from_date(1970, Month::JANUARY, 1, 0, 0, 0, 0, &Location::utc())
        };
        let (t, ok) = _date_set_full_year(self, &t, &limit_call_args(call, 3), 0, true)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
    }

    pub fn dateproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(
                &d.time().format(DATE_TIME_LAYOUT),
            )))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_to_utc_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toUTCString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(
                &d.time_utc().format(UTC_DATE_TIME_LAYOUT),
            )))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_to_date_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toDateString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(&d.time().format(DATE_LAYOUT))))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_to_time_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toTimeString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(&d.time().format(TIME_LAYOUT))))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_to_locale_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toLocaleString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(
                &d.time().format(DATETIME_LAYOUT_EN_GB),
            )))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_to_locale_date_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toLocaleDateString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(
                &d.time().format(DATE_LAYOUT_EN_GB),
            )))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_to_locale_time_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "toLocaleTimeString")?.1;
        if d.is_set() {
            Ok(Value::String(ascii_string(
                &d.time().format(TIME_LAYOUT_EN_GB),
            )))
        } else {
            Ok(Value::String(STRING_INVALID_DATE.clone()))
        }
    }
    pub fn dateproto_get_full_year(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getFullYear")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().year()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_full_year(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCFullYear")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().year()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_month(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getMonth")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().month().0 - 1))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_month(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCMonth")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().month().0 - 1))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_hours(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getHours")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().hour()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_hours(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCHours")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().hour()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_date(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getDate")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().day()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_date(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCDate")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().day()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_day(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getDay")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().weekday().0))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_day(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCDay")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().weekday().0))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_minutes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getMinutes")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().minute()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_minutes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCMinutes")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().minute()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_seconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getSeconds")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().second()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_seconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCSeconds")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().second()))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_milliseconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getMilliseconds")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time().nanosecond() / 1_000_000))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_get_utc_milliseconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let d = this_date(self, call.this, "getUTCMilliseconds")?.1;
        if d.is_set() {
            Ok(int_to_value(d.time_utc().nanosecond() / 1_000_000))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_seconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setSeconds")?;
        let (t, ok) = _date_set_full_year(self, &d.time(), &call, -5, false)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_utc_seconds(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setUTCSeconds")?;
        let (t, ok) = _date_set_full_year(self, &d.time_utc(), &call, -5, true)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_minutes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setMinutes")?;
        let (t, ok) = _date_set_full_year(self, &d.time(), &call, -4, false)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_utc_minutes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setUTCMinutes")?;
        let (t, ok) = _date_set_full_year(self, &d.time_utc(), &call, -4, true)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_hours(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setHours")?;
        let (t, ok) = _date_set_full_year(self, &d.time(), &call, -3, false)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_utc_hours(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setUTCHours")?;
        let (t, ok) = _date_set_full_year(self, &d.time_utc(), &call, -3, true)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_date(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setDate")?;
        let (t, ok) = _date_set_full_year(self, &d.time(), &limit_call_args(call, 1), -2, false)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_utc_date(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setUTCDate")?;
        let (t, ok) =
            _date_set_full_year(self, &d.time_utc(), &limit_call_args(call, 1), -2, true)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_month(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setMonth")?;
        let (t, ok) = _date_set_full_year(self, &d.time(), &limit_call_args(call, 2), -1, false)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
    pub fn dateproto_set_utc_month(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (obj, d) = this_date(self, call.this, "setUTCMonth")?;
        let (t, ok) =
            _date_set_full_year(self, &d.time_utc(), &limit_call_args(call, 2), -1, true)?;
        if !ok {
            date_unset(self, obj);
            return Ok(nan());
        }
        if date_is_set(self, obj) {
            Ok(date_set_time_ms(self, obj, time_to_msec(&t)))
        } else {
            Ok(nan())
        }
    }
}

fn _norm(hi: i64, lo: i64, base: i64) -> (i64, i64, bool) {
    let (mut hi, mut lo) = (hi, lo);
    if lo < 0 {
        if hi == i64::MIN && lo <= -base {
            return (0, 0, false);
        }
        let n = lo.wrapping_neg().wrapping_sub(1) / base + 1;
        hi = hi.wrapping_sub(n);
        lo = lo.wrapping_add(n.wrapping_mul(base));
    }
    if lo >= base {
        if hi == i64::MAX {
            return (0, 0, false);
        }
        let n = lo / base;
        hi = hi.wrapping_add(n);
        lo = lo.wrapping_sub(n.wrapping_mul(base));
    }
    (hi, lo, true)
}

#[allow(clippy::too_many_arguments)]
fn mk_time(
    year: i64,
    m: i64,
    day: i64,
    hour: i64,
    min: i64,
    sec: i64,
    nsec: i64,
    loc: &Location,
) -> (Time, bool) {
    let (year, m, ok) = _norm(year, m, 12);
    if !ok {
        return (Time::default(), false);
    }

    let (sec, nsec, ok) = _norm(sec, nsec, 1_000_000_000);
    if !ok {
        return (Time::default(), false);
    }
    let (min, sec, ok) = _norm(min, sec, 60);
    if !ok {
        return (Time::default(), false);
    }
    let (hour, min, ok) = _norm(hour, min, 60);
    if !ok {
        return (Time::default(), false);
    }
    let (day, hour, ok) = _norm(day, hour, 24);
    if !ok {
        return (Time::default(), false);
    }
    if year > i64::from(i32::MAX)
        || year < i64::from(i32::MIN)
        || day > i64::from(i32::MAX)
        || day < i64::from(i32::MIN)
        || m >= i64::from(i32::MAX)
        || m < i64::from(i32::MIN) - 1
    {
        return (Time::default(), false);
    }
    let month = Month(m + 1);
    (
        Time::from_date(year, month, day, hour, min, sec, nsec, loc),
        true,
    )
}

fn _int_arg(r: &mut Runtime, call: &FunctionCall<'_>, arg_num: i64) -> JsResult<(i64, bool)> {
    let n = call.argument(arg_num as usize).to_number(r)?;
    if is_nan(&n) {
        return Ok((0, false));
    }
    Ok((n.prim_to_integer(), true))
}

fn has_arg(call: &FunctionCall<'_>, arg_num: i64) -> bool {
    arg_num == 0 || arg_num > 0 && (arg_num as usize) < call.arguments.len()
}

fn _date_set_year(
    r: &mut Runtime,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let year = if has_arg(call, arg_num) {
        let (y, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        if (0..=99).contains(&y) { y + 1900 } else { y }
    } else {
        t.year()
    };

    _date_set_month(r, year, t, call, arg_num + 1, utc)
}

fn _date_set_full_year(
    r: &mut Runtime,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let year = if has_arg(call, arg_num) {
        let (y, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        y
    } else {
        t.year()
    };
    _date_set_month(r, year, t, call, arg_num + 1, utc)
}

fn _date_set_month(
    r: &mut Runtime,
    year: i64,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let mon = if has_arg(call, arg_num) {
        let (v, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        v
    } else {
        t.month().0 - 1
    };

    _date_set_day(r, year, mon, t, call, arg_num + 1, utc)
}

fn _date_set_day(
    r: &mut Runtime,
    year: i64,
    mon: i64,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let day = if has_arg(call, arg_num) {
        let (v, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        v
    } else {
        t.day()
    };

    _date_set_hours(r, year, mon, day, t, call, arg_num + 1, utc)
}

#[allow(clippy::too_many_arguments)]
fn _date_set_hours(
    r: &mut Runtime,
    year: i64,
    mon: i64,
    day: i64,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let hours = if has_arg(call, arg_num) {
        let (v, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        v
    } else {
        t.hour()
    };
    _date_set_minutes(r, year, mon, day, hours, t, call, arg_num + 1, utc)
}

#[allow(clippy::too_many_arguments)]
fn _date_set_minutes(
    r: &mut Runtime,
    year: i64,
    mon: i64,
    day: i64,
    hours: i64,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let min = if has_arg(call, arg_num) {
        let (v, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        v
    } else {
        t.minute()
    };
    _date_set_seconds(r, year, mon, day, hours, min, t, call, arg_num + 1, utc)
}

#[allow(clippy::too_many_arguments)]
fn _date_set_seconds(
    r: &mut Runtime,
    year: i64,
    mon: i64,
    day: i64,
    hours: i64,
    min: i64,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let sec = if has_arg(call, arg_num) {
        let (v, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        v
    } else {
        t.second()
    };
    _date_set_milliseconds(
        r,
        year,
        mon,
        day,
        hours,
        min,
        sec,
        t,
        call,
        arg_num + 1,
        utc,
    )
}

#[allow(clippy::too_many_arguments)]
fn _date_set_milliseconds(
    r: &mut Runtime,
    year: i64,
    mon: i64,
    day: i64,
    hours: i64,
    min: i64,
    sec: i64,
    t: &Time,
    call: &FunctionCall<'_>,
    arg_num: i64,
    utc: bool,
) -> JsResult<(Time, bool)> {
    let msec = if has_arg(call, arg_num) {
        let (v, ok) = _int_arg(r, call, arg_num)?;
        if !ok {
            return Ok((Time::default(), false));
        }
        v
    } else {
        t.nanosecond() / 1_000_000
    };
    let (sec, msec, ok) = _norm(sec, msec, 1000);
    if !ok {
        return Ok((Time::default(), false));
    }

    let loc = if utc {
        Location::utc()
    } else {
        Location::local()
    };
    let (res, ok) = mk_time(year, mon, day, hours, min, sec, msec * 1_000_000, &loc);
    if !ok {
        return Ok((Time::default(), false));
    }
    if utc {
        return Ok((res.in_location(&Location::local()), true));
    }
    Ok((res, true))
}

fn create_date_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| Some(r.get_function_prototype()));

    t.put_str("name", |_| {
        value_prop(Value::String(ascii_string("Date")), false, false, true)
    });
    t.put_str("length", |_| {
        value_prop(int_to_value(7), false, false, true)
    });

    t.put_str("prototype", |r| {
        value_prop(Value::Object(r.get_date_prototype()), false, false, false)
    });

    t.put_str("parse", |r| {
        r.method_prop(|r, c| r.date_parse(c), "parse", 1)
    });
    t.put_str("UTC", |r| r.method_prop(|r, c| r.date_utc(c), "UTC", 7));
    t.put_str("now", |r| r.method_prop(|r, c| r.date_now(c), "now", 0));

    t
}

pub fn get_date_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_date_template)
}

fn create_date_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_date()), true, false, true)
    });

    t.put_str("toString", |r| {
        r.method_prop(|r, c| r.dateproto_to_string(c), "toString", 0)
    });
    t.put_str("toDateString", |r| {
        r.method_prop(|r, c| r.dateproto_to_date_string(c), "toDateString", 0)
    });
    t.put_str("toTimeString", |r| {
        r.method_prop(|r, c| r.dateproto_to_time_string(c), "toTimeString", 0)
    });
    t.put_str("toLocaleString", |r| {
        r.method_prop(|r, c| r.dateproto_to_locale_string(c), "toLocaleString", 0)
    });
    t.put_str("toLocaleDateString", |r| {
        r.method_prop(
            |r, c| r.dateproto_to_locale_date_string(c),
            "toLocaleDateString",
            0,
        )
    });
    t.put_str("toLocaleTimeString", |r| {
        r.method_prop(
            |r, c| r.dateproto_to_locale_time_string(c),
            "toLocaleTimeString",
            0,
        )
    });
    t.put_str("valueOf", |r| {
        r.method_prop(|r, c| r.dateproto_value_of(c), "valueOf", 0)
    });
    t.put_str("getTime", |r| {
        r.method_prop(|r, c| r.dateproto_get_time(c), "getTime", 0)
    });
    t.put_str("getFullYear", |r| {
        r.method_prop(|r, c| r.dateproto_get_full_year(c), "getFullYear", 0)
    });
    t.put_str("getUTCFullYear", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_full_year(c), "getUTCFullYear", 0)
    });
    t.put_str("getMonth", |r| {
        r.method_prop(|r, c| r.dateproto_get_month(c), "getMonth", 0)
    });
    t.put_str("getUTCMonth", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_month(c), "getUTCMonth", 0)
    });
    t.put_str("getDate", |r| {
        r.method_prop(|r, c| r.dateproto_get_date(c), "getDate", 0)
    });
    t.put_str("getUTCDate", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_date(c), "getUTCDate", 0)
    });
    t.put_str("getDay", |r| {
        r.method_prop(|r, c| r.dateproto_get_day(c), "getDay", 0)
    });
    t.put_str("getUTCDay", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_day(c), "getUTCDay", 0)
    });
    t.put_str("getHours", |r| {
        r.method_prop(|r, c| r.dateproto_get_hours(c), "getHours", 0)
    });
    t.put_str("getUTCHours", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_hours(c), "getUTCHours", 0)
    });
    t.put_str("getMinutes", |r| {
        r.method_prop(|r, c| r.dateproto_get_minutes(c), "getMinutes", 0)
    });
    t.put_str("getUTCMinutes", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_minutes(c), "getUTCMinutes", 0)
    });
    t.put_str("getSeconds", |r| {
        r.method_prop(|r, c| r.dateproto_get_seconds(c), "getSeconds", 0)
    });
    t.put_str("getUTCSeconds", |r| {
        r.method_prop(|r, c| r.dateproto_get_utc_seconds(c), "getUTCSeconds", 0)
    });
    t.put_str("getMilliseconds", |r| {
        r.method_prop(|r, c| r.dateproto_get_milliseconds(c), "getMilliseconds", 0)
    });
    t.put_str("getUTCMilliseconds", |r| {
        r.method_prop(
            |r, c| r.dateproto_get_utc_milliseconds(c),
            "getUTCMilliseconds",
            0,
        )
    });
    t.put_str("getTimezoneOffset", |r| {
        r.method_prop(
            |r, c| r.dateproto_get_timezone_offset(c),
            "getTimezoneOffset",
            0,
        )
    });
    t.put_str("setTime", |r| {
        r.method_prop(|r, c| r.dateproto_set_time(c), "setTime", 1)
    });
    t.put_str("setMilliseconds", |r| {
        r.method_prop(|r, c| r.dateproto_set_milliseconds(c), "setMilliseconds", 1)
    });
    t.put_str("setUTCMilliseconds", |r| {
        r.method_prop(
            |r, c| r.dateproto_set_utc_milliseconds(c),
            "setUTCMilliseconds",
            1,
        )
    });
    t.put_str("setSeconds", |r| {
        r.method_prop(|r, c| r.dateproto_set_seconds(c), "setSeconds", 2)
    });
    t.put_str("setUTCSeconds", |r| {
        r.method_prop(|r, c| r.dateproto_set_utc_seconds(c), "setUTCSeconds", 2)
    });
    t.put_str("setMinutes", |r| {
        r.method_prop(|r, c| r.dateproto_set_minutes(c), "setMinutes", 3)
    });
    t.put_str("setUTCMinutes", |r| {
        r.method_prop(|r, c| r.dateproto_set_utc_minutes(c), "setUTCMinutes", 3)
    });
    t.put_str("setHours", |r| {
        r.method_prop(|r, c| r.dateproto_set_hours(c), "setHours", 4)
    });
    t.put_str("setUTCHours", |r| {
        r.method_prop(|r, c| r.dateproto_set_utc_hours(c), "setUTCHours", 4)
    });
    t.put_str("setDate", |r| {
        r.method_prop(|r, c| r.dateproto_set_date(c), "setDate", 1)
    });
    t.put_str("setUTCDate", |r| {
        r.method_prop(|r, c| r.dateproto_set_utc_date(c), "setUTCDate", 1)
    });
    t.put_str("setMonth", |r| {
        r.method_prop(|r, c| r.dateproto_set_month(c), "setMonth", 2)
    });
    t.put_str("setUTCMonth", |r| {
        r.method_prop(|r, c| r.dateproto_set_utc_month(c), "setUTCMonth", 2)
    });
    t.put_str("setFullYear", |r| {
        r.method_prop(|r, c| r.dateproto_set_full_year(c), "setFullYear", 3)
    });
    t.put_str("setUTCFullYear", |r| {
        r.method_prop(|r, c| r.dateproto_set_utc_full_year(c), "setUTCFullYear", 3)
    });
    t.put_str("toUTCString", |r| {
        r.method_prop(|r, c| r.dateproto_to_utc_string(c), "toUTCString", 0)
    });
    t.put_str("toISOString", |r| {
        r.method_prop(|r, c| r.dateproto_to_iso_string(c), "toISOString", 0)
    });
    t.put_str("toJSON", |r| {
        r.method_prop(|r, c| r.dateproto_to_json(c), "toJSON", 1)
    });

    t.put_sym(SYM_TO_PRIMITIVE, |r| {
        let f = r.new_native_func(
            NativeFn::Fn(|r, c| r.dateproto_to_primitive(c)),
            "[Symbol.toPrimitive]",
            1,
        );
        value_prop(Value::Object(f), false, false, true)
    });

    t
}

pub fn get_date_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_date_proto_template)
}

impl Runtime {
    pub fn get_date(&mut self) -> ObjRef {
        if let Some(r) = self.global.date {
            return r;
        }
        let ret = self.reserve_object();
        self.global.date = Some(ret);
        let proto = self.get_date_prototype();
        let ctor = self.wrap_native_construct(
            Some(|r: &mut Runtime, args: &[Value], proto: Option<ObjRef>| {
                r.builtin_new_date(args, proto)
            }),
            ret,
            Some(proto),
        );
        self.new_templated_func_object(
            get_date_template(),
            Some(ret),
            Some(NativeFn::Fn(|r, c| r.builtin_date(c))),
            ctor,
        );
        ret
    }

    pub fn get_date_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.date_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.date_prototype = Some(ret);
        self.new_templated_object(get_date_proto_template(), Some(ret));
        ret
    }
}
