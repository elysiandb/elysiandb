use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ERROR, CLASS_OBJECT, Flag, FunctionCall, IterNextFunc, ObjClass,
    ObjExt, PropertyDescriptor, UStr,
};
use crate::runtime::{JsResult, Runtime, StackFrame};
use crate::string::{
    JsString, STRING_AGGREGATE_ERROR, STRING_EMPTY, STRING_ERROR, STRING_EVAL_ERROR,
    STRING_RANGE_ERROR, STRING_REFERENCE_ERROR, STRING_SYNTAX_ERROR, STRING_TYPE_ERROR,
    STRING_URI_ERROR, StringBuilder, ascii_string,
};
use crate::value::Value;

pub const PROP_NAME_STACK: &str = "stack";

pub struct ErrorObject {
    pub stack: Vec<StackFrame>,
    pub stack_prop_added: bool,
}

impl ErrorObject {
    pub fn trace(&self, _t: &mut Tracer) {}

    fn data(rt: &Runtime, o: ObjRef) -> &ErrorObject {
        match &rt.obj(o).ext {
            ObjExt::Error(e) => e,
            _ => unreachable!("not an error object"),
        }
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut ErrorObject {
        match &mut rt.obj_mut(o).ext {
            ObjExt::Error(e) => e,
            _ => unreachable!("not an error object"),
        }
    }

    pub fn format_stack(rt: &mut Runtime, o: ObjRef) -> JsResult<JsString> {
        let mut b = StringBuilder::new();
        if let Some(val) = write_error_string(rt, &mut b, o)? {
            b.write_string(&val);
        }
        b.write_rune('\n' as i32);
        let stack = ErrorObject::data(rt, o).stack.clone();
        for frame in &stack {
            b.write_ascii(b"\tat ");
            frame.write_to_value_builder(&mut b);
            b.write_rune('\n' as i32);
        }
        Ok(b.string())
    }

    pub fn add_stack_prop(rt: &mut Runtime, o: ObjRef) -> JsResult<Option<Value>> {
        if !ErrorObject::data(rt, o).stack_prop_added {
            let s = ErrorObject::format_stack(rt, o)?;
            let res = BaseObject::_put_prop(
                rt,
                o,
                &PROP_NAME_STACK.into(),
                Value::String(s),
                true,
                false,
                true,
            );
            let names = &mut rt.obj_mut(o).base.prop_names;
            if names.len() > 1 {
                let last = names.pop();
                if let Some(last) = last {
                    names.insert(0, last);
                }
            }
            ErrorObject::data_mut(rt, o).stack_prop_added = true;
            return Ok(Some(res));
        }
        Ok(None)
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let prop = ErrorObject::get_own_prop_str(rt, o, p)?;
        BaseObject::get_str_with_own_prop(rt, o, prop, p, receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let res = BaseObject::get_own_prop_str(rt, o, name)?;
        if res.is_none() && name == PROP_NAME_STACK {
            return ErrorObject::add_stack_prop(rt, o);
        }
        Ok(res)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if name == PROP_NAME_STACK {
            ErrorObject::add_stack_prop(rt, o)?;
        }
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
        let p = ErrorObject::get_own_prop_str(rt, o, name)?;
        BaseObject::_set_foreign_str(rt, o, name, p.as_ref(), val, receiver, throw)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        if name == PROP_NAME_STACK {
            ErrorObject::add_stack_prop(rt, o)?;
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if name == PROP_NAME_STACK {
            ErrorObject::add_stack_prop(rt, o)?;
        }
        BaseObject::define_own_property_str(rt, o, name, desc, throw)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        if BaseObject::has_own_property_str(rt, o, name)? {
            return Ok(true);
        }
        Ok(name == PROP_NAME_STACK && !ErrorObject::data(rt, o).stack_prop_added)
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        if all && !ErrorObject::data(rt, o).stack_prop_added {
            accum.push(Value::String(ascii_string(PROP_NAME_STACK)));
        }
        BaseObject::string_keys(rt, o, all, accum)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        ErrorObject::add_stack_prop(rt, o)?;
        BaseObject::iterate_string_keys(rt, o)
    }
}

pub static ERROR_OBJECT_CLASS: ObjClass = ObjClass {
    get_str: ErrorObject::get_str,
    get_own_prop_str: ErrorObject::get_own_prop_str,
    set_own_str: ErrorObject::set_own_str,
    set_foreign_str: ErrorObject::set_foreign_str,
    delete_str: ErrorObject::delete_str,
    define_own_property_str: ErrorObject::define_own_property_str,
    has_own_property_str: ErrorObject::has_own_property_str,
    string_keys: ErrorObject::string_keys,
    iterate_string_keys: ErrorObject::iterate_string_keys,
    ..BASE_CLASS
};

pub fn write_error_string(
    rt: &mut Runtime,
    sb: &mut StringBuilder,
    obj: ObjRef,
) -> JsResult<Option<JsString>> {
    let name = obj.get_str(rt, &"name".into(), None)?;
    let name_str = match name {
        None | Some(Value::Undefined) => ascii_string("Error"),
        Some(n) => n.to_string(rt)?,
    };
    let msg = obj.get_str(rt, &"message".into(), None)?;
    let msg_str = match msg {
        None | Some(Value::Undefined) => STRING_EMPTY.clone(),
        Some(m) => m.to_string(rt)?,
    };
    if name_str.length() == 0 {
        return Ok(Some(msg_str));
    }
    if msg_str.length() == 0 {
        return Ok(Some(name_str));
    }
    sb.write_string(&name_str);
    sb.write_string(&ascii_string(": "));
    sb.write_string(&msg_str);
    Ok(None)
}

impl Runtime {
    pub fn new_error_object(&mut self, proto: Option<ObjRef>, class: &'static str) -> ObjRef {
        let stack = self.vm_capture_stack(0);
        self.alloc_object(
            &ERROR_OBJECT_CLASS,
            BaseObject::new(class, proto),
            ObjExt::Error(Box::new(ErrorObject {
                stack,
                stack_prop_added: false,
            })),
        )
    }

    fn error_cause(&mut self, obj: ObjRef, options: &Value) -> JsResult<()> {
        if let Value::Object(options) = options {
            let options = *options;
            if options.has_property(self, &Value::String(ascii_string("cause")))? {
                let cause = options.get_str(self, &"cause".into(), None)?;
                obj.define_own_property_str(
                    self,
                    &"cause".into(),
                    PropertyDescriptor {
                        writable: Flag::True,
                        enumerable: Flag::False,
                        configurable: Flag::True,
                        value: Some(cause.unwrap_or(Value::Empty)),
                        ..PropertyDescriptor::default()
                    },
                    true,
                )?;
            }
        }
        Ok(())
    }

    pub fn builtin_error(&mut self, args: &[Value], proto: Option<ObjRef>) -> JsResult<ObjRef> {
        let obj = self.new_error_object(proto, CLASS_ERROR);
        if let Some(a0) = args.first()
            && !a0.is_undefined()
        {
            let m = a0.to_string_value(self)?;
            BaseObject::_put_prop(self, obj, &"message".into(), m, true, false, true);
        }
        if let Some(a1) = args.get(1)
            && !a1.is_undefined()
        {
            self.error_cause(obj, a1)?;
        }
        Ok(obj)
    }

    pub fn builtin_aggregate_error(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let obj = self.new_error_object(proto, CLASS_ERROR);
        if let Some(a1) = args.get(1)
            && !a1.is_empty()
            && !a1.is_undefined()
        {
            let m = a1.to_string(self)?;
            BaseObject::_put_prop(
                self,
                obj,
                &"message".into(),
                Value::String(m),
                true,
                false,
                true,
            );
        }
        let mut errors = Vec::new();
        if let Some(a0) = args.first() {
            errors = self.iterable_to_list(a0, None)?;
        }
        let arr = self.new_array_values(errors);
        BaseObject::_put_prop(
            self,
            obj,
            &"errors".into(),
            Value::Object(arr),
            true,
            false,
            true,
        );
        if let Some(a2) = args.get(2)
            && !a2.is_undefined()
        {
            self.error_cause(obj, a2)?;
        }
        Ok(obj)
    }

    pub fn error_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut sb = StringBuilder::new();
        let o = self.to_object(call.this)?;
        if let Some(val) = write_error_string(self, &mut sb, o)? {
            return Ok(Value::String(val));
        }
        Ok(Value::String(sb.string()))
    }

    pub fn create_error_prototype(&mut self, name: JsString, ctor: ObjRef) -> ObjRef {
        let ep = self.get_error_prototype();
        let o = self.new_base_object(Some(ep), CLASS_OBJECT);
        BaseObject::_put_prop(
            self,
            o,
            &"message".into(),
            Value::String(STRING_EMPTY.clone()),
            true,
            false,
            true,
        );
        BaseObject::_put_prop(
            self,
            o,
            &"name".into(),
            Value::String(name),
            true,
            false,
            true,
        );
        BaseObject::_put_prop(
            self,
            o,
            &"constructor".into(),
            Value::Object(ctor),
            true,
            false,
            true,
        );
        o
    }

    pub fn get_error_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.error_prototype {
            return r;
        }
        let ret = self.new_object();
        self.global.error_prototype = Some(ret);
        ret._put_prop(
            self,
            &"message".into(),
            Value::String(STRING_EMPTY.clone()),
            true,
            false,
            true,
        );
        ret._put_prop(
            self,
            &"name".into(),
            Value::String(STRING_ERROR.clone()),
            true,
            false,
            true,
        );
        let ts = self.new_native_func(
            crate::func::NativeFn::Fn(|r, c| r.error_to_string(c)),
            "toString",
            0,
        );
        ret._put_prop(
            self,
            &"toString".into(),
            Value::Object(ts),
            true,
            false,
            true,
        );
        let e = self.get_error();
        ret._put_prop(
            self,
            &"constructor".into(),
            Value::Object(e),
            true,
            false,
            true,
        );
        ret
    }

    pub fn get_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.error = Some(ret);
        let ep = self.get_error_prototype();
        self.new_native_func_construct(ret, |r, a, p| r.builtin_error(a, p), "Error", Some(ep), 1);
        ret
    }

