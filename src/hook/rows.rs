use std::any::Any;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use talos::export::string_from_go_shared;
use talos::heap::Tracer;
use talos::host::{
    DocMap, DocValue, DynamicArray, DynamicObject, GoMethod, GoMethods, SliceHeader,
};
use talos::object::FunctionCall;
use talos::runtime::{JsResult, Runtime};
use talos::value::{Value, float_to_value, int_to_value};

use crate::gocompat::json::{self, Map};

use super::results::QueryResult;

#[derive(Clone, Debug)]
pub(crate) enum Cell {
    Null,
    Bool(bool),
    Num(f64),
    Str(Arc<str>),
    Nested(json::Value),
}

#[derive(Clone)]
pub(crate) struct Row {
    pub(crate) keys: Arc<[Arc<str>]>,
    pub(crate) vals: Arc<[Cell]>,
    pub(crate) seq: u32,
    pub(crate) size: u32,
    pub(crate) lsn: u64,
}

impl Row {
    pub(crate) fn find(&self, key: &str) -> Option<usize> {
        self.keys
            .iter()
            .position(|k| k.len() == key.len() && &**k == key)
    }
}

const INTERNED_MAX_LEN: usize = 32;
const INTERNED_MAX_COUNT: usize = 1 << 16;
const ROW_OVERHEAD: i64 = 48;
const FIELD_OVERHEAD: i64 = 24;
const NESTED_FACTOR: i64 = 4;

#[derive(Default)]
pub(crate) struct RowBuilder {
    shapes: HashMap<String, Arc<[Arc<str>]>>,
    strings: HashMap<Arc<str>, ()>,
    pub(crate) bytes: i64,
}

impl RowBuilder {
    pub(crate) fn new() -> RowBuilder {
        RowBuilder::default()
    }

    fn shape(&mut self, doc: &Map) -> Arc<[Arc<str>]> {
        let mut sig = String::new();
        for (i, k) in doc.keys().enumerate() {
            if i > 0 {
                sig.push('\0');
            }
            sig.push_str(k);
        }

        if let Some(shared) = self.shapes.get(&sig) {
            return shared.clone();
        }

        let keys: Arc<[Arc<str>]> = doc.keys().map(|k| Arc::from(k.as_str())).collect();
        self.bytes += sig.len() as i64 + keys.len() as i64 * 16;
        self.shapes.insert(sig, keys.clone());

        keys
    }

    fn str(&mut self, s: &str) -> Arc<str> {
        if s.len() <= INTERNED_MAX_LEN
            && let Some((v, _)) = self.strings.get_key_value(s)
        {
            return v.clone();
        }

        let v: Arc<str> = Arc::from(s);
        self.bytes += s.len() as i64 + 16;
        if s.len() <= INTERNED_MAX_LEN && self.strings.len() < INTERNED_MAX_COUNT {
            self.strings.insert(v.clone(), ());
        }

        v
    }

    pub(crate) fn build(&mut self, doc: &Map, seq: u32, lsn: u64, size: usize) -> Row {
        let before = self.bytes;
        let keys = self.shape(doc);
        self.bytes += ROW_OVERHEAD + keys.len() as i64 * FIELD_OVERHEAD;

        let mut nested = false;
        let mut vals: Vec<Cell> = Vec::with_capacity(keys.len());
        for v in doc.values() {
            let cell = match v {
                json::Value::Null => Cell::Null,
                json::Value::String(s) => Cell::Str(self.str(s)),
                json::Value::Number(f) => Cell::Num(*f),
                json::Value::Int(i) => Cell::Num(*i as f64),
                json::Value::Bool(b) => Cell::Bool(*b),
                other => {
                    if !nested {
                        nested = true;
                        self.bytes += keys.len() as i64 * 16;
                    }
                    Cell::Nested(other.clone())
                }
            };
            vals.push(cell);
        }

        if nested {
            self.bytes += size as i64 * NESTED_FACTOR;
        }

        let grown = (self.bytes - before).min(1 << 31);

        Row {
            keys,
            vals: vals.into(),
            seq,
            size: grown as u32,
            lsn,
        }
    }
}

pub(crate) fn scalar_value(c: &Cell) -> Value {
    match c {
        Cell::Null | Cell::Nested(_) => Value::Null,
        Cell::Bool(b) => Value::Bool(*b),
        Cell::Num(f) => float_to_value(*f),
        Cell::Str(s) => Value::String(string_from_go_shared(s)),
    }
}

