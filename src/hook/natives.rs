use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

use goja::heap::Tracer;
use goja::host::{DocValue, DynamicObject, GoMethod, GoMethods};
use goja::object::FunctionCall;
use goja::runtime::{JsResult, Runtime};
use goja::value::{Value, float_to_value, int_to_value};

use crate::docpath;
use crate::gocompat::json;
use crate::gocompat::strconv;
use crate::sync::Mutex;

use super::results::QueryResult;
use super::rows::{Cell, KEYED_METHODS, Row, arg_string, string_list};

#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct Aggregate {
    pub(crate) count: i64,
    pub(crate) sum: f64,
}

pub(crate) type Groups = BTreeMap<String, Aggregate>;

#[derive(Default)]
struct AggState {
    sums: HashMap<String, f64>,
    group: HashMap<String, Arc<Groups>>,
}

#[derive(Default)]
pub(crate) struct Aggregates {
    st: Mutex<AggState>,
}

pub(crate) enum Scalar<'a> {
    Nil,
    Bool(bool),
    Num(f64),
    Str(&'a str),
    Other,
}

fn json_scalar(v: &json::Value) -> Scalar<'_> {
    match v {
        json::Value::Null => Scalar::Nil,
        json::Value::Bool(b) => Scalar::Bool(*b),
        json::Value::Number(f) => Scalar::Num(*f),
        json::Value::Int(i) => Scalar::Num(*i as f64),
        json::Value::String(s) => Scalar::Str(s),
        _ => Scalar::Other,
    }
}

impl Row {
    pub(crate) fn value(&self, field: &str) -> Option<Scalar<'_>> {
        if let Some(i) = self.find(field) {
            return Some(match &self.vals[i] {
                Cell::Nested(v) => json_scalar(v),
                Cell::Null => Scalar::Nil,
                Cell::Bool(b) => Scalar::Bool(*b),
                Cell::Num(n) => Scalar::Num(*n),
                Cell::Str(s) => Scalar::Str(s),
            });
        }

        let (head, rest) = field.split_once('.')?;

        let i = self.find(head)?;
        let Cell::Nested(v) = &self.vals[i] else {
            return None;
        };

        let values = docpath::resolve_values(v, rest.as_bytes());
        if values.len() != 1 {
            return None;
        }

        Some(json_scalar(values[0]))
    }
}

fn number(v: &Scalar<'_>) -> Option<f64> {
    match v {
        Scalar::Num(n) => Some(*n),
        _ => None,
    }
}

fn group_key(v: &Scalar<'_>) -> Option<String> {
    match v {
        Scalar::Str(s) => Some(s.to_string()),
        Scalar::Bool(b) => Some(strconv::format_bool(*b).to_string()),
        Scalar::Num(n) => Some(strconv::format_float(*n, b'f', -1, 64)),
        _ => None,
    }
}

impl QueryResult {
    fn aggs(&self) -> &Aggregates {
        self.agg.get_or_init(Aggregates::default)
    }

    pub(crate) fn sum(&self, field: &str) -> f64 {
        let a = self.aggs();
        let mut st = a.st.lock();

        if let Some(s) = st.sums.get(field) {
            return *s;
        }

        let mut total = 0.0;
        for r in self.rows() {
            if let Some(v) = r.value(field)
                && let Some(n) = number(&v)
            {
                total += n;
            }
        }
        st.sums.insert(field.to_string(), total);

        total
    }

    pub(crate) fn group_by(&self, field: &str, sum_field: &str) -> Arc<Groups> {
        let key = format!("{field}\0{sum_field}");
        let a = self.aggs();
        let mut st = a.st.lock();

        if let Some(g) = st.group.get(&key) {
            return g.clone();
        }

        let mut g = Groups::new();
        for r in self.rows() {
            let Some(v) = r.value(field) else {
                continue;
            };

            let Some(k) = group_key(&v) else {
                continue;
            };

            let agg = g.entry(k).or_default();
            agg.count += 1;

            if !sum_field.is_empty()
                && let Some(sv) = r.value(sum_field)
                && let Some(n) = number(&sv)
            {
                agg.sum += n;
            }
        }
        let g = Arc::new(g);
        st.group.insert(key, g.clone());

        g
    }
}

#[derive(Default)]
struct GroupState {
    over: HashMap<String, Value>,
    deleted: HashSet<String>,
    added: Vec<String>,
}

pub(crate) struct GroupView {
    g: Arc<Groups>,
    with_sum: bool,
    runtime: std::cell::Cell<u64>,
    st: RefCell<GroupState>,
}

pub(crate) fn group_object(rt: &mut Runtime, g: Arc<Groups>, with_sum: bool) -> Value {
    let v = GroupView {
        g,
        with_sum,
        runtime: std::cell::Cell::new(rt.host.id()),
        st: RefCell::new(GroupState::default()),
    };
    Value::Object(rt.new_dynamic_object(Rc::new(v)))
}

impl GroupView {
    fn bind(&self, rt: &Runtime) {
        if self.runtime.get() == rt.host.id() {
            return;
        }
        self.runtime.set(rt.host.id());
        let mut st = self.st.borrow_mut();
        st.over.clear();
        st.added.clear();
    }

    fn get_key(&self, rt: &mut Runtime, key: &str) -> JsResult<Option<Value>> {
        self.bind(rt);
        if let Some(v) = self.st.borrow().over.get(key) {
            return Ok(Some(v.clone()));
        }

        let Some(agg) = self.g.get(key).copied() else {
            return Ok(None);
        };
        if self.st.borrow().deleted.contains(key) {
            return Ok(None);
        }

        let entry = rt.new_object();
        entry.set_own_str(rt, &"count".into(), int_to_value(agg.count), true)?;
        if self.with_sum {
            entry.set_own_str(rt, &"sum".into(), float_to_value(agg.sum), true)?;
        }

        let v = Value::Object(entry);
        self.st.borrow_mut().over.insert(key.to_string(), v.clone());

        Ok(Some(v))
    }

    fn set_key(&self, key: &str, val: Value) -> bool {
        let mut st = self.st.borrow_mut();
        if !st.over.contains_key(key) && !self.g.contains_key(key) {
            st.added.push(key.to_string());
        }

        st.over.insert(key.to_string(), val);
        st.deleted.remove(key);

        true
    }

    fn has_key(&self, key: &str) -> bool {
        let st = self.st.borrow();
        if st.over.contains_key(key) {
            return true;
        }

        self.g.contains_key(key) && !st.deleted.contains(key)
    }

    fn delete_key(&self, key: &str) -> bool {
        let mut st = self.st.borrow_mut();
        st.over.remove(key);
        if self.g.contains_key(key) {
            st.deleted.insert(key.to_string());
        }

        if let Some(i) = st.added.iter().position(|k| k == key) {
            st.added.remove(i);
        }

        true
    }

    fn key_list(&self) -> Vec<String> {
        let st = self.st.borrow();
        let mut keys: Vec<String> = Vec::with_capacity(self.g.len() + st.added.len());
        for k in self.g.keys() {
            if !st.deleted.contains(k) {
                keys.push(k.clone());
            }
        }

        keys.extend(st.added.iter().cloned());

        keys
    }
}

impl DynamicObject for GroupView {
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

impl GoMethods for GroupView {
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
            .downcast_ref::<GroupView>()
            .is_some_and(|o| std::ptr::eq(o, self))
    }

    fn trace(&self, t: &mut Tracer) {
        DynamicObject::trace(self, t);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
