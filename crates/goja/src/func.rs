use crate::array::go_append;
use crate::builtin_promise::{
    JobCallback, Promise, PromiseCapability, PromiseReaction, PromiseReactionType,
};
use crate::compiler::Program;
use crate::heap::{ObjRef, StashRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CallFn, ConstructorCall, CtorFn, FunctionCall, ObjClass, ObjData,
    ObjExt, PrivateEnv, PrivateEnvType, UStr, instance_of_operator, to_method,
};
use crate::runtime::{
    Exception, IteratorRecord, JsResult, Runtime, Throw, iterator_complete, iterator_value,
};
use crate::string::{JsString, STRING_FUNCTION, new_string_value, string_value_from_raw};
use crate::unistring;
use crate::value::{Value, ValueProperty, nil_safe};
use crate::vm::{Context, ExecCtx, TRY_PANIC_MARKER};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ResultType {
    Normal = 0,
    Yield = 1,
    YieldRes = 2,
    YieldDelegate = 3,
    YieldDelegateRes = 4,
    Await = 5,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum YieldMarker {
    Await,
    Yield,
    YieldRes,
    YieldDelegate,
    YieldDelegateRes,
    YieldEmpty,
}

impl YieldMarker {
    pub fn result_type(self) -> ResultType {
        match self {
            YieldMarker::Await => ResultType::Await,
            YieldMarker::Yield | YieldMarker::YieldEmpty => ResultType::Yield,
            YieldMarker::YieldRes => ResultType::YieldRes,
            YieldMarker::YieldDelegate => ResultType::YieldDelegate,
            YieldMarker::YieldDelegateRes => ResultType::YieldDelegateRes,
        }
    }
}

pub type NativeFnPtr = fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value>;
pub type NativeCtorPtr = fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>;

pub trait NativeCallable {
    fn call(&self, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value>;
    fn trace(&self, t: &mut Tracer);
    fn as_any(&self) -> Option<&dyn std::any::Any> {
        None
    }
}

pub trait NativeConstructor {
    fn construct(
        &self,
        rt: &mut Runtime,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef>;
    fn trace(&self, t: &mut Tracer);
}

type ClosureFn = dyn Fn(&mut Runtime, &[Value], FunctionCall<'_>) -> JsResult<Value>;
type ClosureCtor = dyn Fn(&mut Runtime, &[Value], &[Value], Option<ObjRef>) -> JsResult<ObjRef>;

pub struct DataClosure {
    pub data: Vec<Value>,
    f: Box<ClosureFn>,
}

pub struct DataCtorClosure {
    pub data: Vec<Value>,
    f: Box<ClosureCtor>,
}

#[derive(Clone)]
pub enum NativeFn {
    Fn(NativeFnPtr),
    Closure(Rc<DataClosure>),
    Dyn(Rc<dyn NativeCallable>),
}

#[derive(Clone)]
pub enum NativeCtor {
    Fn(NativeCtorPtr),
    Closure(Rc<DataCtorClosure>),
    Dyn(Rc<dyn NativeConstructor>),
}

impl NativeFn {
    pub fn closure_with(
        data: Vec<Value>,
        f: impl Fn(&mut Runtime, &[Value], FunctionCall<'_>) -> JsResult<Value> + 'static,
    ) -> NativeFn {
        NativeFn::Closure(Rc::new(DataClosure {
            data,
            f: Box::new(f),
        }))
    }

    pub fn call(&self, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
        match self {
            NativeFn::Fn(f) => f(rt, call),
            NativeFn::Closure(c) => (c.f)(rt, &c.data, call),
            NativeFn::Dyn(d) => d.call(rt, call),
        }
    }

    pub fn trace(&self, t: &mut Tracer) {
        match self {
            NativeFn::Fn(_) => {}
            NativeFn::Closure(c) => t.values(&c.data),
            NativeFn::Dyn(d) => d.trace(t),
        }
    }
}

impl NativeCtor {
    pub fn closure_with(
        data: Vec<Value>,
        f: impl Fn(&mut Runtime, &[Value], &[Value], Option<ObjRef>) -> JsResult<ObjRef> + 'static,
    ) -> NativeCtor {
        NativeCtor::Closure(Rc::new(DataCtorClosure {
            data,
            f: Box::new(f),
        }))
    }

    pub fn construct(
        &self,
        rt: &mut Runtime,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        match self {
            NativeCtor::Fn(f) => f(rt, args, new_target),
            NativeCtor::Closure(c) => (c.f)(rt, &c.data, args, new_target),
            NativeCtor::Dyn(d) => d.construct(rt, args, new_target),
        }
    }

    pub fn trace(&self, t: &mut Tracer) {
        match self {
            NativeCtor::Fn(_) => {}
            NativeCtor::Closure(c) => t.values(&c.data),
            NativeCtor::Dyn(d) => d.trace(t),
        }
    }
}

pub struct NativeFuncObject {
    pub f: Option<NativeFn>,
    pub construct: Option<NativeCtor>,
    pub wrapped: Option<ObjRef>,
}

impl NativeFuncObject {
    pub fn trace(&self, t: &mut Tracer) {
        if let Some(f) = &self.f {
            f.trace(t);
        }
        if let Some(c) = &self.construct {
            c.trace(t);
        }
        t.opt_obj(self.wrapped);
    }

    fn data(rt: &Runtime, o: ObjRef) -> &NativeFuncObject {
        match &rt.obj(o).ext {
            ObjExt::NativeFunc(n) => n,
            _ => unreachable!("not a native function"),
        }
    }

    pub fn source(rt: &mut Runtime, o: ObjRef) -> JsResult<JsString> {
        let n = nil_safe(o.get_str(rt, &"name".into(), None)?);
        let n = n.to_string(rt)?;
        Ok(new_string_value(
            format!("function {}() {{ [native code] }}", n.to_rust_string()).as_bytes(),
        ))
    }

    pub fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        let f = NativeFuncObject::data(rt, o).f.clone();
        match f {
            Some(f) => {
                let saved = rt.in_native;
                rt.in_native = true;
                let r = f.call(rt, call);
                rt.in_native = saved;
                r
            }
            None => Ok(Value::Undefined),
        }
    }

    pub fn construct_impl(
        rt: &mut Runtime,
        o: ObjRef,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let c = NativeFuncObject::data(rt, o).construct.clone();
        match c {
            Some(c) => {
                let saved = rt.in_native;
                rt.in_native = true;
                let r = c.construct(rt, args, new_target);
                rt.in_native = saved;
                r
            }
            None => Err(Throw::TypeError("Not a constructor".to_string())),
        }
    }

    pub fn assert_callable(rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        if NativeFuncObject::data(rt, o).f.is_some() {
            return Some(CallFn {
                obj: o,
                f: NativeFuncObject::call,
            });
        }
        None
    }

    pub fn assert_constructor(rt: &Runtime, o: ObjRef) -> Option<CtorFn> {
        if NativeFuncObject::data(rt, o).construct.is_some() {
            return Some(CtorFn {
                obj: o,
                f: NativeFuncObject::construct_impl,
            });
        }
        None
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        let f = NativeFuncObject::data(rt, o).f.clone();
        native_vm_call(rt, f, n)
    }

    pub fn default_construct(
        rt: &mut Runtime,
        o: ObjRef,
        ccall: fn(&mut Runtime, ConstructorCall<'_>) -> JsResult<Option<ObjRef>>,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let obj = BaseFuncObject::create_instance(rt, o, new_target)?;
        let ret = ccall(
            rt,
            ConstructorCall {
                this: obj,
                arguments: args,
                new_target,
            },
        )?;
        Ok(ret.unwrap_or(obj))
    }
}

pub fn native_vm_call(rt: &mut Runtime, f: Option<NativeFn>, n: usize) -> JsResult<()> {
    let sp = rt.vm.sp as usize;
    match f {
        Some(f) => {
            rt.vm_push_ctx_for_call()?;
            rt.vm.prg = None;
            rt.vm.sb = (sp - n) as isize;
            let this = rt.vm.stack[sp - n - 2].clone();
            let mut buf = rt.vm.take_arg_buf();
            buf.extend_from_slice(&rt.vm.stack[sp - n..sp]);
            let saved = rt.in_native;
            let mark = rt.handles.len();
            rt.in_native = true;
            let ret = f.call(rt, FunctionCall::new(&this, &buf));
            rt.in_native = saved;
            rt.handles.truncate(mark);
            rt.vm.return_arg_buf(buf);
            let ret = ret?;
            rt.vm.stack[sp - n - 2] = if ret.is_empty() {
                Value::Undefined
            } else {
                ret
            };
            rt.vm.pop_ctx();
        }
        None => {
            rt.vm.stack[sp - n - 2] = Value::Undefined;
        }
    }
    rt.vm.sp -= n as isize + 1;
    rt.vm.pc += 1;
    Ok(())
}

pub struct BaseFuncObject;

impl BaseFuncObject {
    pub fn type_of(_rt: &Runtime, _o: ObjRef) -> JsString {
        STRING_FUNCTION.clone()
    }

    pub fn has_instance(rt: &mut Runtime, o: ObjRef, v: &Value) -> JsResult<bool> {
        has_instance(rt, o, v)
    }

    pub fn create_instance(
        rt: &mut Runtime,
        o: ObjRef,
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let new_target = new_target.unwrap_or(o);
        let op = rt.global.object_prototype;
        let proto = rt.get_prototype_from_ctor(new_target, None, op)?;
        Ok(rt.new_base_object(proto, crate::object::CLASS_OBJECT))
    }
}

pub fn base_func_init(rt: &mut Runtime, o: ObjRef, name: &unistring::String, length: Value) {
    let len_prop = ValueProperty {
        configurable: true,
        value: length,
        ..ValueProperty::default()
    };
    rt.obj_mut(o)
        .base
        ._put(&crate::vm::STR_LENGTH, Value::Property(Box::new(len_prop)));
    o._put_prop(
        rt,
        &crate::vm::STR_NAME,
        Value::String(string_value_from_raw(name)),
        false,
        false,
        true,
    );
}

pub fn has_instance(rt: &mut Runtime, val: ObjRef, v: &Value) -> JsResult<bool> {
    if let Value::Object(v) = v {
        let o = val.get_str(rt, &crate::vm::STR_PROTOTYPE, None)?;
        if let Some(Value::Object(o1)) = o {
            let mut v = *v;
            loop {
                match v.proto(rt)? {
                    None => return Ok(false),
                    Some(p) => {
                        if p == o1 {
                            return Ok(true);
                        }
                        v = p;
                    }
                }
            }
        } else {
            return Err(Throw::Value(
                rt.new_type_error("prototype is not an object"),
            ));
        }
    }
    Ok(false)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FuncKind {
    Func,
    Generator,
    Async,
    Class,
    Method,
    GeneratorMethod,
    AsyncMethod,
    Arrow,
    AsyncArrow,
}

pub struct BaseJsFuncObject {
    pub kind: FuncKind,
    pub stash: Option<StashRef>,
    pub priv_env: Option<Rc<PrivateEnv>>,
    pub prg: Option<Arc<Program>>,
    pub src: Arc<[u8]>,
    pub strict: bool,
    pub init_fields: Option<Arc<Program>>,
    pub computed_keys: Vec<Value>,
    pub private_env_type: Option<Arc<PrivateEnvType>>,
    pub private_methods: Vec<Value>,
    pub derived: bool,
    pub home_object: Option<ObjRef>,
    pub func_obj: Option<ObjRef>,
    pub new_target: Value,
}

impl BaseJsFuncObject {
    pub fn new(kind: FuncKind, strict: bool) -> BaseJsFuncObject {
        BaseJsFuncObject {
            kind,
            stash: None,
            priv_env: None,
            prg: None,
            src: Arc::from(&b""[..]),
            strict,
            init_fields: None,
            computed_keys: Vec::new(),
            private_env_type: None,
            private_methods: Vec::new(),
            derived: false,
            home_object: None,
            func_obj: None,
            new_target: Value::Empty,
        }
    }

    pub fn trace(&self, t: &mut Tracer) {
        if let Some(s) = self.stash {
            t.stash(s);
        }
        t.values(&self.computed_keys);
        t.values(&self.private_methods);
        t.opt_obj(self.home_object);
        t.opt_obj(self.func_obj);
        t.value(&self.new_target);
    }

    pub fn data(rt: &Runtime, o: ObjRef) -> &BaseJsFuncObject {
        match &rt.obj(o).ext {
            ObjExt::JsFunc(f) => f,
            _ => unreachable!("not a script function"),
        }
    }

    pub fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut BaseJsFuncObject {
        match &mut rt.obj_mut(o).ext {
            ObjExt::JsFunc(f) => f,
            _ => unreachable!("not a script function"),
        }
    }

    pub fn source(rt: &mut Runtime, o: ObjRef) -> JsResult<JsString> {
        Ok(new_string_value(&BaseJsFuncObject::data(rt, o).src))
    }

    pub fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        crate::vm::js_func_call(rt, o, call, Value::Empty)
    }

    pub fn assert_callable(_rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        Some(CallFn {
            obj: o,
            f: BaseJsFuncObject::call,
        })
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        crate::vm::js_func_vm_call(rt, o, n)
    }

    pub fn construct(
        rt: &mut Runtime,
        o: ObjRef,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        crate::vm::js_func_construct(rt, o, args, new_target)
    }
}

pub struct FuncObject;

impl FuncObject {
    fn _add_proto(rt: &mut Runtime, o: ObjRef, n: &UStr) -> Option<Value> {
        if n == "prototype" && !rt.obj(o).base.values.contains_key(n) {
            return Some(FuncObject::add_prototype(rt, o));
        }
        None
    }

    pub fn add_prototype(rt: &mut Runtime, o: ObjRef) -> Value {
        let proto = rt.new_object();
        proto._put_prop(
            rt,
            &crate::vm::STR_CONSTRUCTOR,
            Value::Object(o),
            true,
            false,
            true,
        );
        o._put_prop(
            rt,
            &crate::vm::STR_PROTOTYPE,
            Value::Object(proto),
            true,
            false,
            false,
        )
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        FuncObject::_add_proto(rt, o, p);
        BaseObject::get_str(rt, o, p, receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        if let Some(v) = FuncObject::_add_proto(rt, o, name) {
            return Ok(Some(v));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        FuncObject::_add_proto(rt, o, name);
        BaseObject::set_own_str(rt, o, name, val, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = o.get_own_prop_str(rt, name)?;
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: crate::object::PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        FuncObject::_add_proto(rt, o, name);
        BaseObject::define_own_property_str(rt, o, name, descr, throw)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        FuncObject::_add_proto(rt, o, name);
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        if BaseObject::has_own_property_str(rt, o, name)? {
            return Ok(true);
        }
        Ok(name == "prototype")
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        if all && !rt.obj(o).base.values.contains_key(&UStr::from("prototype")) {
            go_append(
                &mut accum,
                Value::String(crate::string::ascii_string("prototype")),
            );
        }
        BaseObject::string_keys(rt, o, all, accum)
    }

    pub fn iterate_string_keys(
        rt: &mut Runtime,
        o: ObjRef,
    ) -> JsResult<crate::object::IterNextFunc> {
        if !rt.obj(o).base.values.contains_key(&UStr::from("prototype")) {
            FuncObject::add_prototype(rt, o);
        }
        BaseObject::iterate_string_keys(rt, o)
    }

    pub fn assert_constructor(_rt: &Runtime, o: ObjRef) -> Option<CtorFn> {
        Some(CtorFn {
            obj: o,
            f: BaseJsFuncObject::construct,
        })
    }
}

pub struct ArrowFuncObject;

impl ArrowFuncObject {
    pub fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        let nt = BaseJsFuncObject::data(rt, o).new_target.clone();
        crate::vm::js_func_call_args(rt, o, call.arguments, nt, Value::Empty)
    }

    pub fn assert_callable(_rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        Some(CallFn {
            obj: o,
            f: ArrowFuncObject::call,
        })
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        crate::vm::arrow_func_vm_call(rt, o, n)
    }
}

pub struct ClassFuncObject;

impl ClassFuncObject {
    pub fn call(rt: &mut Runtime, _o: ObjRef, _call: FunctionCall<'_>) -> JsResult<Value> {
        Err(Throw::Value(rt.new_type_error(
            "Class constructor cannot be invoked without 'new'",
        )))
    }

    pub fn assert_callable(_rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        Some(CallFn {
            obj: o,
            f: ClassFuncObject::call,
        })
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, _n: usize) -> JsResult<()> {
        ClassFuncObject::call(rt, o, FunctionCall::new(&Value::Undefined, &[])).map(|_| ())
    }

    pub fn construct(
        rt: &mut Runtime,
        o: ObjRef,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        crate::vm::class_func_construct(rt, o, args, new_target)
    }

    pub fn assert_constructor(_rt: &Runtime, o: ObjRef) -> Option<CtorFn> {
        Some(CtorFn {
            obj: o,
            f: ClassFuncObject::construct,
        })
    }
}

pub struct BoundFuncObject;

impl BoundFuncObject {
    pub fn has_instance(rt: &mut Runtime, o: ObjRef, v: &Value) -> JsResult<bool> {
        let wrapped = match &rt.obj(o).ext {
            ObjExt::NativeFunc(n) => n.wrapped,
            _ => None,
        };
        match wrapped {
            Some(w) => instance_of_operator(rt, v, w),
            None => Ok(false),
        }
    }
}

#[derive(Default)]
pub struct Generator {
    pub ctx: ExecCtx,
    pub try_stack_len: u32,
    pub iter_stack_len: u32,
    pub ref_stack_len: u32,
}

impl Generator {
    pub fn trace(&self, t: &mut Tracer) {
        self.ctx.trace(t);
    }

    fn store_lengths(&mut self, rt: &Runtime) {
        self.try_stack_len = rt.vm.try_stack.len() as u32;
        self.iter_stack_len = rt.vm.iter_stack.len() as u32;
        self.ref_stack_len = rt.vm.ref_stack.len() as u32;
    }
}

pub struct GeneratorObject {
    pub gen_: Generator,
    pub delegated: Option<IteratorRecord>,
    pub state: GeneratorState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum GeneratorState {
    #[default]
    Undefined,
    SuspendedStart,
    Executing,
    SuspendedYield,
    SuspendedYieldRes,
    Completed,
}

impl GeneratorObject {
    pub fn trace(&self, t: &mut Tracer) {
        self.gen_.trace(t);
        if let Some(d) = &self.delegated {
            d.trace(t);
        }
    }
}

pub type VmCallFn = fn(&mut Runtime, ObjRef, usize) -> JsResult<()>;

pub type StepResult = (Value, ResultType, Option<Box<Exception>>);

pub const BASE_FUNC_CLASS: ObjClass = ObjClass {
    type_of: BaseFuncObject::type_of,
    has_instance: BaseFuncObject::has_instance,
    ..BASE_CLASS
};

pub const BASE_JS_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: BaseJsFuncObject::assert_callable,
    vm_call: BaseJsFuncObject::vm_call,
    ..BASE_FUNC_CLASS
};

pub static FUNC_CLASS: ObjClass = ObjClass {
    get_str: FuncObject::get_str,
    get_own_prop_str: FuncObject::get_own_prop_str,
    set_own_str: FuncObject::set_own_str,
    set_foreign_str: FuncObject::set_foreign_str,
    define_own_property_str: FuncObject::define_own_property_str,
    delete_str: FuncObject::delete_str,
    has_own_property_str: FuncObject::has_own_property_str,
    string_keys: FuncObject::string_keys,
    iterate_string_keys: FuncObject::iterate_string_keys,
    assert_constructor: FuncObject::assert_constructor,
    ..BASE_JS_FUNC_CLASS
};

pub static METHOD_FUNC_CLASS: ObjClass = BASE_JS_FUNC_CLASS;

pub static ARROW_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: ArrowFuncObject::assert_callable,
    vm_call: ArrowFuncObject::vm_call,
    ..BASE_JS_FUNC_CLASS
};

pub static CLASS_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: ClassFuncObject::assert_callable,
    vm_call: ClassFuncObject::vm_call,
    assert_constructor: ClassFuncObject::assert_constructor,
    ..BASE_JS_FUNC_CLASS
};

pub static GENERATOR_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: GeneratorFuncObject::assert_callable,
    vm_call: GeneratorFuncObject::vm_call,
    assert_constructor: GeneratorFuncObject::assert_constructor,
    ..BASE_JS_FUNC_CLASS
};

pub static GENERATOR_METHOD_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: GeneratorFuncObject::assert_callable,
    vm_call: GeneratorFuncObject::vm_call,
    ..BASE_JS_FUNC_CLASS
};

pub static ASYNC_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: AsyncFuncObject::assert_callable,
    vm_call: AsyncFuncObject::vm_call,
    ..BASE_JS_FUNC_CLASS
};

pub static ASYNC_METHOD_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: AsyncFuncObject::assert_callable,
    vm_call: AsyncFuncObject::vm_call,
    ..BASE_JS_FUNC_CLASS
};

pub static ASYNC_ARROW_FUNC_CLASS: ObjClass = ObjClass {
    assert_callable: AsyncArrowFuncObject::assert_callable,
    vm_call: AsyncArrowFuncObject::vm_call,
    ..BASE_JS_FUNC_CLASS
};

pub const NATIVE_FUNC_CLASS_CONST: ObjClass = ObjClass {
    assert_callable: NativeFuncObject::assert_callable,
    vm_call: NativeFuncObject::vm_call,
    assert_constructor: NativeFuncObject::assert_constructor,
    ..BASE_FUNC_CLASS
};

pub static NATIVE_FUNC_CLASS: ObjClass = NATIVE_FUNC_CLASS_CONST;

pub static BOUND_FUNC_CLASS: ObjClass = ObjClass {
    has_instance: BoundFuncObject::has_instance,
    ..NATIVE_FUNC_CLASS_CONST
};

pub static GENERATOR_OBJECT_CLASS: ObjClass = BASE_CLASS;

pub struct GeneratorFuncObject;

impl GeneratorFuncObject {
    pub fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        prepare_for_vm_call(rt, o, call);
        generator_call(rt, o, crate::vm::js_func_vm_call, call.arguments.len())
    }

    pub fn assert_callable(_rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        Some(CallFn {
            obj: o,
            f: GeneratorFuncObject::call,
        })
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        generator_vm_call(rt, o, crate::vm::js_func_vm_call, n)
    }

    pub fn assert_constructor(_rt: &Runtime, _o: ObjRef) -> Option<CtorFn> {
        None
    }
}