pub(crate) fn doc_from_json(v: &json::Value) -> DocValue {
    enum Dest {
        Root,
        Map(Rc<RefCell<DocMap>>, String),
        Item(Rc<RefCell<Vec<DocValue>>>, usize),
    }

    let mut root = DocValue::Nil;
    let mut work: Vec<(&json::Value, Dest)> = vec![(v, Dest::Root)];
    while let Some((v, d)) = work.pop() {
        let x = match v {
            json::Value::Null => DocValue::Nil,
            json::Value::Bool(b) => DocValue::Bool(*b),
            json::Value::Number(f) => DocValue::Float64(*f),
            json::Value::Int(i) => DocValue::Int64(*i),
            json::Value::String(s) => DocValue::String(Arc::from(s.as_str())),
            json::Value::Object(m) => {
                let target = Rc::new(RefCell::new(DocMap::new()));
                for (k, item) in m.iter() {
                    target.borrow_mut().insert(k.clone(), DocValue::Nil);
                    work.push((item, Dest::Map(target.clone(), k.clone())));
                }
                DocValue::Map(target)
            }
            json::Value::Array(a) => {
                let h = SliceHeader::new(vec![DocValue::Nil; a.len()]);
                for (i, item) in a.iter().enumerate() {
                    work.push((item, Dest::Item(h.backing.clone(), i)));
                }
                DocValue::Slice(h)
            }
            json::Value::DocList(l) => {
                let mut items = Vec::with_capacity(l.len());
                for m in l {
                    let target = Rc::new(RefCell::new(DocMap::new()));
                    for (k, item) in m.iter() {
                        target.borrow_mut().insert(k.clone(), DocValue::Nil);
                        work.push((item, Dest::Map(target.clone(), k.clone())));
                    }
                    items.push(DocValue::Map(target));
                }
                DocValue::Slice(SliceHeader::new(items))
            }
        };
        match d {
            Dest::Root => root = x,
            Dest::Map(m, k) => {
                m.borrow_mut().insert(k, x);
            }
            Dest::Item(b, i) => b.borrow_mut()[i] = x,
        }
    }

    root
}

#[derive(Default)]
struct RowState {
    over: HashMap<String, Value>,
    deleted: HashSet<String>,
    added: Vec<String>,
}

pub(crate) struct RowView {
    row: Row,
    runtime: std::cell::Cell<u64>,
    st: RefCell<RowState>,
}

impl RowView {
    pub(crate) fn new(rt: &Runtime, row: Row) -> RowView {
        RowView {
            row,
            runtime: std::cell::Cell::new(rt.host.id()),
            st: RefCell::new(RowState::default()),
        }
    }

    fn bind(&self, rt: &Runtime) {
        if self.runtime.get() == rt.host.id() {
            return;
        }
        self.runtime.set(rt.host.id());
        let mut st = self.st.borrow_mut();
        st.over.clear();
        st.added.clear();
    }

    fn over(&self, key: &str) -> Option<Value> {
        self.st.borrow().over.get(key).cloned()
    }

    fn base(&self, rt: &mut Runtime, key: &str) -> JsResult<Option<Value>> {
        let Some(i) = self.row.find(key) else {
            return Ok(None);
        };

        if let Cell::Nested(v) = &self.row.vals[i] {
            let copied = doc_from_json(v);
            let val = rt.to_value(&copied)?;
            self.st
                .borrow_mut()
                .over
                .insert(key.to_string(), val.clone());
            return Ok(Some(val));
        }

        Ok(Some(scalar_value(&self.row.vals[i])))
    }

    fn get_key(&self, rt: &mut Runtime, key: &str) -> JsResult<Option<Value>> {
        self.bind(rt);
        if let Some(v) = self.over(key) {
            return Ok(Some(v));
        }

        if self.st.borrow().deleted.contains(key) {
            return Ok(None);
        }

        self.base(rt, key)
    }

    fn set_key(&self, key: &str, val: Value) -> bool {
        let mut st = self.st.borrow_mut();
        if !st.over.contains_key(key) && self.row.find(key).is_none() {
            st.added.push(key.to_string());
        }

        st.deleted.remove(key);
        st.over.insert(key.to_string(), val);

        true
    }

    fn has_key(&self, key: &str) -> bool {
        let st = self.st.borrow();
        if st.over.contains_key(key) {
            return true;
        }

        !st.deleted.contains(key) && self.row.find(key).is_some()
    }

