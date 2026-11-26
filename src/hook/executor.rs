use std::cell::{Cell, RefCell};
use std::cmp::Reverse;
use std::collections::{BinaryHeap, HashMap};
use std::mem::ManuallyDrop;
use std::panic::{AssertUnwindSafe, catch_unwind, resume_unwind};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, Once};
use std::time::{Duration, Instant};

use talos::export::string_from_go;
use talos::heap::FxHashMap;
use talos::host::{DocMap, DocValue, FuncRef, HostFn, MapRef};
use talos::object::FunctionCall;
use talos::runtime::{InterruptHandle, JsResult, RunError, Throw};
use talos::value::{Value, float_to_value, int_to_value};
use talos::{Program, Runtime};

use crate::acl;
use crate::api::{self, Guard, GuardError};
use crate::crypto::sha256;
use crate::engine;
use crate::globals;
use crate::gocompat::strconv;
use crate::gocompat::time::Duration as GoDuration;
use crate::log;
use crate::metrics;
use crate::query::{Filters, Ops};
use crate::storage;
use crate::sync::{Condvar, Mutex};

use super::foldplan::{FOLD_GLOBAL, ScriptPlan, plan_for};
use super::hook::{HOOK_LOG, Hook, HookError};
use super::memo::{
    Determinism, OUTPUTS, Output, QueryDep, canonical, diff_output, field_bytes, memo_key,
};
use super::natives::group_object;
use super::prepare::PREPARED;
use super::results::{QueryResult, RESULTS, build_result};
use super::rows::new_array_view;
use super::view::{PinnedViews, ViewToken, fold_output};

const DEFAULT_SCRIPT_TIMEOUT: Duration = Duration::from_secs(2);
const MAX_COMPILED_SCRIPTS: usize = 1024;

static REASON: Mutex<Option<(Duration, Arc<str>)>> = Mutex::new(None);

fn timeout_reason(timeout: Duration) -> Arc<str> {
    let mut g = REASON.lock();
    if let Some((d, r)) = g.as_ref()
        && *d == timeout
    {
        return r.clone();
    }

    let r: Arc<str> = Arc::from(format!(
        "hook script stopped after {}",
        GoDuration(timeout.as_nanos() as i64)
    ));
    *g = Some((timeout, r.clone()));

    r
}

fn script_timeout() -> Duration {
    if let Some(cfg) = globals::config()
        && cfg.api.hooks.timeout_ms > 0
    {
        return Duration::from_millis(cfg.api.hooks.timeout_ms as u64);
    }

    DEFAULT_SCRIPT_TIMEOUT
}

pub(crate) struct CompiledScript {
    pub(crate) program: Arc<Program>,
    pub(crate) prefix: sha256::Digest,
}

static COMPILED: Mutex<Option<FxHashMap<String, Arc<CompiledScript>>>> = Mutex::new(None);

fn compile(script: &str) -> Result<Arc<CompiledScript>, HookError> {
    if let Some(p) = COMPILED
        .lock()
        .as_ref()
        .and_then(|m| m.get(script).cloned())
    {
        return Ok(p);
    }

    let program =
        talos::compile("", script.as_bytes(), false).map_err(|e| HookError::new(e.to_string()))?;
    let mut prefix = sha256::Digest::new();
    prefix.write(script.as_bytes());
    prefix.write(&[0]);
    let p = Arc::new(CompiledScript { program, prefix });

    let mut g = COMPILED.lock();
    let m = g.get_or_insert_with(FxHashMap::default);
    if m.len() >= MAX_COMPILED_SCRIPTS {
        m.clear();
    }
    m.insert(script.to_string(), p.clone());

    Ok(p)
}

#[derive(Default)]
pub(crate) struct RequestState {
    pub(crate) memo: HashMap<Vec<u8>, Arc<QueryResult>>,
    pub(crate) pins: PinnedViews,
}

pub(crate) struct RequestInner {
    username: String,
    pub(crate) st: Mutex<RequestState>,
    budget: Duration,
    spent: AtomicI64,
    skipped: AtomicBool,
    guard: Option<Arc<Guard>>,
}

