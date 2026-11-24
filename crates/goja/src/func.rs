use crate::compiler::Program;
use crate::heap::{ObjRef, StashRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CallFn, ConstructorCall, CtorFn, FunctionCall, ObjClass, ObjExt,
    PrivateEnv, PrivateEnvType, UStr, instance_of_operator,
};
use crate::runtime::{IteratorRecord, JsResult, Runtime, Throw};
use crate::string::{JsString, STRING_FUNCTION, new_string_value, string_value_from_raw};
use crate::unistring;
use crate::value::{Value, ValueProperty, nil_safe};
use crate::vm::ExecCtx;
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
            rt.vm.push_ctx()?;
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
        ._put(&"length".into(), Value::Property(Box::new(len_prop)));
    o._put_prop(
        rt,
        &"name".into(),
        Value::String(string_value_from_raw(name)),
        false,
        false,
        true,
    );
}

pub fn has_instance(rt: &mut Runtime, val: ObjRef, v: &Value) -> JsResult<bool> {
    if let Value::Object(v) = v {
        let o = val.get_str(rt, &"prototype".into(), None)?;
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
            &"constructor".into(),
            Value::Object(o),
            true,
            false,
            true,
        );
        o._put_prop(
            rt,
            &"prototype".into(),
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
            accum.push(Value::String(crate::string::ascii_string("prototype")));
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

pub struct GeneratorObject {
    pub gen_ctx: ExecCtx,
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
        self.gen_ctx.trace(t);
        if let Some(d) = &self.delegated {
            d.trace(t);
        }
    }
}

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

pub static GENERATOR_FUNC_CLASS: ObjClass = BASE_JS_FUNC_CLASS;
pub static GENERATOR_METHOD_FUNC_CLASS: ObjClass = BASE_JS_FUNC_CLASS;
pub static ASYNC_FUNC_CLASS: ObjClass = BASE_JS_FUNC_CLASS;
pub static ASYNC_METHOD_FUNC_CLASS: ObjClass = BASE_JS_FUNC_CLASS;
pub static ASYNC_ARROW_FUNC_CLASS: ObjClass = ARROW_FUNC_CLASS_CONST;

const ARROW_FUNC_CLASS_CONST: ObjClass = ObjClass {
    assert_callable: ArrowFuncObject::assert_callable,
    vm_call: ArrowFuncObject::vm_call,
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
