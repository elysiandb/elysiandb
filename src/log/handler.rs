use super::buffer::record;
use super::log::{ERROR_COUNT, FORMAT_JSON, Level, STATE, WARNING_COUNT, current_level_value};
use super::value::{Attr, AttrValue, Value};
use crate::gocompat::json::{Encode, EncodeState};
use crate::gocompat::time::{RFC3339_NANO, Time};
use crate::gocompat::utf8::{RUNE_ERROR, decode_rune};
use crate::gocompat::{strconv, unicode};
use std::borrow::Cow;
use std::fs::File;
use std::io::{ErrorKind, Write};
use std::mem::ManuallyDrop;
use std::os::fd::FromRawFd;
use std::sync::atomic::Ordering;

const RESET: &[u8] = b"\x1b[0m";
const RED: &[u8] = b"\x1b[31m";
const YELLOW: &[u8] = b"\x1b[33m";
const MAGENTA: &[u8] = b"\x1b[35m";
const CYAN: &[u8] = b"\x1b[36m";

const TEXT_TIME_FORMAT: &str = "2006-01-02T15:04:05.000Z07:00";

pub(super) struct Field<'a> {
    pub(super) key: Cow<'a, str>,
    pub(super) value: AttrValue,
}

pub(super) fn handler_enabled(level: Level) -> bool {
    level >= current_level_value()
}

pub(super) fn handle(level: Level, msg: &str, component: Option<&str>, attrs: &[Attr<'_>]) {
    let t = Time::now();

    let logger_attrs: [Attr<'_>; 1];
    let mut fields = Vec::with_capacity(attrs.len() + 1);
    if let Some(c) = component {
        logger_attrs = [("component", Value::Str(c.as_bytes()))];
        append_fields(&mut fields, &logger_attrs);
    }
    append_fields(&mut fields, attrs);

    if level >= Level::ERROR {
        ERROR_COUNT.fetch_add(1, Ordering::Relaxed);
    } else if level >= Level::WARN {
        WARNING_COUNT.fetch_add(1, Ordering::Relaxed);
    }

    record(&t, level, msg, &fields);

    let mut state = STATE.lock();
    let line = if state.format == FORMAT_JSON {
        json_line(&t, level, msg, &fields)
    } else {
        text_line(&t, level, msg, &fields, state.color)
    };

    match state.out.as_mut() {
        Some(w) => {
            let _ = w.write_all(&line);
        }
        None => write_stdout(&line),
    }
}

fn write_stdout(line: &[u8]) {
    let mut out = ManuallyDrop::new(unsafe { File::from_raw_fd(libc::STDOUT_FILENO) });
    if let Err(e) = out.write_all(line)
        && e.kind() == ErrorKind::BrokenPipe
    {
        unsafe {
            libc::signal(libc::SIGPIPE, libc::SIG_DFL);
            libc::raise(libc::SIGPIPE);
        }
    }
}

fn append_fields<'a>(fields: &mut Vec<Field<'a>>, attrs: &'a [Attr<'a>]) {
    let mut stack: Vec<(Cow<'a, str>, std::slice::Iter<'a, Attr<'a>>)> =
        vec![(Cow::Borrowed(""), attrs.iter())];
    while let Some((prefix, iter)) = stack.last_mut() {
        let Some((key, value)) = iter.next() else {
            stack.pop();
            continue;
        };
        let is_group = matches!(value, Value::Group(_));
        if key.is_empty() && !is_group {
            continue;
        }
        let full: Cow<'a, str> = if prefix.is_empty() {
            Cow::Borrowed(*key)
        } else {
            Cow::Owned(format!("{prefix}.{key}"))
        };
        if let Value::Group(members) = value {
            stack.push((full, members.iter()));
            continue;
        }
        fields.push(Field {
            key: full,
            value: plain(value),
        });
    }
}

fn plain(v: &Value<'_>) -> AttrValue {
    match v {
        Value::Nil => AttrValue::Nil,
        Value::Str(s) => AttrValue::Str(s.to_vec()),
        Value::Int(n) => AttrValue::Int(*n),
        Value::Uint(n) => AttrValue::Uint(*n),
        Value::Float(f) => {
            if f.is_nan() || f.is_infinite() {
                AttrValue::Str(strconv::format_float(*f, b'g', -1, 64).into_bytes())
            } else {
                AttrValue::Float(*f)
            }
        }
        Value::Bool(b) => AttrValue::Bool(*b),
        Value::Duration(d) => AttrValue::Str(d.to_string().into_bytes()),
        Value::Time(t) => AttrValue::Str(t.format(RFC3339_NANO).into_bytes()),
        Value::Display(d) => AttrValue::Str(d.to_string().into_bytes()),
        Value::Strings(list) => AttrValue::Strings(list.iter().map(|s| s.to_vec()).collect()),
        Value::Json(j) => AttrValue::Str(j.json_text()),
        Value::Group(_) => AttrValue::Nil,
    }
}