#[derive(Clone)]
pub struct Request {
    pub(crate) inner: Arc<RequestInner>,
}

impl Request {
    pub fn new(username: &str) -> Request {
        let mut budget = Duration::ZERO;
        if let Some(cfg) = globals::config()
            && cfg.api.hooks.request_budget_ms > 0
        {
            budget = Duration::from_millis(cfg.api.hooks.request_budget_ms as u64);
        }

        Request {
            inner: Arc::new(RequestInner {
                username: username.to_string(),
                st: Mutex::new(RequestState::default()),
                budget,
                spent: AtomicI64::new(0),
                skipped: AtomicBool::new(false),
                guard: None,
            }),
        }
    }

    pub fn with_guard(mut self, guard: Arc<Guard>) -> Request {
        match Arc::get_mut(&mut self.inner) {
            Some(inner) => inner.guard = Some(guard),
            None => {
                self.inner = Arc::new(RequestInner {
                    username: self.inner.username.clone(),
                    st: Mutex::new(RequestState::default()),
                    budget: self.inner.budget,
                    spent: AtomicI64::new(self.inner.spent.load(Ordering::SeqCst)),
                    skipped: AtomicBool::new(self.inner.skipped.load(Ordering::SeqCst)),
                    guard: Some(guard),
                })
            }
        }

        self
    }

    pub fn stopped(&self) -> bool {
        api::guard::stopped(self.guard())
    }

    pub(crate) fn guard(&self) -> Option<&Guard> {
        self.inner.guard.as_deref()
    }

    pub(crate) fn inner_guard(&self) -> Option<Arc<Guard>> {
        self.inner.guard.clone()
    }

    pub(crate) fn username(&self) -> &str {
        &self.inner.username
    }

    pub(crate) fn over_budget(&self) -> bool {
        if self.inner.budget.is_zero() {
            return false;
        }

        if Duration::from_nanos(self.inner.spent.load(Ordering::SeqCst).max(0) as u64)
            < self.inner.budget
        {
            return false;
        }

        if self
            .inner
            .skipped
            .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            HOOK_LOG.warn(
                "hook budget of the request exceeded, the remaining hooks are skipped",
                &[(
                    "budget_ms",
                    log::Value::Int(self.inner.budget.as_millis() as i64),
                )],
            );
        }

        true
    }

    pub(crate) fn add_spent(&self, d: Duration) {
        self.inner
            .spent
            .fetch_add(d.as_nanos() as i64, Ordering::SeqCst);
    }

    pub(crate) fn owner(&self, bypass_acl: bool) -> String {
        if bypass_acl {
            return "\0bypass".to_string();
        }

        self.inner.username.clone()
    }
}

pub(crate) fn filters_key(filters: &Filters) -> Vec<u8> {
    let mut b = Vec::new();
    for (f, ops) in filters {
        for (op, v) in ops {
            b.extend_from_slice(strconv::quote(f).as_bytes());
            b.push(b'[');
            b.extend_from_slice(strconv::quote(op).as_bytes());
            b.extend_from_slice(b"]=");
            b.extend_from_slice(strconv::quote(v).as_bytes());
            b.push(b';');
        }
    }

    b
}

pub(crate) fn entity_version(entity: &str) -> String {
    let Some(e) = storage::engine() else {
        return String::new();
    };

    let snap = e.snapshot(entity.as_bytes());

    format!("{}.{}.{}", e.instance(), snap.generation(), snap.version())
}

fn key_version(entity: &str, filters: &Filters) -> String {
    let Some(e) = storage::engine() else {
        return String::new();
    };

    let snap = e.snapshot(entity.as_bytes());
    let mut version = snap.version();
    if let Some((field, terms)) = api::eq_key(filters) {
        let terms: Vec<&[u8]> = terms.iter().map(|t| t.as_slice()).collect();
        match snap.field_version(&field, &terms) {
            Some(v) => version = v,
            None => e.watch_field(entity.as_bytes(), &field),
        }
    }

    format!("{}.{}.{}", e.instance(), snap.generation(), version)
}