pub struct AsyncFuncObject;

impl AsyncFuncObject {
    pub fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        async_call(rt, o, call, crate::vm::js_func_vm_call)
    }

    pub fn assert_callable(_rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        Some(CallFn {
            obj: o,
            f: AsyncFuncObject::call,
        })
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        async_vm_call(rt, o, n, crate::vm::js_func_vm_call)
    }
}

pub struct AsyncArrowFuncObject;

impl AsyncArrowFuncObject {
    pub fn call(rt: &mut Runtime, o: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        async_call(rt, o, call, crate::vm::arrow_func_vm_call)
    }

    pub fn assert_callable(_rt: &Runtime, o: ObjRef) -> Option<CallFn> {
        Some(CallFn {
            obj: o,
            f: AsyncArrowFuncObject::call,
        })
    }

    pub fn vm_call(rt: &mut Runtime, o: ObjRef, n: usize) -> JsResult<()> {
        async_vm_call(rt, o, n, crate::vm::arrow_func_vm_call)
    }
}

pub fn prepare_for_vm_call(rt: &mut Runtime, f: ObjRef, call: FunctionCall<'_>) {
    let args = call.arguments;
    let sp = rt.vm.sp;
    rt.vm.stack_expand(sp + args.len() as isize + 1);
    rt.vm.stack[rt.vm.sp as usize] = call.this.clone();
    rt.vm.sp += 1;
    rt.vm.stack[rt.vm.sp as usize] = Value::Object(f);
    rt.vm.sp += 1;
    for arg in args {
        rt.vm.stack[rt.vm.sp as usize] = if arg.is_empty() {
            Value::Undefined
        } else {
            arg.clone()
        };
        rt.vm.sp += 1;
    }
}