    fn delete_key(&self, key: &str) -> bool {
        let mut st = self.st.borrow_mut();
        let set = st.over.remove(key).is_some();

        if self.row.find(key).is_some() {
            st.deleted.insert(key.to_string());
        }

        if set && let Some(i) = st.added.iter().position(|k| k == key) {
            st.added.remove(i);
        }

        true
    }

    fn key_list(&self) -> Vec<String> {
        let st = self.st.borrow();
        let mut out: Vec<String> = Vec::with_capacity(self.row.keys.len() + st.added.len());
        for k in self.row.keys.iter() {
            if !st.deleted.contains(&**k) {
                out.push(k.to_string());
            }
        }

        out.extend(st.added.iter().cloned());

        out
    }
}

impl DynamicObject for RowView {
    fn get(&self, rt: &mut Runtime, key: &str) -> JsResult<Option<Value>> {
        self.get_key(rt, key)
    }

    fn set(&self, rt: &mut Runtime, key: &str, val: Value) -> JsResult<bool> {
        self.bind(rt);
        Ok(self.set_key(key, val))
    }

    fn has(&self, rt: &mut Runtime, key: &str) -> JsResult<bool> {
        self.bind(rt);
        Ok(self.has_key(key))
    }

    fn delete(&self, rt: &mut Runtime, key: &str) -> JsResult<bool> {
        self.bind(rt);
        Ok(self.delete_key(key))
    }

    fn keys(&self, rt: &mut Runtime) -> JsResult<Vec<String>> {
        self.bind(rt);
        Ok(self.key_list())
    }

    fn export(self: Rc<Self>) -> DocValue {
        DocValue::Host(self)
    }

    fn trace(&self, t: &mut Tracer) {
        for v in self.st.borrow().over.values() {
            t.value(v);
        }
    }
}

pub(crate) static KEYED_METHODS: [GoMethod; 5] = [
    GoMethod {
        name: "Delete",
        num_in: 1,
    },
    GoMethod {
        name: "Get",
        num_in: 1,
    },
    GoMethod {
        name: "Has",
        num_in: 1,
    },
    GoMethod {
        name: "Keys",
        num_in: 0,
    },
    GoMethod {
        name: "Set",
        num_in: 2,
    },
];

pub(crate) fn string_list(rt: &mut Runtime, keys: Vec<String>) -> Value {
    let items: Vec<Value> = keys
        .iter()
        .map(|k| Value::String(talos::export::string_from_go(k.as_bytes())))
        .collect();
    Value::Object(rt.new_array_values(items))
}

pub(crate) fn arg_string(rt: &mut Runtime, call: &FunctionCall<'_>, i: usize) -> JsResult<String> {
    talos::host::goreflect::arg_string(rt, call, i)
}

