use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, LazyLock};

use goja::Runtime;
use goja::host::{DocValue, MapRef, json as host_json};

use crate::crypto::sha256;
use crate::gocompat::json::{self, Map};
use crate::gocompat::time::Time;
use crate::memory;
use crate::query::Filters;
use crate::sync::{Condvar, Mutex};

use super::results::unix_now;
use super::rows::doc_from_json;

#[derive(Clone, Debug)]
pub(crate) struct QueryDep {
    pub(crate) target: String,
    pub(crate) filters: Filters,
    pub(crate) owner: String,
    pub(crate) key: Vec<u8>,
    pub(crate) version: String,
    pub(crate) bypass: bool,
}

#[derive(Default)]
pub(crate) struct Output {
    pub(crate) deps: Vec<QueryDep>,
    set: Map,
    deleted: Vec<String>,
    bytes: i64,
}

struct OutputItem {
    out: Vec<Arc<Output>>,
    tick: u64,
    used: i64,
}

struct OutputFlight {
    done: Mutex<bool>,
    cv: Condvar,
}

#[derive(Default)]
struct MemoState {
    items: HashMap<[u8; 32], OutputItem>,
    lru: BTreeMap<u64, [u8; 32]>,
    next_tick: u64,
    flights: HashMap<[u8; 32], Arc<OutputFlight>>,
}

impl MemoState {
    fn tick(&mut self) -> u64 {
        self.next_tick += 1;
        self.next_tick
    }
}

pub(crate) struct OutputMemo {
    st: Mutex<MemoState>,
    hits: AtomicI64,
    misses: AtomicI64,
}

const MAX_OUTPUTS_PER_INPUT: usize = 8;

pub(crate) static OUTPUTS: LazyLock<OutputMemo> = LazyLock::new(|| OutputMemo {
    st: Mutex::new(MemoState::default()),
    hits: AtomicI64::new(0),
    misses: AtomicI64::new(0),
});

pub(crate) fn memo_key(script: &str, fn_name: &str, input: &[u8]) -> [u8; 32] {
    let mut h = sha256::Digest::new();
    h.write(script.as_bytes());
    h.write(&[0]);
    h.write(fn_name.as_bytes());
    h.write(&[0]);
    h.write(input);

    h.sum()
}

pub(crate) fn canonical(entity: &MapRef) -> Option<Vec<u8>> {
    let v = DocValue::Map(entity.clone());
    if !json_safe(&v) {
        return None;
    }

    host_json::marshal(&v).ok()
}

pub(crate) fn json_safe(v: &DocValue) -> bool {
    enum Step {
        Enter(DocValue),
        Leave(usize),
    }

    let mut path: HashSet<usize> = HashSet::new();
    let mut stack: Vec<Step> = vec![Step::Enter(v.clone())];
    while let Some(step) = stack.pop() {
        let v = match step {
            Step::Leave(id) => {
                path.remove(&id);
                continue;
            }
            Step::Enter(v) => v,
        };

        match &v {
            DocValue::Nil
            | DocValue::Bool(_)
            | DocValue::String(_)
            | DocValue::Float64(_)
            | DocValue::Int64(_) => {}
            DocValue::Map(m) => {
                let id = std::rc::Rc::as_ptr(m) as *const () as usize;
                if !path.insert(id) {
                    return false;
                }
                stack.push(Step::Leave(id));
                for x in m.borrow().values() {
                    stack.push(Step::Enter(x.clone()));
                }
            }
            DocValue::Slice(h) => {
                let id = std::rc::Rc::as_ptr(&h.backing) as *const () as usize | 1;
                if !path.insert(id) {
                    return false;
                }
                stack.push(Step::Leave(id));
                for x in h.to_vec() {
                    stack.push(Step::Enter(x));
                }
            }
            _ => return false,
        }
    }

    true
}