impl Runtime {
    pub fn gen_enter(&mut self, g: &mut Generator) -> JsResult<()> {
        self.vm_push_ctx_for_call()?;
        self.vm.push_try_frame(TRY_PANIC_MARKER, -1);
        self.vm.prg = None;
        self.vm.sb = -1;
        self.vm.pc = -2;
        g.store_lengths(self);
        Ok(())
    }

    pub fn gen_step(&mut self, g: &mut Generator) -> JsResult<StepResult> {
        if let Some(ex) = self.vm_run_try_loop()? {
            return Ok((Value::Empty, ResultType::Normal, Some(ex)));
        }
        let mut res = self.vm.pop();
        let mut result_type = ResultType::Normal;
        if let Value::YieldMarker(ym) = res {
            result_type = ym.result_type();
            g.ctx = ExecCtx::default();
            self.vm.pc = -self.vm.pc + 1;
            if ym != YieldMarker::YieldEmpty {
                res = self.vm.pop();
            } else {
                res = Value::Empty;
            }
            let (t, i, r) = (g.try_stack_len, g.iter_stack_len, g.ref_stack_len);
            self.vm_suspend(&mut g.ctx, t, i, r);
            self.vm.sp = self.vm.sb - 1;
            self.vm.call_stack.pop();
        }
        Ok((res, result_type, None))
    }