pub(crate) fn query_version(target: &str, filters: &Filters, bypass_acl: bool) -> String {
    if !api::extract_auto_includes(filters).is_empty() {
        return String::new();
    }

    let mut version = key_version(target, filters);
    if !version.is_empty() && !bypass_acl {
        version.push('/');
        version.push_str(&entity_version(acl::ACL_ENTITY));
    }

    version
}

pub(crate) fn query_key(target: &str, owner: &str, filters: &Filters) -> Vec<u8> {
    let mut key = Vec::with_capacity(target.len() + owner.len() + 32);
    key.extend_from_slice(target.as_bytes());
    key.push(0);
    key.extend_from_slice(owner.as_bytes());
    key.push(0);
    key.extend_from_slice(&filters_key(filters));
    key
}

impl Request {
    pub(crate) fn query(
        &self,
        h: &Hook,
        target: &str,
        filters: &Filters,
        bypass_acl: bool,
        guard: &Arc<Guard>,
    ) -> (Option<Arc<QueryResult>>, Vec<u8>, Option<GuardError>) {
        PREPARED.observe(&h.script, target, filters);

        let start = Instant::now();
        let key = query_key(target, &self.owner(bypass_acl), filters);

        let pinned = self.inner.st.lock().memo.get(&key).cloned();
        if let Some(res) = pinned {
            metrics::observe_hook_query(
                &h.id,
                &h.entity,
                &h.event,
                res.len() as i64,
                true,
                start.elapsed(),
            );
            return (Some(res), key, None);
        }

        let version = query_version(target, filters, bypass_acl);
        let (t, f, owner) = (
            target.to_string(),
            filters.clone(),
            self.inner.username.clone(),
        );
        let got = RESULTS.get(
            &key,
            &version,
            Some(guard),
            Box::new(move |prev| build_result(&t, &f, &owner, bypass_acl, None, prev)),
        );
        let (res, hit) = match got {
            Ok(v) => v,
            Err(GuardError::Busy) => return (None, key, Some(api::guard::reject(self.guard()))),
            Err(e) => return (None, key, Some(e)),
        };

        metrics::observe_hook_query(
            &h.id,
            &h.entity,
            &h.event,
            res.len() as i64,
            hit,
            start.elapsed(),
        );

        let mut st = self.inner.st.lock();
        let res = match st.memo.get(&key) {
            Some(cur) => cur.clone(),
            None => {
                st.memo.insert(key.clone(), res.clone());
                res
            }
        };

        (Some(res), key, None)
    }

    fn still_valid(&self, o: &Output) -> bool {
        for d in &o.deps {
            if d.version.is_empty() {
                return false;
            }

            if d.owner != self.owner(d.bypass) {
                return false;
            }

            let pinned = self
                .inner
                .st
                .lock()
                .memo
                .get(&d.key)
                .map(|r| r.version.clone());

            let mut current = query_version(&d.target, &d.filters, d.bypass);
            if let Some(v) = pinned {
                current = v;
            }

            if current != d.version {
                return false;
            }
        }

        true
    }

    fn run_deadline(&self, timeout: Duration) -> (Instant, Arc<str>) {
        let mut stop_at = Instant::now() + timeout;
        let mut reason = timeout_reason(timeout);
        if let Some(g) = self.guard()
            && let Some(d) = g.deadline
            && d < stop_at
        {
            stop_at = d;
            reason = Arc::from(GuardError::Deadline.to_string());
        }

        (stop_at, reason)
    }

    pub(crate) fn stop_reason(
        &self,
        err: GuardError,
        timeout_reason: &str,
    ) -> (String, Option<GuardError>) {
        if err == GuardError::Deadline && api::guard::err(self.guard()).is_none() {
            return (timeout_reason.to_string(), None);
        }

        (err.to_string(), Some(err))
    }
}

struct TimerEntry {
    generation: u64,
    handle: InterruptHandle,
    reason: Arc<str>,
}

struct TimerState {
    heap: BinaryHeap<Reverse<(Instant, u64, u64)>>,
    live: FxHashMap<u64, TimerEntry>,
    next: u64,
    waiting: Option<Instant>,
    idle: bool,
}

