use crate::builtin_symbol::{SYM_HAS_INSTANCE, SYM_TO_STRING_TAG};
use crate::func::{BOUND_FUNC_CLASS, NativeCtor, NativeFn, func_source};
use crate::heap::ObjRef;
use crate::object::{
    BaseObject, CLASS_ASYNC_FUNCTION, CLASS_GENERATOR, CLASS_GENERATOR_FUNCTION, CLASS_OBJECT,
    FunctionCall, ObjExt,
};
use crate::object_template::ObjectTemplate;
use crate::runtime::{JsResult, Runtime, Throw, to_length};
use crate::string::{STRING_BOUND_, STRING_EMPTY, StringBuilder, ascii_string};
use crate::value::{Value, ValueProperty, int_to_value, nil_safe, value_prop};
use std::sync::OnceLock;

impl Runtime {
    pub fn function_ctor(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
        is_async: bool,
        generator: bool,
    ) -> JsResult<ObjRef> {
        let mut sb = StringBuilder::new();
        let head = match (is_async, generator) {
            (true, true) => "(async function* anonymous(",
            (true, false) => "(async function anonymous(",
            (false, true) => "(function* anonymous(",
            (false, false) => "(function anonymous(",
        };
        sb.write_string(&ascii_string(head));
        if args.len() > 1 {
            let ar = &args[..args.len() - 1];
            for (i, arg) in ar.iter().enumerate() {
                let s = arg.to_string(self)?;
                sb.write_string(&s);
                if i < ar.len() - 1 {
                    sb.write_rune(',' as i32);
                }
            }
        }
        sb.write_string(&ascii_string("\n) {\n"));
        if let Some(last) = args.last() {
            let s = last.to_string(self)?;
            sb.write_string(&s);
        }
        sb.write_string(&ascii_string("\n})"));
        let v = self.eval(&sb.string(), false, false)?;
        let ret = self.to_object(&v)?;
        ret.set_proto(self, proto, true)?;
        Ok(ret)
    }

    pub fn builtin_function(&mut self, args: &[Value], proto: Option<ObjRef>) -> JsResult<ObjRef> {
        self.function_ctor(args, proto, false, false)
    }

    pub fn builtin_async_function(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        self.function_ctor(args, proto, true, false)
    }

    pub fn builtin_generator_function(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        self.function_ctor(args, proto, false, true)
    }