    pub fn gen_enter_next(&mut self, g: &mut Generator) -> JsResult<()> {
        self.vm_push_ctx_for_call()?;
        self.vm.push_try_frame(TRY_PANIC_MARKER, -1);
        self.vm.call_stack.push(Context {
            pc: -2,
            ..Context::default()
        });
        g.store_lengths(self);
        self.vm_resume(&mut g.ctx);
        Ok(())
    }

    pub fn gen_next(&mut self, g: &mut Generator, v: Value) -> JsResult<StepResult> {
        self.gen_enter_next(g)?;
        if !v.is_empty() {
            self.vm.push(v);
        }
        let r = self.gen_step(g)?;
        self.vm.pop_try_frame();
        self.vm.pop_ctx();
        Ok(r)
    }

    pub fn gen_next_throw(&mut self, g: &mut Generator, v: Throw) -> JsResult<StepResult> {
        self.gen_enter_next(g)?;
        match self.vm_handle_throw(v) {
            Ok(()) => {}
            Err(Throw::Exception(ex)) => {
                self.vm.pop_try_frame();
                self.vm.pop_ctx();
                return Ok((Value::Empty, ResultType::Normal, Some(ex)));
            }
            Err(other) => return Err(other),
        }
        let r = self.gen_step(g)?;
        self.vm.pop_try_frame();
        self.vm.pop_ctx();
        Ok(r)
    }
}