struct TimerService {
    st: Mutex<Option<TimerState>>,
    cv: Condvar,
}

static TIMER: TimerService = TimerService {
    st: Mutex::new(None),
    cv: Condvar::new(),
};

static TIMER_THREAD: Once = Once::new();

impl TimerState {
    fn compact(&mut self) {
        if self.heap.len() <= 2 * self.live.len() + 64 {
            return;
        }
        let live = &self.live;
        let kept: Vec<Reverse<(Instant, u64, u64)>> = self
            .heap
            .drain()
            .filter(|Reverse((_, id, generation))| {
                live.get(id).is_some_and(|e| e.generation == *generation)
            })
            .collect();
        self.heap = BinaryHeap::from(kept);
    }

    fn wakes(&self, at: Instant) -> bool {
        self.idle || self.waiting.is_some_and(|w| at < w)
    }
}

impl TimerService {
    fn run(&self) {
        let mut g = self.st.lock();
        loop {
            let now = Instant::now();
            let mut next: Option<Instant> = None;
            let st = TimerService::state(&mut g);
            while let Some(Reverse((at, id, generation))) = st.heap.peek().copied() {
                if st.live.get(&id).is_none_or(|e| e.generation != generation) {
                    st.heap.pop();
                    continue;
                }
                if at <= now {
                    st.heap.pop();
                    if let Some(e) = st.live.remove(&id) {
                        e.handle.interrupt(e.reason.to_string());
                    }
                    continue;
                }
                next = Some(at);
                break;
            }
            st.waiting = next;
            st.idle = next.is_none();
            g = match next {
                Some(at) => self.cv.wait_until(g, at).0,
                None => self.cv.wait(g),
            };
        }
    }

    fn state(g: &mut Option<TimerState>) -> &mut TimerState {
        g.get_or_insert_with(|| TimerState {
            heap: BinaryHeap::new(),
            live: FxHashMap::default(),
            next: 0,
            waiting: None,
            idle: true,
        })
    }

    fn start(&self, at: Instant, handle: InterruptHandle, reason: Arc<str>) -> u64 {
        TIMER_THREAD.call_once(|| {
            drop(crate::sync::spawn("hook-timer", || TIMER.run()));
        });

        let mut g = self.st.lock();
        let st = TimerService::state(&mut g);
        st.next += 1;
        let id = st.next;
        st.live.insert(
            id,
            TimerEntry {
                generation: 0,
                handle,
                reason,
            },
        );
        st.heap.push(Reverse((at, id, 0)));
        let wake = st.wakes(at);
        if wake {
            st.waiting = Some(at);
            st.idle = false;
        }
        drop(g);
        if wake {
            self.cv.notify_one();
        }

        id
    }

    fn stop(&self, id: u64) -> bool {
        let mut g = self.st.lock();
        let st = TimerService::state(&mut g);
        let was = st.live.remove(&id).is_some();
        st.compact();
        was
    }

    fn reset(&self, id: u64, at: Instant, handle: InterruptHandle, reason: Arc<str>) {
        let mut g = self.st.lock();
        let st = TimerService::state(&mut g);
        st.next += 1;
        let generation = st.next;
        st.live.insert(
            id,
            TimerEntry {
                generation,
                handle,
                reason,
            },
        );
        st.heap.push(Reverse((at, id, generation)));
        let wake = st.wakes(at);
        if wake {
            st.waiting = Some(at);
            st.idle = false;
        }
        drop(g);
        if wake {
            self.cv.notify_one();
        }
    }
}

pub(crate) struct RunCtx {
    pub(crate) req: Request,
    pub(crate) hook: Arc<Hook>,
    pub(crate) run: Arc<Guard>,
    pub(crate) reason: Arc<str>,
    outside: Cell<i64>,
    deps: RefCell<Vec<QueryDep>>,
    pub(crate) plan: Option<Arc<ScriptPlan>>,
    timer: u64,
    interrupt: InterruptHandle,
    stop_at: Cell<Instant>,
    det: Option<Determinism>,
}