pub(super) fn level_label(level: Level) -> &'static str {
    if level < Level::INFO {
        "DEBUG"
    } else if level < Level::WARN {
        "INFO"
    } else if level < Level::ERROR {
        "WARN"
    } else {
        "ERROR"
    }
}

fn level_color(level: Level) -> &'static [u8] {
    if level < Level::INFO {
        MAGENTA
    } else if level < Level::WARN {
        CYAN
    } else if level < Level::ERROR {
        YELLOW
    } else {
        RED
    }
}

fn text_line(t: &Time, level: Level, msg: &str, fields: &[Field<'_>], color: bool) -> Vec<u8> {
    let mut b = Vec::with_capacity(128);
    b.extend_from_slice(b"time=");
    t.append_format(&mut b, TEXT_TIME_FORMAT);
    b.extend_from_slice(b" level=");
    if color {
        b.extend_from_slice(level_color(level));
        b.extend_from_slice(level_label(level).as_bytes());
        b.extend_from_slice(RESET);
    } else {
        b.extend_from_slice(level_label(level).as_bytes());
    }
    b.extend_from_slice(b" msg=");
    append_quote_if_needed(&mut b, msg.as_bytes());

    for f in fields {
        b.push(b' ');
        append_quote_if_needed(&mut b, f.key.as_bytes());
        b.push(b'=');
        append_quote_if_needed(&mut b, &text_value(&f.value));
    }

    b.push(b'\n');
    b
}

pub(super) fn text_value(v: &AttrValue) -> Cow<'_, [u8]> {
    match v {
        AttrValue::Nil => Cow::Borrowed(b"null"),
        AttrValue::Str(s) => Cow::Borrowed(s),
        AttrValue::Int(n) => Cow::Owned(strconv::format_int(*n, 10).into_bytes()),
        AttrValue::Uint(n) => Cow::Owned(strconv::format_uint(*n, 10).into_bytes()),
        AttrValue::Float(f) => Cow::Owned(strconv::format_float(*f, b'f', -1, 64).into_bytes()),
        AttrValue::Bool(b) => Cow::Borrowed(if *b { b"true" } else { b"false" }),
        AttrValue::Strings(list) => {
            let mut e = EncodeState::new(false);
            e.write_byte(b'[');
            for (i, s) in list.iter().enumerate() {
                if i > 0 {
                    e.write_byte(b',');
                }
                e.write_string(s);
            }
            e.write_byte(b']');
            Cow::Owned(e.into_bytes())
        }
    }
}

fn append_quote_if_needed(b: &mut Vec<u8>, s: &[u8]) {
    if s.is_empty() {
        b.extend_from_slice(b"\"\"");
        return;
    }

    let mut i = 0;
    while i < s.len() {
        let (r, size) = decode_rune(&s[i..]);
        if r == RUNE_ERROR
            || r <= ' ' as i32
            || r == '=' as i32
            || r == '"' as i32
            || r == '\\' as i32
            || !unicode::is_print(r)
        {
            strconv::append_quote(b, s);
            return;
        }
        i += size;
    }

    b.extend_from_slice(s);
}

fn json_line(t: &Time, level: Level, msg: &str, fields: &[Field<'_>]) -> Vec<u8> {
    let mut e = EncodeState::new(true);
    e.write_raw(b"{\"time\":");
    e.write_string(t.format(RFC3339_NANO).as_bytes());
    e.write_raw(b",\"level\":");
    e.write_string(level_label(level).as_bytes());
    e.write_raw(b",\"msg\":");
    e.write_string(msg.as_bytes());

    for f in fields {
        e.write_byte(b',');
        e.write_string(f.key.as_bytes());
        e.write_byte(b':');
        write_json(&mut e, &f.value);
    }

    e.write_raw(b"}\n");
    e.into_bytes()
}

fn write_json(e: &mut EncodeState, v: &AttrValue) {
    let mut inner = EncodeState::new(true);
    if v.encode(&mut inner).is_ok() {
        e.write_raw(inner.bytes());
        return;
    }
    if let AttrValue::Float(f) = v {
        e.write_string(strconv::format_float(*f, b'g', -1, 64).as_bytes());
    }
}