    pub fn functionproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = self.to_object(call.this)?;
        if let Some(s) = func_source(self, obj) {
            return Ok(Value::String(s?));
        }
        if let Some(Some(t)) = crate::builtin_proxy::proxy_target(self, obj)
            && func_source_kind(self, t)
        {
            return Ok(Value::String(ascii_string("function () { [native code] }")));
        }
        Err(Throw::Value(self.new_type_error(
            "Function.prototype.toString requires that 'this' be a Function",
        )))
    }

    pub fn functionproto_has_instance(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(o) = call.this
            && o.assert_callable(self).is_some()
        {
            let r = o.has_instance(self, &call.argument(0))?;
            return Ok(Value::Bool(r));
        }
        Ok(Value::Bool(false))
    }

    pub fn create_list_from_array_like(&mut self, a: &Value) -> JsResult<Vec<Value>> {
        let o = self.to_object(a)?;
        if let Some(values) = crate::array::check_std_array_obj_values(self, o) {
            return Ok(values);
        }
        let lv = nil_safe(o.get_str(self, &"length".into(), None)?);
        let l = to_length(self, &lv)?;
        let mut res = Vec::with_capacity(l.max(0) as usize);
        for k in 0..l {
            let v = nil_safe(o.get_idx(self, k, None)?);
            self.root(&v);
            res.push(v);
        }
        Ok(res)
    }

    pub fn functionproto_apply(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut args = Vec::new();
        let arg_array = call.argument(1);
        if !arg_array.is_undefined() && !arg_array.is_null() {
            args = self.create_list_from_array_like(&arg_array)?;
        }
        let f = self.to_callable(call.this)?;
        f.call(self, call.argument(0), &args)
    }

    pub fn functionproto_call(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let args: &[Value] = if !call.arguments.is_empty() {
            &call.arguments[1..]
        } else {
            &[]
        };
        let f = self.to_callable(call.this)?;
        f.call(self, call.argument(0), args)
    }

    pub fn bound_callable(
        &mut self,
        target: crate::object::CallFn,
        bound_args: &[Value],
    ) -> NativeFn {
        let mut data = vec![Value::Object(target.obj)];
        if !bound_args.is_empty() {
            data.extend_from_slice(bound_args);
        } else {
            data.push(Value::Undefined);
        }
        let tf = target.f;
        NativeFn::closure_with(data, move |r, d, call| {
            let target = crate::object::CallFn {
                obj: d[0].as_object().unwrap_or(crate::heap::ObjRef(0)),
                f: tf,
            };
            let this = d[1].clone();
            let mut a: Vec<Value> = d[2..].to_vec();
            a.extend_from_slice(call.arguments);
            target.call(r, this, &a)
        })
    }

    pub fn bound_construct(
        &mut self,
        f: ObjRef,
        target: Option<crate::object::CtorFn>,
        bound_args: &[Value],
    ) -> Option<NativeCtor> {
        let target = target?;
        let mut data = vec![Value::Object(f), Value::Object(target.obj)];
        if bound_args.len() > 1 {
            data.extend_from_slice(&bound_args[1..]);
        }
        let tf = target.f;
        Some(NativeCtor::closure_with(
            data,
            move |r, d, fargs, new_target| {
                let f = d[0].as_object();
                let target = crate::object::CtorFn {
                    obj: d[1].as_object().unwrap_or(crate::heap::ObjRef(0)),
                    f: tf,
                };
                let mut a: Vec<Value> = d[2..].to_vec();
                a.extend_from_slice(fargs);
                let nt = if new_target == f { None } else { new_target };
                target.construct(r, &a, nt)
            },
        ))
    }

    pub fn functionproto_bind(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = self.to_object(call.this)?;
        let fcall = self.to_callable(call.this)?;
        let construct = obj.assert_constructor(self);
        let mut l = Value::Int(0);
        'len: {
            if obj.has_own_property_str(self, &"length".into())? {
                let mut li: i64 = 0;
                let len_prop = nil_safe(obj.get_str(self, &"length".into(), None)?);
                match len_prop {
                    Value::Int(i) => li = i,
                    Value::Float(f) => {
                        if f == f64::INFINITY {
                            l = Value::Float(f);
                            break 'len;
                        } else if f == f64::NEG_INFINITY {
                            break 'len;
                        } else if f == 0.0 && f.is_sign_negative() {
                        } else if !f.is_nan() {
                            li = crate::value::go_float_to_int(f.abs());
                        }
                    }
                    _ => {}
                }
                if call.arguments.len() > 1 {
                    li -= call.arguments.len() as i64 - 1;
                }
                if li < 0 {
                    li = 0;
                }
                l = int_to_value(li);
            }
        }
        let name = obj.get_str(self, &"name".into(), None)?;
        let mut name_str = STRING_BOUND_.clone();
        if let Some(Value::String(s)) = &name {
            name_str = name_str.concat(s);
        }
        let v = self.reserve_object();
        let bc = self.bound_callable(fcall, call.arguments);
        let bcon = self.bound_construct(v, construct, call.arguments);
        self.new_native_func_and_construct(
            Some(v),
            Some(bc),
            bcon,
            None,
            &name_str.to_rust_string(),
            l,
        );
        let fixed_name = name_str.string();
        if fixed_name.to_string() != name_str.to_rust_string() {
            v._put_prop(
                self,
                &"name".into(),
                Value::String(name_str.clone()),
                false,
                false,
                true,
            );
        }
        let proto = obj.proto(self)?;
        let o = self.obj_mut(v);
        o.class = &BOUND_FUNC_CLASS;
        o.base.prototype = proto;
        if let ObjExt::NativeFunc(n) = &mut o.ext {
            n.wrapped = Some(obj);
        }
        Ok(Value::Object(v))
    }

    pub fn get_thrower(&mut self) -> ObjRef {
        if let Some(r) = self.global.thrower {
            return r;
        }
        let ret = self.new_native_func(NativeFn::Fn(|r, c| r.builtin_thrower(c)), "", 0);
        self.global.thrower = Some(ret);
        let _ = self.object_freeze(FunctionCall::new(&Value::Undefined, &[Value::Object(ret)]));
        ret
    }

    pub fn new_thrower_property(&mut self, configurable: bool) -> Value {
        let thrower = self.get_thrower();
        Value::Property(Box::new(ValueProperty {
            getter_func: Some(thrower),
            setter_func: Some(thrower),
            accessor: true,
            configurable,
            ..ValueProperty::default()
        }))
    }

    pub fn get_function_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.function_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.function_prototype = Some(ret);
        self.new_templated_func_object(
            get_function_proto_template(),
            Some(ret),
            Some(NativeFn::Fn(|_, _| Ok(Value::Undefined))),
            None,
        );
        ret
    }

    pub fn get_function(&mut self) -> ObjRef {
        if let Some(r) = self.global.function {
            return r;
        }
        let ret = self.reserve_object();
        self.global.function = Some(ret);
        let fp = self.get_function_prototype();
        self.new_native_func_construct_obj(
            ret,
            |r, a, p| r.builtin_function(a, p),
            "Function",
            Some(fp),
            1,
        );
        ret
    }

    pub fn get_async_function_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.async_function_prototype {
            return r;
        }
        let o = self.reserve_object();
        self.global.async_function_prototype = Some(o);
        let fp = self.get_function_prototype();
        self.new_base_object_obj(o, Some(fp), CLASS_OBJECT);
        let af = self.get_async_function();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(af),
            true,
            false,
            true,
        );
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_ASYNC_FUNCTION)),
                false,
                false,
                true,
            ),
        );
        o
    }

    pub fn get_async_function(&mut self) -> ObjRef {
        if let Some(r) = self.global.async_function {
            return r;
        }
        let o = self.reserve_object();
        self.global.async_function = Some(o);
        let p = self.get_async_function_prototype();
        self.new_native_func_construct_obj(
            o,
            |r, a, p| r.builtin_async_function(a, p),
            "AsyncFunction",
            Some(p),
            1,
        );
        o
    }

    pub fn builtin_genproto_next(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(o) = call.this
            && let ObjExt::Generator(_) = &self.obj(*o).ext
        {
            return crate::vm::generator_next(self, *o, call.argument(0));
        }
        Err(Throw::Value(self.new_type_error(
            "Method [Generator].prototype.next called on incompatible receiver",
        )))
    }

    pub fn builtin_genproto_return(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(o) = call.this
            && let ObjExt::Generator(_) = &self.obj(*o).ext
        {
            return crate::vm::generator_return(self, *o, call.argument(0));
        }
        Err(Throw::Value(self.new_type_error(
            "Method [Generator].prototype.return called on incompatible receiver",
        )))
    }

    pub fn builtin_genproto_throw(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(o) = call.this
            && let ObjExt::Generator(_) = &self.obj(*o).ext
        {
            return crate::vm::generator_throw(self, *o, call.argument(0));
        }
        Err(Throw::Value(self.new_type_error(
            "Method [Generator].prototype.throw called on incompatible receiver",
        )))
    }

    pub fn get_generator_function_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.generator_function_prototype {
            return r;
        }
        let o = self.reserve_object();
        self.global.generator_function_prototype = Some(o);
        let fp = self.get_function_prototype();
        self.new_base_object_obj(o, Some(fp), CLASS_OBJECT);
        let gf = self.get_generator_function();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(gf),
            false,
            false,
            true,
        );
        let gp = self.get_generator_prototype();
        o._put_prop(
            self,
            &"prototype".into(),
            Value::Object(gp),
            false,
            false,
            true,
        );
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_GENERATOR_FUNCTION)),
                false,
                false,
                true,
            ),
        );
        o
    }

    pub fn get_generator_function(&mut self) -> ObjRef {
        if let Some(r) = self.global.generator_function {
            return r;
        }
        let o = self.reserve_object();
        self.global.generator_function = Some(o);
        let p = self.get_generator_function_prototype();
        self.new_native_func_construct_obj(
            o,
            |r, a, p| r.builtin_generator_function(a, p),
            "GeneratorFunction",
            Some(p),
            1,
        );
        o
    }

    pub fn get_generator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.generator_prototype {
            return r;
        }
        let o = self.reserve_object();
        self.global.generator_prototype = Some(o);
        let ip = self.get_iterator_prototype();
        self.new_base_object_obj(o, Some(ip), CLASS_OBJECT);
        let gfp = self.get_generator_function_prototype();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(gfp),
            false,
            false,
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.builtin_genproto_next(c)), "next", 1);
        o._put_prop(self, &"next".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.builtin_genproto_return(c)),
            "return",
            1,
        );
        o._put_prop(self, &"return".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.builtin_genproto_throw(c)), "throw", 1);
        o._put_prop(self, &"throw".into(), Value::Object(f), true, false, true);
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_GENERATOR)),
                false,
                false,
                true,
            ),
        );
        o
    }

    pub fn get_iterator_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.iterator_prototype {
            return r;
        }
        let o = self.reserve_object();
        self.global.iterator_prototype = Some(o);
        let op = self.global.object_prototype;
        self.new_base_object_obj(o, op, CLASS_OBJECT);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.return_this(c)),
            "[Symbol.iterator]",
            0,
        );
        o._put_sym(
            self,
            crate::builtin_symbol::SYM_ITERATOR,
            value_prop(Value::Object(f), true, false, true),
        );
        o
    }
}