pub(crate) fn to_json(v: &DocValue) -> json::Value {
    enum Dest {
        Root,
        Map(usize, String),
        Item(usize, usize),
    }
    enum Node {
        Map(Map),
        List(Vec<json::Value>),
    }
    enum Work {
        Value(DocValue, Dest),
        Close(usize, Dest),
    }

    let mut nodes: Vec<Node> = Vec::new();
    let mut root = json::Value::Null;
    let mut work: Vec<Work> = vec![Work::Value(v.clone(), Dest::Root)];

    fn place(nodes: &mut [Node], root: &mut json::Value, d: Dest, x: json::Value) {
        match d {
            Dest::Root => *root = x,
            Dest::Map(i, k) => {
                if let Node::Map(m) = &mut nodes[i] {
                    m.insert(k, x);
                }
            }
            Dest::Item(i, j) => {
                if let Node::List(l) = &mut nodes[i] {
                    l[j] = x;
                }
            }
        }
    }

    while let Some(w) = work.pop() {
        match w {
            Work::Close(i, d) => {
                let x = match std::mem::replace(&mut nodes[i], Node::List(Vec::new())) {
                    Node::Map(m) => json::Value::Object(m),
                    Node::List(l) => json::Value::Array(l),
                };
                place(&mut nodes, &mut root, d, x);
            }
            Work::Value(v, d) => match &v {
                DocValue::Nil => place(&mut nodes, &mut root, d, json::Value::Null),
                DocValue::Bool(b) => place(&mut nodes, &mut root, d, json::Value::Bool(*b)),
                DocValue::Int64(i) => place(&mut nodes, &mut root, d, json::Value::Int(*i)),
                DocValue::Float64(f) => place(&mut nodes, &mut root, d, json::Value::Number(*f)),
                DocValue::String(s) => {
                    place(&mut nodes, &mut root, d, json::Value::String(s.to_string()))
                }
                DocValue::Map(m) => {
                    let i = nodes.len();
                    let mut target = Map::new();
                    let entries: Vec<(String, DocValue)> = m
                        .borrow()
                        .iter()
                        .map(|(k, x)| (k.clone(), x.clone()))
                        .collect();
                    for (k, _) in &entries {
                        target.insert(k.clone(), json::Value::Null);
                    }
                    nodes.push(Node::Map(target));
                    work.push(Work::Close(i, d));
                    for (k, x) in entries {
                        work.push(Work::Value(x, Dest::Map(i, k)));
                    }
                }
                DocValue::Slice(h) => {
                    let i = nodes.len();
                    let items = h.to_vec();
                    nodes.push(Node::List(vec![json::Value::Null; items.len()]));
                    work.push(Work::Close(i, d));
                    for (j, x) in items.into_iter().enumerate() {
                        work.push(Work::Value(x, Dest::Item(i, j)));
                    }
                }
                _ => place(&mut nodes, &mut root, d, json::Value::Null),
            },
        }
    }

    root
}

pub(crate) struct FlightToken {
    key: [u8; 32],
    f: Arc<OutputFlight>,
}

impl Drop for FlightToken {
    fn drop(&mut self) {
        OUTPUTS.st.lock().flights.remove(&self.key);
        *self.f.done.lock() = true;
        self.f.cv.notify_all();
    }
}

impl OutputMemo {
    pub(crate) fn find(
        &self,
        key: &[u8; 32],
        valid: impl Fn(&Output) -> bool,
    ) -> Option<Arc<Output>> {
        let candidates: Vec<Arc<Output>> = {
            let mut st = self.st.lock();
            let t = st.tick();
            match st.items.get_mut(key) {
                Some(it) => {
                    let old = it.tick;
                    it.tick = t;
                    it.used = unix_now();
                    let c = it.out.clone();
                    st.lru.remove(&old);
                    st.lru.insert(t, *key);
                    c
                }
                None => Vec::new(),
            }
        };

        for o in candidates {
            if valid(&o) {
                self.hits.fetch_add(1, Ordering::SeqCst);
                return Some(o);
            }
        }

        self.misses.fetch_add(1, Ordering::SeqCst);

        None
    }

    pub(crate) fn begin(&self, key: &[u8; 32]) -> Option<FlightToken> {
        let mut st = self.st.lock();
        if let Some(f) = st.flights.get(key).cloned() {
            drop(st);
            let mut done = f.done.lock();
            while !*done {
                done = f.cv.wait(done);
            }
            return None;
        }

        let f = Arc::new(OutputFlight {
            done: Mutex::new(false),
            cv: Condvar::new(),
        });
        st.flights.insert(*key, f.clone());

        Some(FlightToken { key: *key, f })
    }

    pub(crate) fn store(&self, key: &[u8; 32], o: Output) {
        let o = Arc::new(o);
        let mut st = self.st.lock();
        let t = st.tick();
        let now = unix_now();
        let old_tick = st.items.get(key).map(|it| it.tick);
        match old_tick {
            None => {
                st.items.insert(
                    *key,
                    OutputItem {
                        out: Vec::new(),
                        tick: t,
                        used: now,
                    },
                );
            }
            Some(old) => {
                st.lru.remove(&old);
            }
        }
        st.lru.insert(t, *key);
        let it = st.items.get_mut(key).expect("output item");
        it.tick = t;
        it.used = now;

        if it.out.len() >= MAX_OUTPUTS_PER_INPUT {
            memory::HOOK_RESULTS.release(it.out[0].bytes);
            it.out.remove(0);
        }
        memory::HOOK_RESULTS.add(o.bytes);
        it.out.push(o);

        let limit = memory::HOOK_RESULTS.effective();
        while limit > 0 && memory::HOOK_RESULTS.used() > limit && st.lru.len() > 1 {
            let Some((_, k)) = st.lru.pop_first() else {
                break;
            };
            if let Some(old) = st.items.remove(&k) {
                for x in &old.out {
                    memory::HOOK_RESULTS.release(x.bytes);
                }
            }
        }
    }