impl GoMethods for RowView {
    fn methods(&self) -> &'static [GoMethod] {
        &KEYED_METHODS
    }

    fn call(&self, rt: &mut Runtime, idx: usize, call: FunctionCall<'_>) -> JsResult<Value> {
        self.bind(rt);
        match idx {
            0 => {
                let k = arg_string(rt, &call, 0)?;
                Ok(Value::Bool(self.delete_key(&k)))
            }
            1 => {
                let k = arg_string(rt, &call, 0)?;
                Ok(self.get_key(rt, &k)?.unwrap_or(Value::Undefined))
            }
            2 => {
                let k = arg_string(rt, &call, 0)?;
                Ok(Value::Bool(self.has_key(&k)))
            }
            3 => {
                let keys = self.key_list();
                Ok(string_list(rt, keys))
            }
            _ => {
                let k = arg_string(rt, &call, 0)?;
                Ok(Value::Bool(self.set_key(&k, call.argument(1))))
            }
        }
    }

    fn same(&self, other: &dyn GoMethods) -> bool {
        other
            .as_any()
            .downcast_ref::<RowView>()
            .is_some_and(|o| std::ptr::eq(o, self))
    }

    fn trace(&self, t: &mut Tracer) {
        DynamicObject::trace(self, t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

struct ArrayState {
    items: Vec<Option<Value>>,
    base: usize,
    length: usize,
    runtime: u64,
}

pub(crate) struct ArrayView {
    res: Option<Arc<QueryResult>>,
    st: RefCell<ArrayState>,
}

pub(crate) fn new_array_view(rt: &mut Runtime, res: Option<Arc<QueryResult>>) -> Value {
    let n = res.as_ref().map_or(0, |r| r.len());
    let a = ArrayView {
        res,
        st: RefCell::new(ArrayState {
            items: Vec::new(),
            base: n,
            length: n,
            runtime: rt.host.id(),
        }),
    };
    Value::Object(rt.new_dynamic_array(Rc::new(a)))
}

impl ArrayState {
    fn grow(&mut self, n: usize) {
        if self.items.len() < n {
            self.items.resize(n, None);
        }
    }
}

impl ArrayView {
    fn bind(&self, rt: &Runtime) {
        let mut st = self.st.borrow_mut();
        if st.runtime != rt.host.id() {
            st.runtime = rt.host.id();
            st.items.clear();
        }
    }

    fn get_at(&self, rt: &mut Runtime, i: i64) -> Option<Value> {
        self.bind(rt);
        let row = {
            let st = self.st.borrow();
            if i < 0 || i as usize >= st.length {
                return None;
            }
            let i = i as usize;
            if let Some(Some(v)) = st.items.get(i) {
                return Some(v.clone());
            }
            if i >= st.base {
                return None;
            }
            self.res.as_ref()?.row(i).clone()
        };

        let view = RowView::new(rt, row);
        let v = Value::Object(rt.new_dynamic_object(Rc::new(view)));
        let mut st = self.st.borrow_mut();
        let length = st.length;
        st.grow(length);
        st.items[i as usize] = Some(v.clone());

        Some(v)
    }

    fn set_at(&self, i: i64, val: Value) -> bool {
        if i < 0 {
            return false;
        }

        let i = i as usize;
        let mut st = self.st.borrow_mut();
        if i >= st.length {
            st.length = i + 1;
        }

        let length = st.length;
        st.grow(length);
        st.items[i] = Some(val);

        true
    }

    fn set_len(&self, n: i64) -> bool {
        if n < 0 {
            return false;
        }

        let n = n as usize;
        let mut st = self.st.borrow_mut();
        if n < st.base {
            st.base = n;
        }

        if n < st.items.len() {
            st.items.truncate(n);
        }

        st.length = n;

        true
    }
}

impl DynamicArray for ArrayView {
    fn length(&self, _rt: &mut Runtime) -> JsResult<i64> {
        Ok(self.st.borrow().length as i64)
    }

    fn get(&self, rt: &mut Runtime, idx: i64) -> JsResult<Option<Value>> {
        Ok(self.get_at(rt, idx))
    }

    fn set(&self, rt: &mut Runtime, idx: i64, val: Value) -> JsResult<bool> {
        self.bind(rt);
        Ok(self.set_at(idx, val))
    }

    fn set_length(&self, rt: &mut Runtime, len: i64) -> JsResult<bool> {
        self.bind(rt);
        Ok(self.set_len(len))
    }

    fn export(self: Rc<Self>) -> DocValue {
        DocValue::Host(self)
    }

    fn trace(&self, t: &mut Tracer) {
        for v in self.st.borrow().items.iter().flatten() {
            t.value(v);
        }
    }
}

static ARRAY_METHODS: [GoMethod; 4] = [
    GoMethod {
        name: "Get",
        num_in: 1,
    },
    GoMethod {
        name: "Len",
        num_in: 0,
    },
    GoMethod {
        name: "Set",
        num_in: 2,
    },
    GoMethod {
        name: "SetLen",
        num_in: 1,
    },
];

impl GoMethods for ArrayView {
    fn methods(&self) -> &'static [GoMethod] {
        &ARRAY_METHODS
    }

    fn call(&self, rt: &mut Runtime, idx: usize, call: FunctionCall<'_>) -> JsResult<Value> {
        match idx {
            0 => {
                let i = talos::host::goreflect::arg_int(rt, &call, 0)?;
                Ok(self.get_at(rt, i).unwrap_or(Value::Undefined))
            }
            1 => Ok(int_to_value(self.st.borrow().length as i64)),
            2 => {
                let i = talos::host::goreflect::arg_int(rt, &call, 0)?;
                self.bind(rt);
                Ok(Value::Bool(self.set_at(i, call.argument(1))))
            }
            _ => {
                let n = talos::host::goreflect::arg_int(rt, &call, 0)?;
                self.bind(rt);
                Ok(Value::Bool(self.set_len(n)))
            }
        }
    }

    fn same(&self, other: &dyn GoMethods) -> bool {
        other
            .as_any()
            .downcast_ref::<ArrayView>()
            .is_some_and(|o| std::ptr::eq(o, self))
    }

    fn trace(&self, t: &mut Tracer) {
        DynamicArray::trace(self, t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
