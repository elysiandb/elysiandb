use super::{DocMap, DocValue, SliceHeader, TypedSlice};
use gocompat::json::{self, EncodeState};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub const ENCODE_PREFIX: &str = "cannot encode JSON: ";
pub const ERR_CYCLE: &str = "the value contains a cycle";
pub const ERR_FUNC: &str = "a function is not a valid JSON value";

fn float_error(f: f64) -> String {
    let what = if f.is_nan() {
        "NaN"
    } else if f > 0.0 {
        "Infinity"
    } else {
        "-Infinity"
    };
    format!("{ENCODE_PREFIX}{what} is not a valid JSON number")
}

enum Enc {
    Value(DocValue),
    Raw(&'static [u8]),
    Key(String),
    Leave(usize),
}

fn path_key(v: &DocValue) -> Option<usize> {
    match v {
        DocValue::Map(m) => Some(Rc::as_ptr(m) as *const () as usize),
        DocValue::Slice(h) if h.len > 0 => Some(Rc::as_ptr(&h.backing) as *const () as usize),
        DocValue::SlicePtr(p) => Some(Rc::as_ptr(p) as *const () as usize),
        DocValue::Pairs(h) if h.len > 0 => Some(Rc::as_ptr(&h.backing) as *const () as usize),
        _ => None,
    }
}

fn push_items(stack: &mut Vec<Enc>, items: Vec<DocValue>) {
    stack.push(Enc::Raw(b"]"));
    for (i, it) in items.into_iter().enumerate().rev() {
        stack.push(Enc::Value(it));
        if i > 0 {
            stack.push(Enc::Raw(b","));
        }
    }
}

fn pair_items(p: &super::Pair) -> Vec<DocValue> {
    vec![p[0].clone(), p[1].clone()]
}

pub fn marshal(v: &DocValue) -> Result<Vec<u8>, String> {
    let mut e = EncodeState::new(true);
    let mut stack = vec![Enc::Value(v.clone())];
    let mut path: Vec<usize> = Vec::new();
    while let Some(item) = stack.pop() {
        let v = match item {
            Enc::Raw(b) => {
                e.write_raw(b);
                continue;
            }
            Enc::Key(k) => {
                e.write_string(k.as_bytes());
                e.write_byte(b':');
                continue;
            }
            Enc::Leave(p) => {
                if path.last() == Some(&p) {
                    path.pop();
                }
                continue;
            }
            Enc::Value(v) => v,
        };
        if let Some(p) = path_key(&v) {
            if path.contains(&p) {
                return Err(format!("{ENCODE_PREFIX}{ERR_CYCLE}"));
            }
            path.push(p);
            stack.push(Enc::Leave(p));
        }
        match &v {
            DocValue::Nil => e.write_null(),
            DocValue::Bool(b) => e.write_bool(*b),
            DocValue::Int64(i) => e.write_i64(*i),
            DocValue::Float64(f) => e.write_f64(*f).map_err(|_| float_error(*f))?,
            DocValue::String(s) => e.write_string(s.as_bytes()),
            DocValue::Map(m) => {
                e.write_byte(b'{');
                stack.push(Enc::Raw(b"}"));
                let entries: Vec<(String, DocValue)> = m
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (i, (k, v)) in entries.into_iter().enumerate().rev() {
                    stack.push(Enc::Value(v));
                    stack.push(Enc::Key(k));
                    if i > 0 {
                        stack.push(Enc::Raw(b","));
                    }
                }
            }
            DocValue::Slice(h) => {
                e.write_byte(b'[');
                push_items(&mut stack, h.to_vec());
            }
            DocValue::SlicePtr(p) => {
                let h = p.borrow().clone();
                e.write_byte(b'[');
                push_items(&mut stack, h.to_vec());
            }
            DocValue::Time(t) => {
                let mut b = Vec::new();
                super::gotime::marshal_json(t, &mut b)
                    .map_err(|err| format!("{ENCODE_PREFIX}{err}"))?;
                e.write_raw(&b);
            }
            DocValue::Pairs(h) => {
                e.write_byte(b'[');
                stack.push(Enc::Raw(b"]"));
                let pairs = h.to_vec();
                for (i, p) in pairs.iter().enumerate().rev() {
                    stack.push(Enc::Raw(b"]"));
                    stack.push(Enc::Value(p[1].clone()));
                    stack.push(Enc::Raw(b","));
                    stack.push(Enc::Value(p[0].clone()));
                    stack.push(Enc::Raw(b"["));
                    if i > 0 {
                        stack.push(Enc::Raw(b","));
                    }
                }
            }
            DocValue::PairRef(c) => {
                e.write_byte(b'[');
                push_items(&mut stack, pair_items(&c.get()));
            }
            DocValue::Bytes(h) => {
                let b = h.to_vec();
                let s = gocompat::base64::STD_ENCODING.encode_to_string(&b);
                e.write_string(s.as_bytes());
            }
            DocValue::Int8s(h) => write_ints(&mut e, h.to_vec().into_iter().map(i64::from)),
            DocValue::Uint16s(h) => write_ints(&mut e, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int16s(h) => write_ints(&mut e, h.to_vec().into_iter().map(i64::from)),
            DocValue::Uint32s(h) => write_ints(&mut e, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int32s(h) => write_ints(&mut e, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int64s(h) => write_ints(&mut e, h.to_vec().into_iter()),
            DocValue::Uint64s(h) => {
                e.write_byte(b'[');
                for (i, x) in h.to_vec().into_iter().enumerate() {
                    if i > 0 {
                        e.write_byte(b',');
                    }
                    e.write_u64(x);
                }
                e.write_byte(b']');
            }
            DocValue::Float32s(h) => {
                e.write_byte(b'[');
                for (i, x) in h.to_vec().into_iter().enumerate() {
                    if i > 0 {
                        e.write_byte(b',');
                    }
                    e.write_f32(x).map_err(|_| float_error(f64::from(x)))?;
                }
                e.write_byte(b']');
            }
            DocValue::Float64s(h) => {
                e.write_byte(b'[');
                for (i, x) in h.to_vec().into_iter().enumerate() {
                    if i > 0 {
                        e.write_byte(b',');
                    }
                    e.write_f64(x).map_err(|_| float_error(x))?;
                }
                e.write_byte(b']');
            }
            DocValue::BigInt(b) => e.write_raw(b.string().as_bytes()),
            DocValue::Func(_) => return Err(format!("{ENCODE_PREFIX}{ERR_FUNC}")),
            DocValue::Host(h) => {
                let mut b = Vec::new();
                h.marshal_json(&mut b)
                    .map_err(|err| format!("{ENCODE_PREFIX}{err}"))?;
                e.write_raw(&b);
            }
            DocValue::Object(_) => e.write_raw(b"{}"),
        }
    }
    Ok(e.into_bytes())
}

fn write_ints(e: &mut EncodeState, it: impl Iterator<Item = i64>) {
    e.write_byte(b'[');
    for (i, x) in it.enumerate() {
        if i > 0 {
            e.write_byte(b',');
        }
        e.write_i64(x);
    }
    e.write_byte(b']');
}

enum Fmt {
    Value(DocValue),
    Raw(&'static str),
    Key(String),
}

pub fn format_v(v: &DocValue) -> String {
    let mut out: Vec<u8> = Vec::new();
    let mut stack = vec![Fmt::Value(v.clone())];
    while let Some(item) = stack.pop() {
        let v = match item {
            Fmt::Raw(s) => {
                out.extend_from_slice(s.as_bytes());
                continue;
            }
            Fmt::Key(k) => {
                out.extend_from_slice(k.as_bytes());
                out.push(b':');
                continue;
            }
            Fmt::Value(v) => v,
        };
        match &v {
            DocValue::Nil => out.extend_from_slice(b"<nil>"),
            DocValue::Bool(b) => out.extend_from_slice(if *b { b"true" } else { b"false" }),
            DocValue::Int64(i) => out.extend_from_slice(i.to_string().as_bytes()),
            DocValue::Float64(f) => {
                gocompat::strconv::append_float(&mut out, *f, b'g', -1, 64);
            }
            DocValue::String(s) => out.extend_from_slice(s.as_bytes()),
            DocValue::Map(m) => {
                out.extend_from_slice(b"map[");
                stack.push(Fmt::Raw("]"));
                let entries: Vec<(String, DocValue)> = m
                    .borrow()
                    .iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect();
                for (i, (k, v)) in entries.into_iter().enumerate().rev() {
                    stack.push(Fmt::Value(v));
                    stack.push(Fmt::Key(k));
                    if i > 0 {
                        stack.push(Fmt::Raw(" "));
                    }
                }
            }
            DocValue::Slice(h) => push_fmt_items(&mut out, &mut stack, h.to_vec()),
            DocValue::SlicePtr(p) => {
                out.push(b'&');
                let h = p.borrow().clone();
                push_fmt_items(&mut out, &mut stack, h.to_vec());
            }
            DocValue::Pairs(h) => {
                let items = h.to_vec().iter().map(pair_slice).collect();
                push_fmt_items(&mut out, &mut stack, items);
            }
            DocValue::PairRef(c) => {
                out.push(b'&');
                push_fmt_items(&mut out, &mut stack, pair_items(&c.get()));
            }
            DocValue::Time(t) => out.extend_from_slice(t.to_string().as_bytes()),
            DocValue::Bytes(h) => fmt_nums(&mut out, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int8s(h) => fmt_nums(&mut out, h.to_vec().into_iter().map(i64::from)),
            DocValue::Uint16s(h) => fmt_nums(&mut out, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int16s(h) => fmt_nums(&mut out, h.to_vec().into_iter().map(i64::from)),
            DocValue::Uint32s(h) => fmt_nums(&mut out, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int32s(h) => fmt_nums(&mut out, h.to_vec().into_iter().map(i64::from)),
            DocValue::Int64s(h) => fmt_nums(&mut out, h.to_vec().into_iter()),
            DocValue::Uint64s(h) => {
                let items = h.to_vec().into_iter().map(|x| x.to_string()).collect();
                fmt_strs(&mut out, items);
            }
            DocValue::Float32s(h) => {
                let items = h
                    .to_vec()
                    .into_iter()
                    .map(|x| gocompat::strconv::format_float(f64::from(x), b'g', -1, 32))
                    .collect();
                fmt_strs(&mut out, items);
            }
            DocValue::Float64s(h) => {
                let items = h
                    .to_vec()
                    .into_iter()
                    .map(|x| gocompat::strconv::format_float(x, b'g', -1, 64))
                    .collect();
                fmt_strs(&mut out, items);
            }
            DocValue::BigInt(b) => out.extend_from_slice(b.string().as_bytes()),
            DocValue::Func(_) => out.extend_from_slice(b"0x0"),
            DocValue::Host(h) => h.format_v(&mut out),
            DocValue::Object(_) => out.extend_from_slice(b"{0x0}"),
        }
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn pair_slice(p: &super::Pair) -> DocValue {
    DocValue::Slice(SliceHeader::new(pair_items(p)))
}

fn push_fmt_items(out: &mut Vec<u8>, stack: &mut Vec<Fmt>, items: Vec<DocValue>) {
    out.push(b'[');
    stack.push(Fmt::Raw("]"));
    for (i, it) in items.into_iter().enumerate().rev() {
        stack.push(Fmt::Value(it));
        if i > 0 {
            stack.push(Fmt::Raw(" "));
        }
    }
}

fn fmt_nums(out: &mut Vec<u8>, it: impl Iterator<Item = i64>) {
    fmt_strs(out, it.map(|x| x.to_string()).collect());
}

fn fmt_strs(out: &mut Vec<u8>, items: Vec<String>) {
    out.push(b'[');
    out.extend_from_slice(items.join(" ").as_bytes());
    out.push(b']');
}

enum Dest {
    Root,
    Map(Rc<RefCell<DocMap>>, String),
    Items(usize),
}

pub fn from_json(v: &json::Value) -> DocValue {
    enum Work<'a> {
        Value(&'a json::Value, Dest),
        Close(usize),
    }
    let mut root = DocValue::Nil;
    let mut lists: Vec<Vec<DocValue>> = Vec::new();
    let mut parents: Vec<Dest> = Vec::new();
    let mut work = vec![Work::Value(v, Dest::Root)];
    let place = |root: &mut DocValue, lists: &mut Vec<Vec<DocValue>>, d: Dest, x: DocValue| match d
    {
        Dest::Root => *root = x,
        Dest::Map(m, k) => {
            m.borrow_mut().insert(k, x);
        }
        Dest::Items(i) => lists[i].push(x),
    };
    while let Some(w) = work.pop() {
        match w {
            Work::Close(i) => {
                let items = std::mem::take(&mut lists[i]);
                let d = parents.pop().unwrap_or(Dest::Root);
                let x = DocValue::Slice(TypedSlice::decoded(items));
                place(&mut root, &mut lists, d, x);
            }
            Work::Value(v, d) => match v {
                json::Value::Null => place(&mut root, &mut lists, d, DocValue::Nil),
                json::Value::Bool(b) => place(&mut root, &mut lists, d, DocValue::Bool(*b)),
                json::Value::Number(f) => place(&mut root, &mut lists, d, DocValue::Float64(*f)),
                json::Value::Int(i) => place(&mut root, &mut lists, d, DocValue::Int64(*i)),
                json::Value::String(s) => place(
                    &mut root,
                    &mut lists,
                    d,
                    DocValue::String(Arc::from(s.as_str())),
                ),
                json::Value::Object(m) => {
                    let target = Rc::new(RefCell::new(DocMap::new()));
                    for (k, item) in m.iter().rev() {
                        work.push(Work::Value(item, Dest::Map(target.clone(), k.clone())));
                    }
                    place(&mut root, &mut lists, d, DocValue::Map(target));
                }
                json::Value::Array(a) => {
                    let i = lists.len();
                    lists.push(Vec::with_capacity(a.len()));
                    parents.push(d);
                    work.push(Work::Close(i));
                    for item in a.iter().rev() {
                        work.push(Work::Value(item, Dest::Items(i)));
                    }
                }
                json::Value::DocList(l) => {
                    let i = lists.len();
                    lists.push(Vec::with_capacity(l.len()));
                    parents.push(d);
                    work.push(Work::Close(i));
                    let targets: Vec<Rc<RefCell<DocMap>>> = l
                        .iter()
                        .map(|_| Rc::new(RefCell::new(DocMap::new())))
                        .collect();
                    for t in &targets {
                        lists[i].push(DocValue::Map(t.clone()));
                    }
                    for (m, target) in l.iter().zip(targets).rev() {
                        for (k, item) in m.iter().rev() {
                            work.push(Work::Value(item, Dest::Map(target.clone(), k.clone())));
                        }
                    }
                }
            },
        }
    }
    root
}