    pub(crate) fn sweep(&self, cutoff: i64) -> i64 {
        let mut st = self.st.lock();
        let mut freed: i64 = 0;
        let stale: Vec<([u8; 32], u64)> = st
            .items
            .iter()
            .filter(|(_, it)| it.used < cutoff)
            .map(|(k, it)| (*k, it.tick))
            .collect();
        for (k, tick) in stale {
            st.lru.remove(&tick);
            if let Some(it) = st.items.remove(&k) {
                for o in &it.out {
                    memory::HOOK_RESULTS.release(o.bytes);
                    freed += o.bytes;
                }
            }
        }

        freed
    }

    pub(crate) fn reset(&self) {
        let mut st = self.st.lock();
        for it in st.items.values() {
            for o in &it.out {
                memory::HOOK_RESULTS.release(o.bytes);
            }
        }
        st.items = HashMap::new();
        st.lru = BTreeMap::new();
    }
}

pub(crate) fn diff_output(before: &HashMap<String, Vec<u8>>, after: &MapRef) -> Option<Output> {
    let mut o = Output::default();
    let entries: Vec<(String, DocValue)> = after
        .borrow()
        .iter()
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for (k, v) in &entries {
        if !json_safe(v) {
            return None;
        }

        let b = host_json::marshal(v).ok()?;

        if before.get(k).is_some_and(|old| *old == b) {
            continue;
        }

        o.set.insert(k.clone(), to_json(v));
        o.bytes += (k.len() + b.len()) as i64 * 3;
    }

    let current = after.borrow();
    for k in before.keys() {
        if !current.contains_key(k) {
            o.deleted.push(k.clone());
        }
    }

    o.bytes += 128;

    Some(o)
}

pub(crate) fn field_bytes(entity: &MapRef) -> HashMap<String, Vec<u8>> {
    let m = entity.borrow();
    let mut out = HashMap::with_capacity(m.len());
    for (k, v) in m.iter() {
        out.insert(k.clone(), host_json::marshal(v).unwrap_or_default());
    }

    out
}

impl Output {
    pub(crate) fn apply(&self, entity: &MapRef) {
        for (k, v) in &self.set {
            let copied = doc_from_json(v);
            entity.borrow_mut().insert(k.clone(), copied);
        }

        for k in &self.deleted {
            entity.borrow_mut().remove(k);
        }
    }
}

pub(crate) struct Determinism {
    pub(crate) broken: Arc<AtomicBool>,
}

thread_local! {
    static RAND: std::cell::RefCell<Option<crate::gocompat::rand::Rand>> = const { std::cell::RefCell::new(None) };
}

fn random_float() -> f64 {
    RAND.with(|r| {
        let mut r = r.borrow_mut();
        let rand = r.get_or_insert_with(|| {
            let mut seed = [0u8; 8];
            let _ = crate::crypto::rand::os_random(&mut seed);
            crate::gocompat::rand::Rand::new(crate::gocompat::rand::new_source(i64::from_le_bytes(
                seed,
            )))
        });
        rand.float64()
    })
}

impl Determinism {
    pub(crate) fn new() -> Determinism {
        Determinism {
            broken: Arc::new(AtomicBool::new(false)),
        }
    }

    pub(crate) fn install(&self, rt: &mut Runtime) {
        let b = self.broken.clone();
        rt.set_time_source(Box::new(move || {
            b.store(true, Ordering::SeqCst);
            Time::now()
        }));
        let b = self.broken.clone();
        rt.rand = Some(Box::new(move || {
            b.store(true, Ordering::SeqCst);
            random_float()
        }));
    }
}

pub fn reset_output_memo() {
    OUTPUTS.reset();
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct OutputMemoStats {
    pub hits: i64,
    pub misses: i64,
}

pub fn output_memo() -> OutputMemoStats {
    OutputMemoStats {
        hits: OUTPUTS.hits.load(Ordering::SeqCst),
        misses: OUTPUTS.misses.load(Ordering::SeqCst),
    }
}