pub(crate) struct Paused {
    began: Instant,
    running: bool,
}

impl RunCtx {
    pub(crate) fn pause(&self) -> Paused {
        let began = Instant::now();
        let running = TIMER.stop(self.timer);

        Paused { began, running }
    }

    pub(crate) fn resume(&self, p: Paused) {
        let spent = p.began.elapsed();
        self.outside
            .set(self.outside.get() + spent.as_nanos() as i64);
        if !p.running {
            return;
        }

        let mut stop_at = self.stop_at.get() + spent;
        if let Some(g) = self.req.guard()
            && let Some(d) = g.deadline
            && d < stop_at
        {
            stop_at = d;
        }
        self.stop_at.set(stop_at);
        TIMER.reset(
            self.timer,
            stop_at,
            self.interrupt.clone(),
            self.reason.clone(),
        );
    }

    pub(crate) fn throw(&self, rt: &mut Runtime, err: GuardError) -> Throw {
        let (msg, cause) = self.req.stop_reason(err, &self.reason);
        let v = match cause {
            Some(g) => rt.new_go_error_with(&msg, Rc::new(g)),
            None => rt.new_go_error(&msg),
        };

        Throw::Value(v)
    }

    pub(crate) fn depend(&self, target: &str, filters: &Filters, key: Vec<u8>, version: String) {
        self.deps.borrow_mut().push(QueryDep {
            target: target.to_string(),
            filters: filters.clone(),
            owner: self.req.owner(self.hook.bypass_acl),
            key,
            version,
            bypass: self.hook.bypass_acl,
        });
    }

    fn load(
        &self,
        rt: &mut Runtime,
        target: &str,
        filters: &Filters,
    ) -> JsResult<Arc<QueryResult>> {
        let began = Instant::now();
        let (res, key, err) =
            self.req
                .query(&self.hook, target, filters, self.hook.bypass_acl, &self.run);
        self.outside
            .set(self.outside.get() + began.elapsed().as_nanos() as i64);
        if let Some(e) = err {
            return Err(self.throw(rt, e));
        }

        let res = res.unwrap_or_default();
        self.depend(target, filters, key, res.version.clone());

        Ok(res)
    }

    fn fetch(
        &self,
        rt: &mut Runtime,
        name: &str,
        call: &FunctionCall<'_>,
        args: usize,
    ) -> JsResult<Option<Arc<QueryResult>>> {
        let (target, filters, core) = parse_args(rt, name, call, args)?;
        if core {
            return Ok(None);
        }

        self.load(rt, &target, &filters).map(Some)
    }
}

pub(crate) fn parse_args(
    rt: &mut Runtime,
    name: &str,
    call: &FunctionCall<'_>,
    args: usize,
) -> JsResult<(String, Filters, bool)> {
    if call.arguments.len() < args {
        let msg = format!("{name} expected");
        return Err(Throw::Value(Value::String(string_from_go(msg.as_bytes()))));
    }

    let target = call.arguments[0].go_string(rt)?;
    if engine::is_core_entity_type(target.as_bytes()) {
        return Ok((target, Filters::new(), true));
    }

    let params = rt.export(&call.arguments[1])?;

    let mut filters = Filters::new();
    if let DocValue::Map(params) = &params {
        for (field, cond) in params.borrow().iter() {
            let DocValue::Map(typed) = cond else {
                continue;
            };

            let mut ops = Ops::new();
            for (op, val) in typed.borrow().iter() {
                ops.insert(
                    op.as_bytes().to_vec(),
                    talos::host::json::format_v(val).into_bytes(),
                );
            }

            filters.insert(field.as_bytes().to_vec(), ops);
        }
    }

    Ok((target, filters, false))
}