    fn new_error_type(
        &mut self,
        ret: ObjRef,
        ctor: fn(&mut Runtime, &[Value], Option<ObjRef>) -> JsResult<ObjRef>,
        name: &str,
        sname: JsString,
        length: i64,
    ) {
        let proto = self.create_error_prototype(sname, ret);
        let e = self.get_error();
        self.new_native_func_construct_proto(ret, ctor, name, Some(proto), Some(e), length);
    }

    pub fn get_aggregate_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.aggregate_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.aggregate_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_aggregate_error(a, p),
            "AggregateError",
            STRING_AGGREGATE_ERROR.clone(),
            2,
        );
        ret
    }

    pub fn get_type_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.type_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.type_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_error(a, p),
            "TypeError",
            STRING_TYPE_ERROR.clone(),
            1,
        );
        ret
    }

    pub fn get_reference_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.reference_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.reference_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_error(a, p),
            "ReferenceError",
            STRING_REFERENCE_ERROR.clone(),
            1,
        );
        ret
    }

    pub fn get_syntax_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.syntax_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.syntax_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_error(a, p),
            "SyntaxError",
            STRING_SYNTAX_ERROR.clone(),
            1,
        );
        ret
    }

    pub fn get_range_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.range_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.range_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_error(a, p),
            "RangeError",
            STRING_RANGE_ERROR.clone(),
            1,
        );
        ret
    }

    pub fn get_eval_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.eval_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.eval_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_error(a, p),
            "EvalError",
            STRING_EVAL_ERROR.clone(),
            1,
        );
        ret
    }

    pub fn get_uri_error(&mut self) -> ObjRef {
        if let Some(r) = self.global.uri_error {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uri_error = Some(ret);
        self.new_error_type(
            ret,
            |r, a, p| r.builtin_error(a, p),
            "URIError",
            STRING_URI_ERROR.clone(),
            1,
        );
        ret
    }
}