fn gen_obj(rt: &Runtime, o: ObjRef) -> &GeneratorObject {
    match &rt.obj(o).ext {
        ObjExt::Generator(g) => g,
        _ => unreachable!("not a generator object"),
    }
}

fn gen_obj_mut(rt: &mut Runtime, o: ObjRef) -> &mut GeneratorObject {
    match &mut rt.obj_mut(o).ext {
        ObjExt::Generator(g) => g,
        _ => unreachable!("not a generator object"),
    }
}

fn take_gen(rt: &mut Runtime, o: ObjRef) -> Generator {
    std::mem::take(&mut gen_obj_mut(rt, o).gen_)
}

fn put_gen(rt: &mut Runtime, o: ObjRef, g: Generator) {
    gen_obj_mut(rt, o).gen_ = g;
}

fn set_gen_state(rt: &mut Runtime, o: ObjRef, state: GeneratorState) {
    gen_obj_mut(rt, o).state = state;
}

fn gen_state(rt: &Runtime, o: ObjRef) -> GeneratorState {
    gen_obj(rt, o).state
}

fn generator_object_gen_next(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<StepResult> {
    let mut g = take_gen(rt, o);
    let r = rt.gen_next(&mut g, v);
    put_gen(rt, o, g);
    r
}

fn generator_object_gen_next_throw(rt: &mut Runtime, o: ObjRef, v: Throw) -> JsResult<StepResult> {
    let mut g = take_gen(rt, o);
    let r = rt.gen_next_throw(&mut g, v);
    put_gen(rt, o, g);
    r
}

pub fn generator_object_init(
    rt: &mut Runtime,
    o: ObjRef,
    f: ObjRef,
    vm_call: VmCallFn,
    n_args: usize,
) -> JsResult<()> {
    let mut g = Generator::default();
    rt.gen_enter(&mut g)?;
    vm_call(rt, f, n_args)?;
    let r = rt.gen_step(&mut g);
    put_gen(rt, o, g);
    let (_, _, ex) = r?;
    rt.vm.pop_try_frame();
    if let Some(ex) = ex {
        return Err(Throw::Exception(ex));
    }
    set_gen_state(rt, o, GeneratorState::SuspendedStart);
    rt.vm.pop_ctx();
    Ok(())
}

pub fn generator_call(
    rt: &mut Runtime,
    f: ObjRef,
    vm_call: VmCallFn,
    n_args: usize,
) -> JsResult<Value> {
    let o = rt.alloc_object(
        &GENERATOR_OBJECT_CLASS,
        BaseObject::new(crate::object::CLASS_OBJECT, None),
        ObjExt::Generator(Box::new(GeneratorObject {
            gen_: Generator::default(),
            delegated: None,
            state: GeneratorState::Undefined,
        })),
    );
    rt.handles.push(o);
    generator_object_init(rt, o, f, vm_call, n_args)?;
    let gp = rt.get_generator_prototype();
    let proto = rt.get_prototype_from_ctor(f, None, Some(gp))?;
    rt.obj_mut(o).base.prototype = proto;
    Ok(Value::Object(o))
}

pub fn generator_vm_call(
    rt: &mut Runtime,
    f: ObjRef,
    vm_call: VmCallFn,
    n_args: usize,
) -> JsResult<()> {
    let v = generator_call(rt, f, vm_call, n_args)?;
    rt.vm.push(v);
    rt.vm.pc += 1;
    Ok(())
}

fn generator_object_validate(rt: &mut Runtime, o: ObjRef) -> JsResult<()> {
    if gen_state(rt, o) == GeneratorState::Executing {
        return Err(Throw::Value(rt.new_type_error("Illegal generator state")));
    }
    Ok(())
}

fn generator_object_step(rt: &mut Runtime, o: ObjRef, r: StepResult) -> JsResult<Value> {
    let (res, res_type, ex) = r;
    if let Some(ex) = ex {
        let g = gen_obj_mut(rt, o);
        g.delegated = None;
        g.state = GeneratorState::Completed;
        return Err(Throw::Exception(ex));
    }
    match res_type {
        ResultType::Yield => {
            set_gen_state(rt, o, GeneratorState::SuspendedYield);
            Ok(rt.create_iter_result_object(res, false))
        }
        ResultType::YieldDelegate => {
            set_gen_state(rt, o, GeneratorState::SuspendedYield);
            generator_object_delegate(rt, o, res)
        }
        ResultType::YieldRes => {
            set_gen_state(rt, o, GeneratorState::SuspendedYieldRes);
            Ok(rt.create_iter_result_object(res, false))
        }
        ResultType::YieldDelegateRes => {
            set_gen_state(rt, o, GeneratorState::SuspendedYieldRes);
            generator_object_delegate(rt, o, res)
        }
        ResultType::Normal => {
            set_gen_state(rt, o, GeneratorState::Completed);
            Ok(rt.create_iter_result_object(res, true))
        }
        ResultType::Await => Err(Throw::Value(rt.new_type_error(&format!(
            "Runtime bug: unexpected result type: {}",
            res_type as u8
        )))),
    }
}

fn generator_object_delegate(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    if let Value::Object(obj) = &v {
        rt.handles.push(*obj);
    }
    let mut iter = None;
    let ex = rt.vm_try_result(|rt| {
        iter = Some(rt.get_iterator(&v, None)?);
        Ok(())
    })?;
    if let Some(ex) = ex {
        let g = gen_obj_mut(rt, o);
        g.delegated = None;
        g.state = GeneratorState::Completed;
        let r = generator_object_gen_next_throw(rt, o, Throw::Exception(ex))?;
        return generator_object_step(rt, o, r);
    }
    gen_obj_mut(rt, o).delegated = iter;
    generator_object_next(rt, o, Value::Undefined)
}

fn generator_object_try_call_delegated(
    rt: &mut Runtime,
    o: ObjRef,
    f: impl FnOnce(&mut Runtime) -> JsResult<(Value, bool)>,
) -> JsResult<(Value, bool)> {
    let mut ret = (Value::Empty, false);
    let ex = rt.vm_try_result(|rt| {
        ret = f(rt)?;
        Ok(())
    })?;
    if let Some(ex) = ex {
        let g = gen_obj_mut(rt, o);
        g.delegated = None;
        g.state = GeneratorState::Executing;
        let r = generator_object_gen_next_throw(rt, o, Throw::Exception(ex))?;
        return Ok((generator_object_step(rt, o, r)?, false));
    }
    Ok(ret)
}

fn generator_object_call_delegated(
    rt: &mut Runtime,
    o: ObjRef,
    method: Option<CallFn>,
    v: Value,
) -> JsResult<(Value, bool)> {
    let this = gen_obj(rt, o)
        .delegated
        .and_then(|d| d.iterator)
        .map(Value::Object)
        .unwrap_or_default();
    let Some(method) = method else {
        return Err(Throw::Value(
            rt.new_type_error("iterator.next is missing or not a function"),
        ));
    };
    let r = method.call(rt, this, &[v])?;
    let res = rt.to_object(&r)?;
    rt.handles.push(res);
    if iterator_complete(rt, res)? {
        gen_obj_mut(rt, o).delegated = None;
        return Ok((iterator_value(rt, res)?, true));
    }
    Ok((Value::Object(res), false))
}

pub fn generator_object_next(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    generator_object_validate(rt, o)?;
    if gen_state(rt, o) == GeneratorState::Completed {
        return Ok(rt.create_iter_result_object(Value::Undefined, true));
    }
    let mut v = v;
    if let Some(d) = gen_obj(rt, o).delegated {
        let arg = v.clone();
        let (res, done) = generator_object_try_call_delegated(rt, o, |rt| {
            generator_object_call_delegated(rt, o, d.next, arg)
        })?;
        if !done {
            return Ok(res);
        }
        v = res;
    }
    if gen_state(rt, o) != GeneratorState::SuspendedYieldRes {
        v = Value::Empty;
    }
    set_gen_state(rt, o, GeneratorState::Executing);
    let r = generator_object_gen_next(rt, o, v)?;
    generator_object_step(rt, o, r)
}

pub fn generator_object_throw(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    generator_object_validate(rt, o)?;
    if gen_state(rt, o) == GeneratorState::SuspendedStart {
        set_gen_state(rt, o, GeneratorState::Completed);
    }
    if gen_state(rt, o) == GeneratorState::Completed {
        return Err(Throw::Value(v));
    }
    if let Some(d) = gen_obj(rt, o).delegated {
        let arg = v.clone();
        let (mut res, done) = generator_object_try_call_delegated(rt, o, |rt| {
            let it = gen_obj(rt, o).delegated.and_then(|d| d.iterator);
            let m = match it {
                Some(it) => it.get_str(rt, &crate::vm::STR_THROW, None)?,
                None => None,
            };
            let method = to_method(rt, m)?;
            if method.is_some() {
                return generator_object_call_delegated(rt, o, method, arg);
            }
            gen_obj_mut(rt, o).delegated = None;
            let mut d = d;
            d.return_iter(rt)?;
            Err(Throw::Value(rt.new_type_error(
                "The iterator does not provide a 'throw' method",
            )))
        })?;
        if !done {
            return Ok(res);
        }
        if gen_state(rt, o) != GeneratorState::SuspendedYieldRes {
            res = Value::Empty;
        }
        set_gen_state(rt, o, GeneratorState::Executing);
        let r = generator_object_gen_next(rt, o, res)?;
        return generator_object_step(rt, o, r);
    }
    set_gen_state(rt, o, GeneratorState::Executing);
    let r = generator_object_gen_next_throw(rt, o, Throw::Value(v))?;
    generator_object_step(rt, o, r)
}

pub fn generator_object_return(rt: &mut Runtime, o: ObjRef, v: Value) -> JsResult<Value> {
    generator_object_validate(rt, o)?;
    if gen_state(rt, o) == GeneratorState::SuspendedStart {
        set_gen_state(rt, o, GeneratorState::Completed);
    }
    if gen_state(rt, o) == GeneratorState::Completed {
        return Ok(rt.create_iter_result_object(v, true));
    }
    let mut v = v;
    if gen_obj(rt, o).delegated.is_some() {
        let arg = v.clone();
        let (res, done) = generator_object_try_call_delegated(rt, o, |rt| {
            let it = gen_obj(rt, o).delegated.and_then(|d| d.iterator);
            let m = match it {
                Some(it) => it.get_str(rt, &crate::vm::STR_RETURN, None)?,
                None => None,
            };
            let method = to_method(rt, m)?;
            if method.is_some() {
                return generator_object_call_delegated(rt, o, method, arg);
            }
            gen_obj_mut(rt, o).delegated = None;
            Ok((arg, true))
        })?;
        if !done {
            return Ok(res);
        }
        v = res;
    }
    if let Value::Object(obj) = &v {
        rt.handles.push(*obj);
    }

    set_gen_state(rt, o, GeneratorState::Executing);

    let mut g = take_gen(rt, o);
    let r = generator_object_return_inner(rt, o, &mut g);
    put_gen(rt, o, g);
    r?;
    Ok(rt.create_iter_result_object(v, true))
}

fn generator_object_return_inner(rt: &mut Runtime, o: ObjRef, g: &mut Generator) -> JsResult<()> {
    rt.gen_enter_next(g)?;
    let mut ex = None;
    loop {
        let Some(tf) = rt.vm.try_stack.last() else {
            break;
        };
        if tf.call_stack_len as usize != rt.vm.call_stack.len() {
            break;
        }
        if tf.finally_pos >= 0 {
            let (sp, stash, priv_env, iter_len, ref_len, finally_pos) = (
                tf.sp,
                tf.stash,
                tf.priv_env.clone(),
                tf.iter_len,
                tf.ref_len,
                tf.finally_pos,
            );
            rt.vm.sp = sp as isize;
            rt.vm.stash = stash;
            rt.vm.priv_env = priv_env;
            if let Some(e) = rt.vm_restore_stacks(iter_len, ref_len)? {
                ex = Some(e);
                rt.vm.pop_try_frame();
                continue;
            }
            rt.vm.pc = finally_pos as isize;
            if let Some(tf) = rt.vm.try_stack.last_mut() {
                tf.catch_pos = TRY_PANIC_MARKER;
                tf.finally_pos = -1;
                tf.finally_ret = -2;
            }
            loop {
                if let Some(e) = rt.vm_run_try_inner()? {
                    ex = Some(e);
                    rt.vm.pop_try_frame();
                    break;
                }
                if rt.vm.halted() {
                    break;
                }
            }
        } else {
            rt.vm.pop_try_frame();
        }
    }

    set_gen_state(rt, o, GeneratorState::Completed);

    rt.vm.pop_try_frame();

    if ex.is_none() {
        ex = rt.vm_restore_stacks(g.iter_stack_len, g.ref_stack_len)?;
    }

    if let Some(ex) = ex {
        return Err(Throw::Exception(ex));
    }

    rt.vm.call_stack.pop();
    rt.vm.sp = rt.vm.sb - 1;
    rt.vm.pop_ctx();
    Ok(())
}

pub struct AsyncRunner {
    pub gen_: Generator,
    pub promise_cap: Option<PromiseCapability>,
    pub f: ObjRef,
    pub vm_call: VmCallFn,
}

impl ObjData for AsyncRunner {
    fn trace(&self, t: &mut Tracer) {
        self.gen_.trace(t);
        if let Some(p) = &self.promise_cap {
            p.trace(t);
        }
        t.obj(self.f);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn async_runner(rt: &Runtime, ar: ObjRef) -> &AsyncRunner {
    rt.ext_dyn::<AsyncRunner>(ar).expect("async runner")
}

fn async_runner_mut(rt: &mut Runtime, ar: ObjRef) -> &mut AsyncRunner {
    rt.ext_dyn_mut::<AsyncRunner>(ar).expect("async runner")
}

fn take_async_gen(rt: &mut Runtime, ar: ObjRef) -> Generator {
    std::mem::take(&mut async_runner_mut(rt, ar).gen_)
}

fn put_async_gen(rt: &mut Runtime, ar: ObjRef, g: Generator) {
    async_runner_mut(rt, ar).gen_ = g;
}

pub fn new_async_runner(rt: &mut Runtime, f: ObjRef, vm_call: VmCallFn) -> ObjRef {
    let ar = rt.alloc_object(
        &crate::object::BASE_OBJECT_CLASS,
        BaseObject::new(crate::object::CLASS_OBJECT, None),
        ObjExt::Dyn(Box::new(AsyncRunner {
            gen_: Generator::default(),
            promise_cap: None,
            f,
            vm_call,
        })),
    );
    rt.handles.push(ar);
    ar
}

pub fn async_runner_on_fulfilled(
    rt: &mut Runtime,
    ar: ObjRef,
    call: FunctionCall<'_>,
) -> JsResult<Value> {
    rt.vm.cur_async_runner = Some(ar);
    let arg = call.argument(0);
    let mut g = take_async_gen(rt, ar);
    let r = rt.gen_next(&mut g, arg);
    put_async_gen(rt, ar, g);
    let r = match r {
        Ok((res, res_type, ex)) => {
            async_runner_step(rt, ar, res, res_type == ResultType::Normal, ex)
        }
        Err(e) => Err(e),
    };
    rt.vm.cur_async_runner = None;
    r?;
    Ok(Value::Undefined)
}

pub fn async_runner_on_rejected(
    rt: &mut Runtime,
    ar: ObjRef,
    call: FunctionCall<'_>,
) -> JsResult<Value> {
    rt.vm.cur_async_runner = Some(ar);
    let reason = call.argument(0);
    let mut g = take_async_gen(rt, ar);
    let r = rt.gen_next_throw(&mut g, Throw::Value(reason));
    put_async_gen(rt, ar, g);
    let r = match r {
        Ok((res, res_type, ex)) => {
            async_runner_step(rt, ar, res, res_type == ResultType::Normal, ex)
        }
        Err(e) => Err(e),
    };
    rt.vm.cur_async_runner = None;
    r?;
    Ok(Value::Undefined)
}

pub fn async_runner_step(
    rt: &mut Runtime,
    ar: ObjRef,
    res: Value,
    done: bool,
    ex: Option<Box<Exception>>,
) -> JsResult<()> {
    if done || ex.is_some() {
        let cap = async_runner(rt, ar).promise_cap;
        return match (&cap, ex) {
            (Some(cap), None) => cap.resolve(rt, res),
            (Some(cap), Some(ex)) => cap.reject(rt, ex.val),
            (None, _) => Ok(()),
        };
    }
    let p = rt.get_promise();
    let promise = rt.promise_resolve(p, res)?;
    rt.promise_add_async_runner_reactions(promise, ar)
}

pub fn async_runner_start(rt: &mut Runtime, ar: ObjRef, n_args: usize) -> JsResult<()> {
    let p = rt.get_promise();
    let cap = rt.new_promise_capability(p)?;
    async_runner_mut(rt, ar).promise_cap = Some(cap);
    let sp = rt.vm.sp;
    let mut g = Generator::default();
    let entered = rt.gen_enter(&mut g);
    put_async_gen(rt, ar, g);
    entered?;
    let (f, vm_call) = {
        let a = async_runner(rt, ar);
        (a.f, a.vm_call)
    };
    vm_call(rt, f, n_args)?;
    let mut g = take_async_gen(rt, ar);
    let r = rt.gen_step(&mut g);
    put_async_gen(rt, ar, g);
    let (res, res_type, ex) = r?;
    let failed = ex.is_some();
    async_runner_step(rt, ar, res, res_type == ResultType::Normal, ex)?;
    if failed {
        rt.vm.sp = sp - n_args as isize - 2;
    }
    rt.vm.pop_try_frame();
    rt.vm.pop_ctx();
    Ok(())
}

pub fn async_runner_promise(rt: &Runtime, ar: ObjRef) -> Value {
    async_runner(rt, ar)
        .promise_cap
        .as_ref()
        .map(|c| Value::Object(c.promise))
        .unwrap_or_default()
}

pub fn async_call(
    rt: &mut Runtime,
    f: ObjRef,
    call: FunctionCall<'_>,
    vm_call: VmCallFn,
) -> JsResult<Value> {
    prepare_for_vm_call(rt, f, call);
    let ar = new_async_runner(rt, f, vm_call);
    async_runner_start(rt, ar, call.arguments.len())?;
    Ok(async_runner_promise(rt, ar))
}

pub fn async_vm_call(rt: &mut Runtime, f: ObjRef, n: usize, vm_call: VmCallFn) -> JsResult<()> {
    let ar = new_async_runner(rt, f, vm_call);
    async_runner_start(rt, ar, n)?;
    let p = async_runner_promise(rt, ar);
    rt.vm.push(p);
    rt.vm.pc += 1;
    Ok(())
}

fn async_runner_data(d: &[Value]) -> ObjRef {
    match &d[0] {
        Value::Object(ar) => *ar,
        _ => unreachable!("async runner"),
    }
}

impl Runtime {
    pub fn promise_add_async_runner_reactions(
        &mut self,
        promise: ObjRef,
        ar: ObjRef,
    ) -> JsResult<()> {
        let on_fulfilled = NativeFn::closure_with(
            vec![Value::Object(ar)],
            |rt: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                async_runner_on_fulfilled(rt, async_runner_data(d), call)
            },
        );
        let on_rejected = NativeFn::closure_with(
            vec![Value::Object(ar)],
            |rt: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                async_runner_on_rejected(rt, async_runner_data(d), call)
            },
        );
        Promise::add_reactions(
            self,
            promise,
            PromiseReaction {
                capability: None,
                typ: PromiseReactionType::Fulfill,
                handler: Some(JobCallback::Native(on_fulfilled)),
                async_runner: Some(ar),
            },
            PromiseReaction {
                capability: None,
                typ: PromiseReactionType::Reject,
                handler: Some(JobCallback::Native(on_rejected)),
                async_runner: Some(ar),
            },
        );
        Ok(())
    }
}

pub fn func_source(rt: &mut Runtime, o: ObjRef) -> Option<JsResult<JsString>> {
    match &rt.obj(o).ext {
        ObjExt::JsFunc(_) => Some(BaseJsFuncObject::source(rt, o)),
        ObjExt::NativeFunc(_) => Some(NativeFuncObject::source(rt, o)),
        ObjExt::Template(t) if t.is_func() => {
            Some(crate::object_template::TemplatedFuncObject::source(rt, o))
        }
        _ => None,
    }
}
