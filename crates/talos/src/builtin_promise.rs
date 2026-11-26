use crate::array::go_append;
use crate::builtin_map::incompatible_receiver;
use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::{NativeCallable, NativeFn};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_OBJECT, CLASS_PROMISE, CallFn, FunctionCall, ObjData,
    ObjExt,
};
use crate::runtime::{Job, JsResult, Runtime, Throw, assert_callable};
use crate::string::ascii_string;
use crate::value::{Value, nil_safe, value_prop};
use std::cell::{Cell, RefCell};
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseState {
    Pending,
    Fulfilled,
    Rejected,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseRejectionOperation {
    Reject,
    Handle,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PromiseReactionType {
    Fulfill,
    Reject,
}

#[derive(Clone)]
pub enum JobCallback {
    Call(CallFn),
    Native(NativeFn),
}

impl JobCallback {
    pub fn trace(&self, t: &mut Tracer) {
        match self {
            JobCallback::Call(c) => t.obj(c.obj),
            JobCallback::Native(f) => f.trace(t),
        }
    }

    fn root(&self, rt: &mut Runtime) {
        match self {
            JobCallback::Call(c) => rt.root_obj(c.obj),
            JobCallback::Native(NativeFn::Closure(c)) => {
                for v in &c.data {
                    rt.root(v);
                }
            }
            JobCallback::Native(_) => {}
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PromiseCapability {
    pub promise: ObjRef,
    pub resolve_obj: ObjRef,
    pub reject_obj: ObjRef,
}

impl PromiseCapability {
    pub fn trace(&self, t: &mut Tracer) {
        t.obj(self.promise);
        t.obj(self.resolve_obj);
        t.obj(self.reject_obj);
    }

    fn root(&self, rt: &mut Runtime) {
        rt.root_obj(self.promise);
        rt.root_obj(self.resolve_obj);
        rt.root_obj(self.reject_obj);
    }

    fn data(&self) -> [Value; 3] {
        [
            Value::Object(self.promise),
            Value::Object(self.resolve_obj),
            Value::Object(self.reject_obj),
        ]
    }

    fn from_data(d: &[Value]) -> PromiseCapability {
        PromiseCapability {
            promise: data_obj(&d[0]),
            resolve_obj: data_obj(&d[1]),
            reject_obj: data_obj(&d[2]),
        }
    }

    pub fn resolve(&self, rt: &mut Runtime, result: Value) -> JsResult<()> {
        let f = rt.to_callable(&Value::Object(self.resolve_obj))?;
        f.call(rt, Value::Empty, &[result])?;
        Ok(())
    }

    pub fn reject(&self, rt: &mut Runtime, reason: Value) -> JsResult<()> {
        let f = rt.to_callable(&Value::Object(self.reject_obj))?;
        f.call(rt, Value::Empty, &[reason])?;
        Ok(())
    }

    pub fn r#try(
        &self,
        rt: &mut Runtime,
        f: impl FnOnce(&mut Runtime) -> JsResult<()>,
    ) -> JsResult<bool> {
        if let Some(ex) = rt.vm_try_result(f)? {
            self.reject(rt, ex.val)?;
            return Ok(false);
        }
        Ok(true)
    }
}

#[derive(Clone)]
pub struct PromiseReaction {
    pub capability: Option<PromiseCapability>,
    pub typ: PromiseReactionType,
    pub handler: Option<JobCallback>,
    pub async_runner: Option<ObjRef>,
}

impl PromiseReaction {
    pub fn trace(&self, t: &mut Tracer) {
        if let Some(c) = &self.capability {
            c.trace(t);
        }
        if let Some(h) = &self.handler {
            h.trace(t);
        }
        t.opt_obj(self.async_runner);
    }

    fn root(&self, rt: &mut Runtime) {
        if let Some(c) = &self.capability {
            c.root(rt);
        }
        if let Some(h) = &self.handler {
            h.root(rt);
        }
        if let Some(a) = self.async_runner {
            rt.root_obj(a);
        }
    }
}

pub struct Promise {
    pub state: PromiseState,
    pub result: Option<Value>,
    pub fulfill_reactions: Vec<PromiseReaction>,
    pub reject_reactions: Vec<PromiseReaction>,
    pub handled: bool,
}

impl ObjData for Promise {
    fn trace(&self, t: &mut Tracer) {
        if let Some(r) = &self.result {
            t.value(r);
        }
        for r in &self.fulfill_reactions {
            r.trace(t);
        }
        for r in &self.reject_reactions {
            r.trace(t);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn data_obj(v: &Value) -> ObjRef {
    match v {
        Value::Object(o) => *o,
        _ => unreachable!("closure data is not an object"),
    }
}

fn native_scope<R>(rt: &mut Runtime, f: impl FnOnce(&mut Runtime) -> R) -> R {
    let saved = rt.in_native;
    rt.in_native = true;
    let mark = rt.handle_scope_enter();
    let r = f(rt);
    rt.handle_scope_leave(mark);
    rt.in_native = saved;
    r
}

impl Promise {
    pub fn is_promise(rt: &Runtime, p: ObjRef) -> bool {
        rt.ext_dyn::<Promise>(p).is_some()
    }

    fn data(rt: &Runtime, p: ObjRef) -> &Promise {
        match rt.ext_dyn::<Promise>(p) {
            Some(d) => d,
            None => unreachable!("not a Promise"),
        }
    }

    fn data_mut(rt: &mut Runtime, p: ObjRef) -> &mut Promise {
        match rt.ext_dyn_mut::<Promise>(p) {
            Some(d) => d,
            None => unreachable!("not a Promise"),
        }
    }

    pub fn state(rt: &Runtime, p: ObjRef) -> PromiseState {
        Promise::data(rt, p).state
    }

    pub fn result(rt: &Runtime, p: ObjRef) -> Option<Value> {
        Promise::data(rt, p).result.clone()
    }

    pub fn create_resolving_functions(rt: &mut Runtime, p: ObjRef) -> (ObjRef, ObjRef) {
        let already_resolved = Rc::new(Cell::new(false));
        let already = already_resolved.clone();
        let resolve = rt.new_native_func(
            NativeFn::closure_with(
                vec![Value::Object(p)],
                move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                    if already.get() {
                        return Ok(Value::Undefined);
                    }
                    already.set(true);
                    let p = data_obj(&d[0]);
                    let resolution = call.argument(0);
                    if resolution.same_as(&Value::Object(p)) {
                        let e = r.new_type_error("Promise self-resolution");
                        return Ok(Promise::reject(r, p, e));
                    }
                    if let Value::Object(obj) = &resolution {
                        let obj = *obj;
                        let mut then_action = None;
                        let ex = r.vm_try_result(|r| {
                            then_action = obj.get_str(r, &"then".into(), None)?;
                            Ok(())
                        })?;
                        if let Some(ex) = ex {
                            return Ok(Promise::reject(r, p, ex.val));
                        }
                        if let Some(call) = then_action.and_then(|t| assert_callable(r, &t)) {
                            let job = r.new_promise_resolve_thenable_job(
                                p,
                                resolution,
                                JobCallback::Call(call),
                            );
                            r.enqueue_promise_job(job);
                            return Ok(Value::Undefined);
                        }
                    }
                    Ok(Promise::fulfill(r, p, resolution))
                },
            ),
            "",
            1,
        );
        let reject = rt.new_native_func(
            NativeFn::closure_with(
                vec![Value::Object(p)],
                move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                    if already_resolved.get() {
                        return Ok(Value::Undefined);
                    }
                    already_resolved.set(true);
                    let reason = call.argument(0);
                    Ok(Promise::reject(r, data_obj(&d[0]), reason))
                },
            ),
            "",
            1,
        );
        (resolve, reject)
    }

    pub fn reject(rt: &mut Runtime, p: ObjRef, reason: Value) -> Value {
        let (reactions, handled) = {
            let pd = Promise::data_mut(rt, p);
            let reactions = std::mem::take(&mut pd.reject_reactions);
            pd.result = Some(reason.clone());
            pd.fulfill_reactions = Vec::new();
            pd.state = PromiseState::Rejected;
            (reactions, pd.handled)
        };
        if !handled {
            rt.track_promise_rejection(p, PromiseRejectionOperation::Reject);
        }
        rt.trigger_promise_reactions(reactions, reason);
        Value::Undefined
    }

    pub fn fulfill(rt: &mut Runtime, p: ObjRef, value: Value) -> Value {
        let reactions = {
            let pd = Promise::data_mut(rt, p);
            let reactions = std::mem::take(&mut pd.fulfill_reactions);
            pd.result = Some(value.clone());
            pd.reject_reactions = Vec::new();
            pd.state = PromiseState::Fulfilled;
            reactions
        };
        rt.trigger_promise_reactions(reactions, value);
        Value::Undefined
    }

    pub fn add_reactions(
        rt: &mut Runtime,
        p: ObjRef,
        fulfill_reaction: PromiseReaction,
        reject_reaction: PromiseReaction,
    ) {
        let (state, result, handled) = {
            let pd = Promise::data(rt, p);
            (pd.state, pd.result.clone(), pd.handled)
        };
        match state {
            PromiseState::Pending => {
                let pd = Promise::data_mut(rt, p);
                pd.fulfill_reactions.push(fulfill_reaction);
                pd.reject_reactions.push(reject_reaction);
            }
            PromiseState::Fulfilled => {
                let job = rt.new_promise_reaction_job(fulfill_reaction, nil_safe(result));
                rt.enqueue_promise_job(job);
            }
            PromiseState::Rejected => {
                let reason = nil_safe(result);
                if !handled {
                    rt.track_promise_rejection(p, PromiseRejectionOperation::Handle);
                }
                let job = rt.new_promise_reaction_job(reject_reaction, reason);
                rt.enqueue_promise_job(job);
            }
        }
        Promise::data_mut(rt, p).handled = true;
    }
}

struct PromiseResolveThenableJob {
    p: ObjRef,
    thenable: Value,
    then: JobCallback,
}

impl Job for PromiseResolveThenableJob {
    fn run(self: Box<Self>, rt: &mut Runtime) -> JsResult<()> {
        let PromiseResolveThenableJob { p, thenable, then } = *self;
        native_scope(rt, |rt| {
            rt.root_obj(p);
            rt.root(&thenable);
            then.root(rt);
            let (resolve, reject) = Promise::create_resolving_functions(rt, p);
            let ex = rt.vm_try_result(|rt| {
                rt.call_job_callback(
                    &then,
                    thenable,
                    &[Value::Object(resolve), Value::Object(reject)],
                )?;
                Ok(())
            })?;
            if let Some(ex) = ex
                && let Some(f) = reject.assert_callable(rt)
            {
                f.call(rt, Value::Empty, &[ex.val])?;
            }
            Ok(())
        })
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.p);
        t.value(&self.thenable);
        self.then.trace(t);
    }
}

struct PromiseReactionJob {
    reaction: PromiseReaction,
    argument: Value,
}

impl Job for PromiseReactionJob {
    fn run(self: Box<Self>, rt: &mut Runtime) -> JsResult<()> {
        let PromiseReactionJob { reaction, argument } = *self;
        native_scope(rt, |rt| {
            reaction.root(rt);
            rt.root(&argument);
            let mut handler_result = Value::Undefined;
            let mut fulfill = false;
            match &reaction.handler {
                None => {
                    handler_result = argument;
                    if reaction.typ == PromiseReactionType::Fulfill {
                        fulfill = true;
                    }
                }
                Some(handler) => {
                    let ex = rt.vm_try_result(|rt| {
                        handler_result =
                            rt.call_job_callback(handler, Value::Undefined, &[argument])?;
                        fulfill = true;
                        Ok(())
                    })?;
                    if let Some(ex) = ex {
                        handler_result = ex.val;
                    }
                }
            }
            if let Some(capability) = &reaction.capability {
                if fulfill {
                    capability.resolve(rt, handler_result)?;
                } else {
                    capability.reject(rt, handler_result)?;
                }
            }
            Ok(())
        })
    }

    fn trace(&self, t: &mut Tracer) {
        self.reaction.trace(t);
        t.value(&self.argument);
    }
}

struct CapabilityExecutor {
    resolve: RefCell<Option<Value>>,
    reject: RefCell<Option<Value>>,
}

fn is_set(v: &RefCell<Option<Value>>) -> bool {
    matches!(&*v.borrow(), Some(v) if !v.is_empty())
}

impl NativeCallable for CapabilityExecutor {
    fn call(&self, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
        if is_set(&self.resolve) {
            return Err(Throw::Value(rt.new_type_error("resolve is already set")));
        }
        if is_set(&self.reject) {
            return Err(Throw::Value(rt.new_type_error("reject is already set")));
        }
        let arg = call.argument(0);
        if !arg.is_undefined() {
            *self.resolve.borrow_mut() = Some(arg);
        }
        let arg = call.argument(1);
        if !arg.is_undefined() {
            *self.reject.borrow_mut() = Some(arg);
        }
        Ok(Value::Empty)
    }

    fn trace(&self, t: &mut Tracer) {
        if let Some(v) = &*self.resolve.borrow() {
            t.value(v);
        }
        if let Some(v) = &*self.reject.borrow() {
            t.value(v);
        }
    }
}

struct CombinatorState {
    values: Vec<Value>,
    remaining_elements_count: i64,
}

impl ObjData for CombinatorState {
    fn trace(&self, t: &mut Tracer) {
        t.values(&self.values);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn new_combinator_state(rt: &mut Runtime) -> ObjRef {
    rt.alloc_object(
        &BASE_OBJECT_CLASS,
        BaseObject::new(CLASS_OBJECT, None),
        ObjExt::Dyn(Box::new(CombinatorState {
            values: Vec::new(),
            remaining_elements_count: 1,
        })),
    )
}

fn combinator_state(rt: &mut Runtime, o: ObjRef) -> &mut CombinatorState {
    match rt.ext_dyn_mut::<CombinatorState>(o) {
        Some(s) => s,
        None => unreachable!("not a combinator state"),
    }
}

fn push_element(rt: &mut Runtime, state: ObjRef) -> usize {
    let s = combinator_state(rt, state);
    go_append(&mut s.values, Value::Undefined);
    s.values.len() - 1
}

fn element_done(rt: &mut Runtime, state: ObjRef) -> Option<Vec<Value>> {
    let s = combinator_state(rt, state);
    s.remaining_elements_count -= 1;
    if s.remaining_elements_count == 0 {
        return Some(std::mem::take(&mut s.values));
    }
    None
}

fn closure_data(state: ObjRef, pcap: &PromiseCapability) -> Vec<Value> {
    let mut d = Vec::with_capacity(4);
    d.push(Value::Object(state));
    d.extend(pcap.data());
    d
}

fn new_aggregate_error(rt: &mut Runtime, errors: Vec<Value>) -> JsResult<Value> {
    let ae = rt.get_aggregate_error();
    let error = rt.builtin_new(ae, &[])?;
    let arr = rt.new_array_values(errors);
    error._put_prop(rt, &"errors".into(), Value::Object(arr), true, false, true);
    Ok(Value::Object(error))
}

fn all_settled_reaction(
    rt: &mut Runtime,
    state: ObjRef,
    pcap: &PromiseCapability,
    index: usize,
    already_called: Rc<Cell<bool>>,
    status: &'static str,
    value_key: &'static str,
) -> ObjRef {
    let status = Value::String(ascii_string(status));
    let mut data = closure_data(state, pcap);
    data.push(status);
    rt.new_native_func(
        NativeFn::closure_with(
            data,
            move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                if already_called.get() {
                    return Ok(Value::Undefined);
                }
                already_called.set(true);
                let state = data_obj(&d[0]);
                let pcap = PromiseCapability::from_data(&d[1..4]);
                let obj = r.new_object();
                obj._put_prop(r, &"status".into(), d[4].clone(), true, true, true);
                obj._put_prop(r, &value_key.into(), call.argument(0), true, true, true);
                combinator_state(r, state).values[index] = Value::Object(obj);
                if let Some(values) = element_done(r, state) {
                    let arr = r.new_array_values(values);
                    pcap.resolve(r, Value::Object(arr))?;
                }
                Ok(Value::Undefined)
            },
        ),
        "",
        1,
    )
}

impl Runtime {
    fn track_promise_rejection(&mut self, _p: ObjRef, _operation: PromiseRejectionOperation) {}

    fn call_job_callback(
        &mut self,
        job: &JobCallback,
        this: Value,
        args: &[Value],
    ) -> JsResult<Value> {
        match job {
            JobCallback::Call(c) => c.call(self, this, args),
            JobCallback::Native(f) => {
                let saved = self.in_native;
                self.in_native = true;
                let r = f.call(self, FunctionCall::new(&this, args));
                self.in_native = saved;
                let v = r?;
                self.root(&v);
                Ok(v)
            }
        }
    }

    pub fn new_promise_resolve_thenable_job(
        &mut self,
        p: ObjRef,
        thenable: Value,
        then: JobCallback,
    ) -> Box<dyn Job> {
        Box::new(PromiseResolveThenableJob { p, thenable, then })
    }

    pub fn enqueue_promise_job(&mut self, job: Box<dyn Job>) {
        self.job_queue.push_back(job);
    }

    pub fn trigger_promise_reactions(&mut self, reactions: Vec<PromiseReaction>, argument: Value) {
        for reaction in reactions {
            let job = self.new_promise_reaction_job(reaction, argument.clone());
            self.enqueue_promise_job(job);
        }
    }

    pub fn new_promise_reaction_job(
        &mut self,
        reaction: PromiseReaction,
        argument: Value,
    ) -> Box<dyn Job> {
        Box::new(PromiseReactionJob { reaction, argument })
    }

    pub fn new_promise(&mut self, proto: Option<ObjRef>) -> ObjRef {
        self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(Promise {
                state: PromiseState::Pending,
                result: None,
                fulfill_reactions: Vec::new(),
                reject_reactions: Vec::new(),
                handled: false,
            })),
        )
    }

    pub fn builtin_new_promise(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("Promise")));
        };
        let arg0 = args.first().cloned().unwrap_or(Value::Empty);
        let executor = self.to_callable(&arg0)?;

        let def_ctor = self.global.promise;
        let def_proto = self.get_promise_prototype();
        let proto = self.get_prototype_from_ctor(new_target, def_ctor, Some(def_proto))?;
        let po = self.new_promise(proto);

        let (resolve, reject) = Promise::create_resolving_functions(self, po);
        let ex = self.vm_try_result(|r| {
            executor.call(
                r,
                Value::Empty,
                &[Value::Object(resolve), Value::Object(reject)],
            )?;
            Ok(())
        })?;
        if let Some(ex) = ex
            && let Some(f) = reject.assert_callable(self)
        {
            f.call(self, Value::Empty, &[ex.val])?;
        }
        Ok(po)
    }

    pub fn promise_proto_then(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        if Promise::is_promise(self, this_obj) {
            let pc = self.get_promise();
            let c = self.species_constructor_obj(this_obj, pc)?;
            let result_capability = self.new_promise_capability(c)?;
            return Ok(self.perform_promise_then(
                this_obj,
                call.argument(0),
                call.argument(1),
                Some(result_capability),
            ));
        }
        Err(incompatible_receiver(
            self,
            "Promise.prototype.then",
            this_obj,
        ))
    }

    pub fn new_promise_capability(&mut self, c: ObjRef) -> JsResult<PromiseCapability> {
        if c == self.get_promise() {
            let pp = self.get_promise_prototype();
            let p = self.new_promise(Some(pp));
            let (resolve_obj, reject_obj) = Promise::create_resolving_functions(self, p);
            return Ok(PromiseCapability {
                promise: p,
                resolve_obj,
                reject_obj,
            });
        }
        let state = Rc::new(CapabilityExecutor {
            resolve: RefCell::new(None),
            reject: RefCell::new(None),
        });
        let executor = self.new_native_func(NativeFn::Dyn(state.clone()), "", 2);
        let ctor = self.to_constructor(&Value::Object(c))?;
        let promise = ctor.construct(self, &[Value::Object(executor)], Some(c))?;
        let resolve = state.resolve.borrow().clone().unwrap_or(Value::Empty);
        let resolve_obj = self.to_object(&resolve)?;
        self.to_callable(&Value::Object(resolve_obj))?;
        let reject = state.reject.borrow().clone().unwrap_or(Value::Empty);
        let reject_obj = self.to_object(&reject)?;
        self.to_callable(&Value::Object(reject_obj))?;
        Ok(PromiseCapability {
            promise,
            resolve_obj,
            reject_obj,
        })
    }

    pub fn perform_promise_then(
        &mut self,
        p: ObjRef,
        on_fulfilled: Value,
        on_rejected: Value,
        result_capability: Option<PromiseCapability>,
    ) -> Value {
        let on_fulfilled_job_callback = assert_callable(self, &on_fulfilled).map(JobCallback::Call);
        let on_rejected_job_callback = assert_callable(self, &on_rejected).map(JobCallback::Call);
        let fulfill_reaction = PromiseReaction {
            capability: result_capability,
            typ: PromiseReactionType::Fulfill,
            handler: on_fulfilled_job_callback,
            async_runner: None,
        };
        let reject_reaction = PromiseReaction {
            capability: result_capability,
            typ: PromiseReactionType::Reject,
            handler: on_rejected_job_callback,
            async_runner: None,
        };
        Promise::add_reactions(self, p, fulfill_reaction, reject_reaction);
        match result_capability {
            None => Value::Undefined,
            Some(c) => Value::Object(c.promise),
        }
    }

    pub fn promise_proto_catch(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.invoke(
            call.this,
            &"then".into(),
            &[Value::Undefined, call.argument(0)],
        )
    }

    pub fn promise_resolve(&mut self, c: ObjRef, x: Value) -> JsResult<ObjRef> {
        if let Value::Object(obj) = &x {
            let x_constructor = nil_safe(obj.get_str(self, &"constructor".into(), None)?);
            if x_constructor.same_as(&Value::Object(c)) {
                return Ok(*obj);
            }
        }
        let pcap = self.new_promise_capability(c)?;
        pcap.resolve(self, x)?;
        Ok(pcap.promise)
    }

    pub fn promise_proto_finally(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let promise = self.to_object(call.this)?;
        let pc = self.get_promise();
        let c = self.species_constructor_obj(promise, pc)?;
        let on_finally = call.argument(0);
        let (then_finally, catch_finally) = match assert_callable(self, &on_finally) {
            None => (on_finally.clone(), on_finally),
            Some(on_finally_fn) => {
                let f = on_finally_fn.f;
                let data = vec![Value::Object(c), Value::Object(on_finally_fn.obj)];
                let then_finally = self.new_native_func(
                    NativeFn::closure_with(
                        data.clone(),
                        move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                            let value = call.argument(0);
                            let on_finally_fn = CallFn {
                                obj: data_obj(&d[1]),
                                f,
                            };
                            let result = on_finally_fn.call(r, Value::Empty, &[])?;
                            let promise = r.promise_resolve(data_obj(&d[0]), result)?;
                            let value_thunk = r.new_native_func(
                                NativeFn::closure_with(
                                    vec![value],
                                    |_r: &mut Runtime, d: &[Value], _call: FunctionCall<'_>| {
                                        Ok(d[0].clone())
                                    },
                                ),
                                "",
                                0,
                            );
                            r.invoke(
                                &Value::Object(promise),
                                &"then".into(),
                                &[Value::Object(value_thunk)],
                            )
                        },
                    ),
                    "",
                    1,
                );
                let catch_finally = self.new_native_func(
                    NativeFn::closure_with(
                        data,
                        move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                            let reason = call.argument(0);
                            let on_finally_fn = CallFn {
                                obj: data_obj(&d[1]),
                                f,
                            };
                            let result = on_finally_fn.call(r, Value::Empty, &[])?;
                            let promise = r.promise_resolve(data_obj(&d[0]), result)?;
                            let thrower = r.new_native_func(
                                NativeFn::closure_with(
                                    vec![reason],
                                    |_r: &mut Runtime, d: &[Value], _call: FunctionCall<'_>| {
                                        Err(Throw::Value(d[0].clone()))
                                    },
                                ),
                                "",
                                0,
                            );
                            r.invoke(
                                &Value::Object(promise),
                                &"then".into(),
                                &[Value::Object(thrower)],
                            )
                        },
                    ),
                    "",
                    1,
                );
                (Value::Object(then_finally), Value::Object(catch_finally))
            }
        };
        self.invoke(
            &Value::Object(promise),
            &"then".into(),
            &[then_finally, catch_finally],
        )
    }

    pub fn promise_all(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let pcap = self.new_promise_capability(c)?;
        let iterable = call.argument(0);

        pcap.r#try(self, |r| {
            let resolve = nil_safe(c.get_str(r, &"resolve".into(), None)?);
            let promise_resolve = r.to_callable(&resolve)?;
            let mut iter = r.get_iterator(&iterable, None)?;
            let state = new_combinator_state(r);
            iter.iterate(r, &mut |r, next_value| {
                let index = push_element(r, state);
                let next_promise = promise_resolve.call(r, Value::Object(c), &[next_value])?;
                let already_called = Cell::new(false);
                let on_fulfilled = r.new_native_func(
                    NativeFn::closure_with(
                        closure_data(state, &pcap),
                        move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                            if already_called.get() {
                                return Ok(Value::Undefined);
                            }
                            already_called.set(true);
                            let state = data_obj(&d[0]);
                            let pcap = PromiseCapability::from_data(&d[1..4]);
                            combinator_state(r, state).values[index] = call.argument(0);
                            if let Some(values) = element_done(r, state) {
                                let arr = r.new_array_values(values);
                                pcap.resolve(r, Value::Object(arr))?;
                            }
                            Ok(Value::Undefined)
                        },
                    ),
                    "",
                    1,
                );
                combinator_state(r, state).remaining_elements_count += 1;
                r.invoke(
                    &next_promise,
                    &"then".into(),
                    &[Value::Object(on_fulfilled), Value::Object(pcap.reject_obj)],
                )?;
                Ok(())
            })?;
            if let Some(values) = element_done(r, state) {
                let arr = r.new_array_values(values);
                pcap.resolve(r, Value::Object(arr))?;
            }
            Ok(())
        })?;
        Ok(Value::Object(pcap.promise))
    }

    pub fn promise_all_settled(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let pcap = self.new_promise_capability(c)?;
        let iterable = call.argument(0);

        pcap.r#try(self, |r| {
            let resolve = nil_safe(c.get_str(r, &"resolve".into(), None)?);
            let promise_resolve = r.to_callable(&resolve)?;
            let mut iter = r.get_iterator(&iterable, None)?;
            let state = new_combinator_state(r);
            iter.iterate(r, &mut |r, next_value| {
                let index = push_element(r, state);
                let next_promise = promise_resolve.call(r, Value::Object(c), &[next_value])?;
                let already_called = Rc::new(Cell::new(false));
                let on_fulfilled = all_settled_reaction(
                    r,
                    state,
                    &pcap,
                    index,
                    already_called.clone(),
                    "fulfilled",
                    "value",
                );
                let on_rejected = all_settled_reaction(
                    r,
                    state,
                    &pcap,
                    index,
                    already_called,
                    "rejected",
                    "reason",
                );
                combinator_state(r, state).remaining_elements_count += 1;
                r.invoke(
                    &next_promise,
                    &"then".into(),
                    &[Value::Object(on_fulfilled), Value::Object(on_rejected)],
                )?;
                Ok(())
            })?;
            if let Some(values) = element_done(r, state) {
                let arr = r.new_array_values(values);
                pcap.resolve(r, Value::Object(arr))?;
            }
            Ok(())
        })?;
        Ok(Value::Object(pcap.promise))
    }

    pub fn promise_any(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let pcap = self.new_promise_capability(c)?;
        let iterable = call.argument(0);

        pcap.r#try(self, |r| {
            let resolve = nil_safe(c.get_str(r, &"resolve".into(), None)?);
            let promise_resolve = r.to_callable(&resolve)?;
            let mut iter = r.get_iterator(&iterable, None)?;
            let state = new_combinator_state(r);
            iter.iterate(r, &mut |r, next_value| {
                let index = push_element(r, state);
                let next_promise = promise_resolve.call(r, Value::Object(c), &[next_value])?;
                let already_called = Cell::new(false);
                let on_rejected = r.new_native_func(
                    NativeFn::closure_with(
                        closure_data(state, &pcap),
                        move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                            if already_called.get() {
                                return Ok(Value::Undefined);
                            }
                            already_called.set(true);
                            let state = data_obj(&d[0]);
                            let pcap = PromiseCapability::from_data(&d[1..4]);
                            combinator_state(r, state).values[index] = call.argument(0);
                            if let Some(errors) = element_done(r, state) {
                                let error = new_aggregate_error(r, errors)?;
                                pcap.reject(r, error)?;
                            }
                            Ok(Value::Undefined)
                        },
                    ),
                    "",
                    1,
                );
                combinator_state(r, state).remaining_elements_count += 1;
                r.invoke(
                    &next_promise,
                    &"then".into(),
                    &[Value::Object(pcap.resolve_obj), Value::Object(on_rejected)],
                )?;
                Ok(())
            })?;
            if let Some(errors) = element_done(r, state) {
                let error = new_aggregate_error(r, errors)?;
                pcap.reject(r, error)?;
            }
            Ok(())
        })?;
        Ok(Value::Object(pcap.promise))
    }

    pub fn promise_race(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let pcap = self.new_promise_capability(c)?;
        let iterable = call.argument(0);

        pcap.r#try(self, |r| {
            let resolve = nil_safe(c.get_str(r, &"resolve".into(), None)?);
            let promise_resolve = r.to_callable(&resolve)?;
            let mut iter = r.get_iterator(&iterable, None)?;
            iter.iterate(r, &mut |r, next_value| {
                let next_promise = promise_resolve.call(r, Value::Object(c), &[next_value])?;
                r.invoke(
                    &next_promise,
                    &"then".into(),
                    &[
                        Value::Object(pcap.resolve_obj),
                        Value::Object(pcap.reject_obj),
                    ],
                )?;
                Ok(())
            })
        })?;
        Ok(Value::Object(pcap.promise))
    }

    pub fn promise_reject(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let pcap = self.new_promise_capability(c)?;
        pcap.reject(self, call.argument(0))?;
        Ok(Value::Object(pcap.promise))
    }

    pub fn promise_resolve_builtin(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        Ok(Value::Object(self.promise_resolve(c, call.argument(0))?))
    }

    fn create_promise_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let o = val;
        let c = self.get_promise();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(c),
            true,
            false,
            true,
        );

        let f = self.new_native_func(NativeFn::Fn(|r, c| r.promise_proto_catch(c)), "catch", 1);
        o._put_prop(self, &"catch".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.promise_proto_finally(c)),
            "finally",
            1,
        );
        o._put_prop(self, &"finally".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.promise_proto_then(c)), "then", 2);
        o._put_prop(self, &"then".into(), Value::Object(f), true, false, true);

        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_PROMISE)),
                false,
                false,
                true,
            ),
        );
    }

    fn create_promise(&mut self, val: ObjRef) {
        let proto = self.get_promise_prototype();
        let o = self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_promise(args, nt),
            Some(proto),
            "Promise",
            1,
        );

        let f = self.new_native_func(NativeFn::Fn(|r, c| r.promise_all(c)), "all", 1);
        o._put_prop(self, &"all".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.promise_all_settled(c)),
            "allSettled",
            1,
        );
        o._put_prop(
            self,
            &"allSettled".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.promise_any(c)), "any", 1);
        o._put_prop(self, &"any".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.promise_race(c)), "race", 1);
        o._put_prop(self, &"race".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.promise_reject(c)), "reject", 1);
        o._put_prop(self, &"reject".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.promise_resolve_builtin(c)),
            "resolve",
            1,
        );
        o._put_prop(self, &"resolve".into(), Value::Object(f), true, false, true);

        self.put_species_return_this(o);
    }

    pub fn get_promise_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.promise_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.promise_prototype = Some(ret);
        self.create_promise_proto(ret);
        ret
    }

    pub fn get_promise(&mut self) -> ObjRef {
        if let Some(r) = self.global.promise {
            return r;
        }
        let ret = self.reserve_object();
        self.global.promise = Some(ret);
        self.create_promise(ret);
        ret
    }
}
