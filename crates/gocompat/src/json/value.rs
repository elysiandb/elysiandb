use super::decode::{Decode, DecodeState, unmarshal, unquote_text};
use super::encode::{Encode, EncodeState};
use super::error::{Error, Expected, Kind};
use crate::strconv;
use std::collections::BTreeMap;
use std::collections::btree_map;
use std::fmt;

pub type Map = BTreeMap<String, Value>;

#[derive(Default)]
pub enum Value {
    #[default]
    Null,
    Bool(bool),
    Number(f64),
    Int(i64),
    String(String),
    Array(Vec<Value>),
    Object(Map),
    DocList(Vec<Map>),
}

const I64_BOUND: f64 = 9_223_372_036_854_775_808.0;
const U64_BOUND: f64 = 18_446_744_073_709_551_616.0;

impl Value {
    pub fn get(&self, key: &str) -> Option<&Value> {
        match self {
            Value::Object(m) => m.get(key),
            _ => None,
        }
    }

    pub fn index(&self, i: usize) -> Option<&Value> {
        match self {
            Value::Array(a) => a.get(i),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Value::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Value::Bool(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Value::Number(f) => Some(*f),
            Value::Int(n) => Some(*n as f64),
            _ => None,
        }
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            Value::Int(n) => Some(*n),
            Value::Number(f) if f.fract() == 0.0 && *f >= -I64_BOUND && *f < I64_BOUND => {
                Some(*f as i64)
            }
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            Value::Int(n) => u64::try_from(*n).ok(),
            Value::Number(f) if f.fract() == 0.0 && *f >= 0.0 && *f < U64_BOUND => Some(*f as u64),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&Vec<Value>> {
        match self {
            Value::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&Map> {
        match self {
            Value::Object(m) => Some(m),
            _ => None,
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, Value::Null)
    }

    pub fn take(&mut self) -> Value {
        std::mem::take(self)
    }
}

impl Drop for Value {
    fn drop(&mut self) {
        let mut stack: Vec<Value> = Vec::new();
        match self {
            Value::Array(a) if !a.is_empty() => stack.append(a),
            Value::Object(m) if !m.is_empty() => stack.extend(std::mem::take(m).into_values()),
            Value::DocList(l) if !l.is_empty() => {
                for m in l.drain(..) {
                    stack.extend(m.into_values());
                }
            }
            _ => return,
        }
        while let Some(mut v) = stack.pop() {
            match &mut v {
                Value::Array(a) => stack.append(a),
                Value::Object(m) => stack.extend(std::mem::take(m).into_values()),
                Value::DocList(l) => {
                    for m in l.drain(..) {
                        stack.extend(m.into_values());
                    }
                }
                _ => {}
            }
        }
    }
}

fn scalar_copy(v: &Value) -> Option<Value> {
    match v {
        Value::Null => Some(Value::Null),
        Value::Bool(b) => Some(Value::Bool(*b)),
        Value::Number(f) => Some(Value::Number(*f)),
        Value::Int(n) => Some(Value::Int(*n)),
        Value::String(s) => Some(Value::String(s.clone())),
        _ => None,
    }
}

enum CloneFrame<'a> {
    Array(std::slice::Iter<'a, Value>, Vec<Value>),
    Object(btree_map::Iter<'a, String, Value>, Map, String),
    DocList(std::slice::Iter<'a, Map>, Vec<Map>),
    DocMap(btree_map::Iter<'a, String, Value>, Map, String),
}

enum CloneStep<'a> {
    Child(&'a Value),
    Map(&'a Map),
    Done,
}

fn open_container<'a>(stack: &mut Vec<CloneFrame<'a>>, v: &'a Value) {
    match v {
        Value::Array(a) => stack.push(CloneFrame::Array(a.iter(), Vec::with_capacity(a.len()))),
        Value::Object(m) => stack.push(CloneFrame::Object(m.iter(), Map::new(), String::new())),
        Value::DocList(l) => stack.push(CloneFrame::DocList(l.iter(), Vec::with_capacity(l.len()))),
        _ => {}
    }
}

fn attach_copy(stack: &mut [CloneFrame<'_>], v: Value) {
    match stack.last_mut() {
        Some(CloneFrame::Array(_, a)) => a.push(v),
        Some(CloneFrame::Object(_, m, k)) | Some(CloneFrame::DocMap(_, m, k)) => {
            m.insert(std::mem::take(k), v);
        }
        _ => {}
    }
}

impl Clone for Value {
    fn clone(&self) -> Value {
        if let Some(v) = scalar_copy(self) {
            return v;
        }
        let mut stack: Vec<CloneFrame<'_>> = Vec::new();
        open_container(&mut stack, self);
        loop {
            let step = match stack.last_mut() {
                None => return Value::Null,
                Some(CloneFrame::Array(it, _)) => {
                    it.next().map_or(CloneStep::Done, CloneStep::Child)
                }
                Some(CloneFrame::Object(it, _, key)) | Some(CloneFrame::DocMap(it, _, key)) => {
                    match it.next() {
                        Some((k, v)) => {
                            key.clone_from(k);
                            CloneStep::Child(v)
                        }
                        None => CloneStep::Done,
                    }
                }
                Some(CloneFrame::DocList(it, _)) => {
                    it.next().map_or(CloneStep::Done, CloneStep::Map)
                }
            };
            match step {
                CloneStep::Child(v) => match scalar_copy(v) {
                    Some(c) => attach_copy(&mut stack, c),
                    None => open_container(&mut stack, v),
                },
                CloneStep::Map(m) => {
                    stack.push(CloneFrame::DocMap(m.iter(), Map::new(), String::new()))
                }
                CloneStep::Done => {
                    let finished = match stack.pop() {
                        Some(CloneFrame::Array(_, a)) => Value::Array(a),
                        Some(CloneFrame::Object(_, m, _)) => Value::Object(m),
                        Some(CloneFrame::DocList(_, l)) => Value::DocList(l),
                        Some(CloneFrame::DocMap(_, m, _)) => {
                            if let Some(CloneFrame::DocList(_, l)) = stack.last_mut() {
                                l.push(m);
                            }
                            continue;
                        }
                        None => return Value::Null,
                    };
                    if stack.is_empty() {
                        return finished;
                    }
                    attach_copy(&mut stack, finished);
                }
            }
        }
    }
}

fn push_map_pairs<'a>(stack: &mut Vec<(&'a Value, &'a Value)>, x: &'a Map, y: &'a Map) -> bool {
    if x.len() != y.len() {
        return false;
    }
    for ((kx, vx), (ky, vy)) in x.iter().zip(y) {
        if kx != ky {
            return false;
        }
        stack.push((vx, vy));
    }
    true
}

impl PartialEq for Value {
    fn eq(&self, other: &Value) -> bool {
        let mut stack: Vec<(&Value, &Value)> = vec![(self, other)];
        while let Some(pair) = stack.pop() {
            let same = match pair {
                (Value::Null, Value::Null) => true,
                (Value::Bool(x), Value::Bool(y)) => x == y,
                (Value::Number(x), Value::Number(y)) => x == y,
                (Value::Int(x), Value::Int(y)) => x == y,
                (Value::String(x), Value::String(y)) => x == y,
                (Value::Array(x), Value::Array(y)) => {
                    stack.extend(x.iter().zip(y));
                    x.len() == y.len()
                }
                (Value::Object(x), Value::Object(y)) => push_map_pairs(&mut stack, x, y),
                (Value::DocList(x), Value::DocList(y)) => {
                    x.len() == y.len()
                        && x.iter()
                            .zip(y)
                            .all(|(mx, my)| push_map_pairs(&mut stack, mx, my))
                }
                _ => false,
            };
            if !same {
                return false;
            }
        }
        true
    }
}

enum DebugFrame<'a> {
    Array(std::slice::Iter<'a, Value>, bool),
    Object(btree_map::Iter<'a, String, Value>, bool, &'static str),
    DocList(std::slice::Iter<'a, Map>, bool),
}

impl fmt::Debug for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut stack: Vec<DebugFrame<'_>> = Vec::new();
        let mut next = Some(Item::Value(self));
        loop {
            match next.take() {
                Some(Item::Value(v)) => match v {
                    Value::Null => f.write_str("Null")?,
                    Value::Bool(b) => write!(f, "Bool({b})")?,
                    Value::Number(x) => write!(f, "Number({x:?})")?,
                    Value::Int(n) => write!(f, "Int({n})")?,
                    Value::String(s) => write!(f, "String({s:?})")?,
                    Value::Array(a) => {
                        f.write_str("Array([")?;
                        stack.push(DebugFrame::Array(a.iter(), true));
                    }
                    Value::Object(m) => {
                        f.write_str("Object({")?;
                        stack.push(DebugFrame::Object(m.iter(), true, "})"));
                    }
                    Value::DocList(l) => {
                        f.write_str("DocList([")?;
                        stack.push(DebugFrame::DocList(l.iter(), true));
                    }
                },
                Some(Item::Map(m)) => {
                    f.write_str("{")?;
                    stack.push(DebugFrame::Object(m.iter(), true, "}"));
                }
                None => {}
            }
            let Some(frame) = stack.last_mut() else {
                return Ok(());
            };
            match frame {
                DebugFrame::Array(it, first) => match it.next() {
                    Some(v) => {
                        if !*first {
                            f.write_str(", ")?;
                        }
                        *first = false;
                        next = Some(Item::Value(v));
                    }
                    None => {
                        f.write_str("])")?;
                        stack.pop();
                    }
                },
                DebugFrame::Object(it, first, close) => match it.next() {
                    Some((k, v)) => {
                        if !*first {
                            f.write_str(", ")?;
                        }
                        *first = false;
                        write!(f, "{k:?}: ")?;
                        next = Some(Item::Value(v));
                    }
                    None => {
                        f.write_str(close)?;
                        stack.pop();
                    }
                },
                DebugFrame::DocList(it, first) => match it.next() {
                    Some(m) => {
                        if !*first {
                            f.write_str(", ")?;
                        }
                        *first = false;
                        next = Some(Item::Map(m));
                    }
                    None => {
                        f.write_str("])")?;
                        stack.pop();
                    }
                },
            }
        }
    }
}

enum Frame {
    Array(Vec<Value>),
    Object(Map, String),
}

fn read_key(d: &mut DecodeState<'_>) -> String {
    let mut key = String::new();
    let _ = key.decode(d);
    let pos = d.start();
    d.advance_to(pos + 1);
    key
}

fn literal_value(d: &mut DecodeState<'_>, nested: bool) -> Option<Value> {
    let start = d.start();
    let lit = d.literal();
    match lit.first() {
        Some(b'n') => Some(Value::Null),
        Some(b't') => Some(Value::Bool(true)),
        Some(b'f') => Some(Value::Bool(false)),
        Some(b'"') => Some(Value::String(unquote_text(lit))),
        _ => {
            if d.wants_exact_integers()
                && !lit.iter().any(|&c| c == b'.' || c == b'e' || c == b'E')
                && let Ok(n) = strconv::parse_int(lit, 10, 64)
            {
                return Some(Value::Int(n));
            }
            match d.number_value(start, lit) {
                Some(f) => Some(Value::Number(f)),
                None if nested => Some(Value::Null),
                None => None,
            }
        }
    }
}

fn decode_tree(d: &mut DecodeState<'_>) -> Value {
    let data = d.data();
    let mut stack: Vec<Frame> = Vec::new();
    loop {
        let start = d.start();
        let mut value = match data.get(start) {
            Some(b'{') => {
                d.advance_to(start + 1);
                let p = d.start();
                if data.get(p) == Some(&b'}') {
                    d.advance_to(p + 1);
                    Value::Object(Map::new())
                } else {
                    let key = read_key(d);
                    stack.push(Frame::Object(Map::new(), key));
                    continue;
                }
            }
            Some(b'[') => {
                d.advance_to(start + 1);
                let p = d.start();
                if data.get(p) == Some(&b']') {
                    d.advance_to(p + 1);
                    Value::Array(Vec::new())
                } else {
                    stack.push(Frame::Array(Vec::new()));
                    continue;
                }
            }
            _ => literal_value(d, true).unwrap_or(Value::Null),
        };
        loop {
            let Some(frame) = stack.last_mut() else {
                return value;
            };
            let p = d.start();
            let more = data.get(p) == Some(&b',');
            d.advance_to(p + 1);
            match frame {
                Frame::Array(a) => {
                    a.push(value);
                    if more {
                        break;
                    }
                    let Some(Frame::Array(a)) = stack.pop() else {
                        return Value::Null;
                    };
                    value = Value::Array(a);
                }
                Frame::Object(m, key) => {
                    m.insert(std::mem::take(key), value);
                    if more {
                        *key = read_key(d);
                        break;
                    }
                    let Some(Frame::Object(m, _)) = stack.pop() else {
                        return Value::Null;
                    };
                    value = Value::Object(m);
                }
            }
        }
    }
}

impl Decode for Value {
    const EXPECTED: Expected = Expected::Object;

    fn decode(&mut self, d: &mut DecodeState<'_>) -> Result<(), Error> {
        match d.peek() {
            Kind::Object | Kind::Array => *self = decode_tree(d),
            _ => {
                if let Some(v) = literal_value(d, false) {
                    *self = v;
                }
            }
        }
        Ok(())
    }
}

enum Item<'a> {
    Value(&'a Value),
    Map(&'a Map),
}

enum EncodeFrame<'a> {
    Array(std::slice::Iter<'a, Value>, bool),
    Object(btree_map::Iter<'a, String, Value>, bool),
    DocList(std::slice::Iter<'a, Map>, bool),
}

impl Encode for Value {
    fn encode(&self, e: &mut EncodeState) -> Result<(), Error> {
        let mut stack: Vec<EncodeFrame<'_>> = Vec::new();
        let mut next = Some(Item::Value(self));
        loop {
            match next.take() {
                Some(Item::Value(v)) => match v {
                    Value::Null => e.write_null(),
                    Value::Bool(b) => e.write_bool(*b),
                    Value::Number(f) => e.write_f64(*f)?,
                    Value::Int(n) => e.write_i64(*n),
                    Value::String(s) => e.write_string(s.as_bytes()),
                    Value::Array(a) => {
                        e.write_byte(b'[');
                        stack.push(EncodeFrame::Array(a.iter(), true));
                    }
                    Value::Object(m) => {
                        e.write_byte(b'{');
                        stack.push(EncodeFrame::Object(m.iter(), true));
                    }
                    Value::DocList(l) => {
                        e.write_byte(b'[');
                        stack.push(EncodeFrame::DocList(l.iter(), true));
                    }
                },
                Some(Item::Map(m)) => {
                    e.write_byte(b'{');
                    stack.push(EncodeFrame::Object(m.iter(), true));
                }
                None => {}
            }
            let Some(frame) = stack.last_mut() else {
                return Ok(());
            };
            match frame {
                EncodeFrame::Array(it, first) => match it.next() {
                    Some(v) => {
                        if !*first {
                            e.write_byte(b',');
                        }
                        *first = false;
                        next = Some(Item::Value(v));
                    }
                    None => {
                        e.write_byte(b']');
                        stack.pop();
                    }
                },
                EncodeFrame::Object(it, first) => match it.next() {
                    Some((k, v)) => {
                        if !*first {
                            e.write_byte(b',');
                        }
                        *first = false;
                        e.write_string(k.as_bytes());
                        e.write_byte(b':');
                        next = Some(Item::Value(v));
                    }
                    None => {
                        e.write_byte(b'}');
                        stack.pop();
                    }
                },
                EncodeFrame::DocList(it, first) => match it.next() {
                    Some(m) => {
                        if !*first {
                            e.write_byte(b',');
                        }
                        *first = false;
                        next = Some(Item::Map(m));
                    }
                    None => {
                        e.write_byte(b']');
                        stack.pop();
                    }
                },
            }
        }
    }

    fn is_empty_value(&self) -> bool {
        self.is_null()
    }
}

pub fn unmarshal_value(data: &[u8]) -> Result<Value, Error> {
    let mut v = Value::Null;
    unmarshal(data, &mut v)?;
    Ok(v)
}

pub fn unmarshal_value_with_integers(data: &[u8]) -> Result<Value, Error> {
    super::scanner::check_valid(data)?;
    let mut d = DecodeState::new(data);
    d.exact_integers(true);
    let mut v = Value::Null;
    v.decode(&mut d)?;
    d.finish()?;
    Ok(v)
}