fn native(
    name: &str,
    ctx: &Rc<RunCtx>,
    f: fn(&RunCtx, &mut Runtime, FunctionCall<'_>) -> JsResult<Value>,
) -> DocValue {
    let c = ctx.clone();
    DocValue::Func(FuncRef::Native(HostFn::new(name, move |rt, call| {
        f(&c, rt, call)
    })))
}

fn ctx_query(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let res = c.fetch(rt, "query(entity, params)", &call, 2)?;
    Ok(new_array_view(rt, res))
}

fn ctx_count(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let res = c.fetch(rt, "count(entity, params)", &call, 2)?;
    Ok(int_to_value(res.map_or(0, |r| r.len()) as i64))
}

fn ctx_sum(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let res = c.fetch(rt, "sum(entity, params, field)", &call, 3)?;
    let field = call.arguments[2].go_string(rt)?;
    Ok(float_to_value(res.map_or(0.0, |r| r.sum(&field))))
}

fn ctx_group(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let res = c.fetch(rt, "group(entity, params, field)", &call, 3)?;
    let mut sum_field = String::new();
    if call.arguments.len() > 3 && !call.arguments[3].is_undefined() && !call.arguments[3].is_null()
    {
        sum_field = call.arguments[3].go_string(rt)?;
    }
    let field = call.arguments[2].go_string(rt)?;
    let g = match res {
        Some(r) => r.group_by(&field, &sum_field),
        None => Arc::new(Default::default()),
    };
    Ok(group_object(rt, g, !sum_field.is_empty()))
}

fn helper_view(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let (target, filters, core) = parse_args(rt, "query(entity, params)", &call, 2)?;
    let mut tok = ViewToken {
        target,
        filters,
        core,
        key: Vec::new(),
    };
    if core {
        return rt.to_value(&DocValue::Host(Rc::new(tok)));
    }

    PREPARED.observe(&c.hook.script, &tok.target, &tok.filters);
    tok.key = query_key(&tok.target, &c.req.owner(c.hook.bypass_acl), &tok.filters);
    let version = query_version(&tok.target, &tok.filters, c.hook.bypass_acl);
    c.depend(&tok.target, &tok.filters, tok.key.clone(), version);

    rt.to_value(&DocValue::Host(Rc::new(tok)))
}

fn token(rt: &mut Runtime, v: &Value) -> JsResult<Option<Rc<dyn talos::host::GoMethods>>> {
    match &rt.export(v)? {
        DocValue::Host(h) if h.as_any().is::<ViewToken>() => Ok(Some(h.clone())),
        _ => Ok(None),
    }
}

fn helper_rows(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let arg = call.argument(0);
    let Some(h) = token(rt, &arg)? else {
        return Ok(arg);
    };
    let tok = h.as_any().downcast_ref::<ViewToken>().expect("view token");
    if tok.core {
        return Ok(new_array_view(rt, None));
    }

    let res = c.load(rt, &tok.target, &tok.filters)?;
    Ok(new_array_view(rt, Some(res)))
}

fn helper_fold(c: &RunCtx, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
    let arg = call.argument(1);
    let Some(h) = token(rt, &arg)? else {
        return Ok(Value::Null);
    };
    let tok = h.as_any().downcast_ref::<ViewToken>().expect("view token");

    let plan = c.plan.clone().expect("fold plan");
    let index = call.argument(0).to_integer(rt)?;
    let def = plan.folds[index as usize].clone();
    let regs = &call.arguments[2..2 + def.regs.len()];
    let free = &call.arguments[2 + def.regs.len()..];

    let paused = c.pause();
    let out = c.req.fold(rt, &c.hook, &def, tok, regs, free);
    c.resume(paused);
    let out = match out? {
        Ok(o) => o,
        Err(e) => return Err(c.throw(rt, e)),
    };

    let Some((vals, names)) = out else {
        return Ok(Value::Null);
    };

    let items = fold_output(&vals, &names);

    Ok(Value::Object(rt.new_array_values(items)))
}

fn release_except(root: DocValue, keep: *const ()) {
    let mut seen: std::collections::HashSet<usize> = std::collections::HashSet::new();
    let mut stack: Vec<DocValue> = vec![root];
    while let Some(v) = stack.pop() {
        match &v {
            DocValue::Map(m) => {
                let p = Rc::as_ptr(m) as *const ();
                if p == keep {
                    continue;
                }
                if seen.insert(p as usize) {
                    let taken = std::mem::take(&mut *m.borrow_mut());
                    stack.extend(taken.into_values());
                }
            }
            DocValue::Slice(h) => {
                if seen.insert(Rc::as_ptr(&h.backing) as *const () as usize) {
                    let taken = std::mem::take(&mut *h.backing.borrow_mut());
                    stack.extend(taken);
                }
            }
            _ => {}
        }
    }
}

fn run_error(rt: &Runtime, e: &RunError) -> HookError {
    let guard = rt
        .run_error_cause(e)
        .and_then(|c| c.downcast_ref::<GuardError>().copied());

    HookError::with_guard(e.error(), guard)
}

fn throw_error(rt: &mut Runtime, t: Throw) -> HookError {
    let e = rt.throw_to_run_error(t);
    run_error(rt, &e)
}

fn run_js(
    rt: &mut Runtime,
    c: &Rc<RunCtx>,
    program: &Arc<Program>,
    fn_name: &str,
    entity: &MapRef,
) -> Result<(), HookError> {
    if let Some(det) = &c.det {
        det.install(rt);
    }

    let mut ctx = DocMap::new();
    ctx.insert("entity".to_string(), DocValue::Map(entity.clone()));
    ctx.insert("query".to_string(), native("query", c, ctx_query));
    ctx.insert("count".to_string(), native("count", c, ctx_count));
    ctx.insert("sum".to_string(), native("sum", c, ctx_sum));
    ctx.insert("group".to_string(), native("group", c, ctx_group));
    let ctx: MapRef = Rc::new(RefCell::new(ctx));

    let out = (|| -> Result<(), HookError> {
        rt.set_value("ctx", &DocValue::Map(ctx.clone()))
            .map_err(|t| throw_error(rt, t))?;

        if c.plan.is_some() {
            let mut helper = DocMap::new();
            helper.insert("view".to_string(), native("view", c, helper_view));
            helper.insert("rows".to_string(), native("rows", c, helper_rows));
            helper.insert("fold".to_string(), native("fold", c, helper_fold));
            rt.set_value(FOLD_GLOBAL, &DocValue::new_map(helper))
                .map_err(|t| throw_error(rt, t))?;
        }

        rt.run_program(program).map_err(|e| run_error(rt, &e))?;

        let f = match rt.get_var(fn_name) {
            Ok(Some(f)) => f,
            Ok(None) => return Ok(()),
            Err(t) => return Err(throw_error(rt, t)),
        };
        if talos::runtime::assert_callable(rt, &f).is_none() {
            return Ok(());
        }

        let arg = match rt.get_var("ctx") {
            Ok(Some(v)) => v,
            Ok(None) => Value::Undefined,
            Err(t) => return Err(throw_error(rt, t)),
        };

        rt.call_function(&f, Value::Undefined, &[arg])
            .map_err(|e| run_error(rt, &e))?;

        Ok(())
    })();

    release_except(DocValue::Map(ctx), Rc::as_ptr(entity) as *const ());

    out
}

impl Request {
    pub(crate) fn apply_script(
        &self,
        h: &Arc<Hook>,
        fn_name: &str,
        entity: &MapRef,
    ) -> Result<(), HookError> {
        let compiled = compile(&h.script)?;
        let mut program = compiled.program.clone();

        let plan = plan_for(&h.script, fn_name);
        if let Some(p) = &plan {
            program = p.program.clone();
        }

        let mut key: Option<[u8; 32]> = None;
        let mut before = HashMap::new();
        let mut _leader = None;
        if let Some(mut input) = canonical(entity) {
            input.push(bool_byte(h.bypass_acl));
            let k = memo_key(&compiled.prefix, fn_name, &input);
            if let Some(o) = OUTPUTS.find(&k, |o| self.still_valid(o)) {
                o.apply(entity);
                return Ok(());
            }

            match OUTPUTS.begin(&k) {
                Some(t) => _leader = Some(t),
                None => {
                    if let Some(o) = OUTPUTS.find(&k, |o| self.still_valid(o)) {
                        o.apply(entity);
                        return Ok(());
                    }
                }
            }

            before = field_bytes(entity);
            key = Some(k);
        }

        let mut laned = false;
        let mut _lane = None;
        if COSTS.heavy(&h.script) {
            _lane = Some(api::acquire_hook_lane(self.guard()).map_err(HookError::from)?);
            laned = true;
        }

        let (stop_at, reason) = self.run_deadline(script_timeout());
        let mut run = Guard::until(self.inner.guard.clone(), Some(stop_at));
        run.laned = laned;

        let start = Instant::now();

        let mut holder = ManuallyDrop::new(Runtime::new());
        let interrupt = holder.interrupt_handle();
        let timer = TIMER.start(stop_at, interrupt.clone(), reason.clone());

        let c = Rc::new(RunCtx {
            req: self.clone(),
            hook: h.clone(),
            run: Arc::new(run),
            reason,
            outside: Cell::new(0),
            deps: RefCell::new(Vec::new()),
            plan,
            timer,
            interrupt,
            stop_at: Cell::new(stop_at),
            det: key.map(|_| Determinism::new()),
        });

        let res = catch_unwind(AssertUnwindSafe(|| {
            run_js(&mut holder, &c, &program, fn_name, entity)
        }));

        TIMER.stop(timer);
        let spent = (start.elapsed().as_nanos() as i64 - c.outside.get()).max(0);
        COSTS.observe(&h.script, spent);

        let res = match res {
            Ok(r) => {
                drop(ManuallyDrop::into_inner(holder));
                r
            }
            Err(p) => resume_unwind(p),
        };

        let broken = c
            .det
            .as_ref()
            .is_some_and(|d| d.broken.load(Ordering::SeqCst));
        let Some(key) = key else {
            return res;
        };
        if res.is_err() || broken {
            return res;
        }

        let deps = std::mem::take(&mut *c.deps.borrow_mut());
        if deps.iter().any(|d| d.version.is_empty()) {
            return Ok(());
        }

        if let Some(mut o) = diff_output(&before, entity) {
            o.deps = deps;
            OUTPUTS.store(&key, o);
        }

        Ok(())
    }
}

pub(crate) fn observe_fold(h: &Hook, rows: usize, hit: bool, start: Instant) {
    metrics::observe_hook_query(
        &h.id,
        &h.entity,
        &h.event,
        rows as i64,
        hit,
        start.elapsed(),
    );
}

fn bool_byte(b: bool) -> u8 {
    if b { 1 } else { 0 }
}

pub fn apply_post_read_script(
    username: &str,
    script: &str,
    entity: &MapRef,
    bypass_acl: bool,
) -> Result<(), HookError> {
    let h = Arc::new(Hook {
        script: script.to_string(),
        bypass_acl,
        ..Hook::default()
    });
    Request::new(username).apply_script(&h, "postRead", entity)
}

pub fn apply_pre_read_script(
    username: &str,
    script: &str,
    entity: &MapRef,
    bypass_acl: bool,
) -> Result<(), HookError> {
    let h = Arc::new(Hook {
        script: script.to_string(),
        bypass_acl,
        ..Hook::default()
    });
    Request::new(username).apply_script(&h, "preRead", entity)
}

const HEAVY_HOOK_AFTER: i64 = 5_000_000;
const MILLISECOND: i64 = 1_000_000;

struct CostTable {
    m: Mutex<Option<FxHashMap<String, i64>>>,
}

static COSTS: CostTable = CostTable {
    m: Mutex::new(None),
};

impl CostTable {
    fn heavy(&self, script: &str) -> bool {
        self.m
            .lock()
            .as_ref()
            .and_then(|m| m.get(script).copied())
            .is_some_and(|e| e >= HEAVY_HOOK_AFTER)
    }

    fn observe(&self, script: &str, d: i64) {
        let mut g = self.m.lock();
        let m = g.get_or_insert_with(FxHashMap::default);
        match m.get_mut(script) {
            None => {
                m.insert(script.to_string(), d);
            }
            Some(e) => {
                *e = (*e * 7 + d.min(8 * *e + MILLISECOND)) / 8;
            }
        }
    }
}