fn func_source_kind(rt: &Runtime, o: ObjRef) -> bool {
    match &rt.obj(o).ext {
        ObjExt::JsFunc(_) | ObjExt::NativeFunc(_) => true,
        ObjExt::Template(t) => t.is_func(),
        _ => false,
    }
}

fn create_function_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);
    t.put_str("constructor", |r| {
        let f = r.get_function();
        value_prop(Value::Object(f), true, false, true)
    });
    t.put_str("length", |_| value_prop(Value::Int(0), false, false, true));
    t.put_str("name", |_| {
        value_prop(Value::String(STRING_EMPTY.clone()), false, false, true)
    });
    t.put_str("apply", |r| {
        r.method_prop(|r, c| r.functionproto_apply(c), "apply", 2)
    });
    t.put_str("bind", |r| {
        r.method_prop(|r, c| r.functionproto_bind(c), "bind", 1)
    });
    t.put_str("call", |r| {
        r.method_prop(|r, c| r.functionproto_call(c), "call", 1)
    });
    t.put_str("toString", |r| {
        r.method_prop(|r, c| r.functionproto_to_string(c), "toString", 0)
    });
    t.put_str("caller", |r| r.new_thrower_property(true));
    t.put_str("arguments", |r| r.new_thrower_property(true));
    t.put_sym(SYM_HAS_INSTANCE, |r| {
        let f = r.new_native_func(
            NativeFn::Fn(|r, c| r.functionproto_has_instance(c)),
            "[Symbol.hasInstance]",
            1,
        );
        value_prop(Value::Object(f), false, false, false)
    });
    t
}

pub fn get_function_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_function_proto_template)
}

pub fn base_object_marker(_b: &BaseObject) {}
