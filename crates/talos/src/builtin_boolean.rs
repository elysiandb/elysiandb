use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{CLASS_BOOLEAN, FunctionCall, ObjExt};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::{STRING_FALSE, STRING_TRUE};
use crate::value::{Value, int_to_value};

impl Runtime {
    pub fn booleanproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut b = None;
        match call.this {
            Value::Bool(v) => b = Some(*v),
            Value::Object(o) => {
                if let ObjExt::Primitive(Value::Bool(v)) = &self.obj(*o).ext {
                    b = Some(*v);
                }
            }
            _ => {}
        }
        match b {
            Some(true) => Ok(Value::String(STRING_TRUE.clone())),
            Some(false) => Ok(Value::String(STRING_FALSE.clone())),
            None => Err(Throw::Value(self.new_type_error(
                "Method Boolean.prototype.toString is called on incompatible receiver",
            ))),
        }
    }

    pub fn booleanproto_value_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        match call.this {
            Value::Bool(v) => return Ok(Value::Bool(*v)),
            Value::Object(o) => {
                if let ObjExt::Primitive(Value::Bool(v)) = &self.obj(*o).ext {
                    return Ok(Value::Bool(*v));
                }
            }
            _ => {}
        }
        Err(Throw::Value(self.new_type_error(
            "Method Boolean.prototype.valueOf is called on incompatible receiver",
        )))
    }

    pub fn get_boolean_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.boolean_prototype {
            return r;
        }
        let op = self.global.object_prototype;
        let ret = self.new_primitive_object(Value::Bool(false), op, CLASS_BOOLEAN);
        self.global.boolean_prototype = Some(ret);
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.booleanproto_to_string(c)),
            "toString",
            0,
        );
        ret._put_prop(
            self,
            &"toString".into(),
            Value::Object(f),
            true,
            false,
            true,
        );
        let f = self.new_native_func(
            NativeFn::Fn(|r, c| r.booleanproto_value_of(c)),
            "valueOf",
            0,
        );
        ret._put_prop(self, &"valueOf".into(), Value::Object(f), true, false, true);
        let b = self.get_boolean();
        ret._put_prop(
            self,
            &"constructor".into(),
            Value::Object(b),
            true,
            false,
            true,
        );
        ret
    }

    pub fn get_boolean(&mut self) -> ObjRef {
        if let Some(r) = self.global.boolean {
            return r;
        }
        let ret = self.reserve_object();
        self.global.boolean = Some(ret);
        let proto = self.get_boolean_prototype();
        let ctor = self.wrap_native_construct(
            Some(|r: &mut Runtime, args: &[Value], proto: Option<ObjRef>| {
                r.builtin_new_boolean(args, proto)
            }),
            ret,
            Some(proto),
        );
        self.new_native_func_and_construct(
            Some(ret),
            Some(NativeFn::Fn(|r, c| r.builtin_boolean(c))),
            ctor,
            Some(proto),
            "Boolean",
            int_to_value(1),
        );
        ret
    }

    pub fn builtin_boolean(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Some(a) = call.arguments.first() {
            return Ok(Value::Bool(a.to_boolean()));
        }
        Ok(Value::Bool(false))
    }

    pub fn builtin_new_boolean(
        &mut self,
        args: &[Value],
        proto: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let v = match args.first() {
            Some(a) => Value::Bool(a.to_boolean()),
            None => Value::Bool(false),
        };
        Ok(self.new_primitive_object(v, proto, CLASS_BOOLEAN))
    }
}
