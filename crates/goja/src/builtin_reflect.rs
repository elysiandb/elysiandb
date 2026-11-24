use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{CLASS_OBJECT, FunctionCall};
use crate::runtime::{JsResult, Runtime, to_property_key};
use crate::string::ascii_string;
use crate::value::{Value, nil_safe, value_prop};

impl Runtime {
    pub fn builtin_reflect_apply(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let f = self.to_callable(call.arg(0))?;
        let args = self.create_list_from_array_like(call.arg(2))?;
        f.call(self, call.argument(1), &args)
    }

    pub fn builtin_reflect_construct(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = call.argument(0);
        let ctor = self.to_constructor(&target)?;
        let new_target = if call.arguments.len() > 2 {
            let nt = call.argument(2);
            self.to_constructor(&nt)?;
            nt
        } else {
            target
        };
        let args = self.create_list_from_array_like(call.arg(1))?;
        let nt = self.to_object(&new_target)?;
        Ok(Value::Object(ctor.construct(self, &args, Some(nt))?))
    }

    pub fn builtin_reflect_define_property(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let key = to_property_key(self, call.arg(1))?;
        let desc = self.to_property_descriptor(call.arg(2))?;
        let r = target.define_own_property(self, &key, desc, false)?;
        Ok(self.to_boolean(r))
    }

    pub fn builtin_reflect_delete_property(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let key = to_property_key(self, call.arg(1))?;
        let r = target.delete(self, &key, false)?;
        Ok(self.to_boolean(r))
    }

    pub fn builtin_reflect_get(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let key = to_property_key(self, call.arg(1))?;
        let receiver = call.arguments.get(2);
        Ok(nil_safe(target.get(self, &key, receiver)?))
    }

    pub fn builtin_reflect_get_own_property_descriptor(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let key = to_property_key(self, call.arg(1))?;
        let p = target.get_own_prop(self, &key)?;
        self.value_prop_to_descriptor_object(p)
    }

    pub fn builtin_reflect_get_prototype_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        if let Some(proto) = target.proto(self)? {
            return Ok(Value::Object(proto));
        }
        Ok(Value::Null)
    }

    pub fn builtin_reflect_has(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let key = to_property_key(self, call.arg(1))?;
        let r = target.has_property(self, &key)?;
        Ok(self.to_boolean(r))
    }

    pub fn builtin_reflect_is_extensible(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let r = target.is_extensible(self)?;
        Ok(self.to_boolean(r))
    }

    pub fn builtin_reflect_own_keys(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let keys = target.keys(self, true, Vec::new())?;
        Ok(Value::Object(self.new_array_values(keys)))
    }

    pub fn builtin_reflect_prevent_extensions(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let r = target.prevent_extensions(self, false)?;
        Ok(self.to_boolean(r))
    }

    pub fn builtin_reflect_set(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let receiver = if call.arguments.len() >= 4 {
            call.argument(3)
        } else {
            Value::Object(target)
        };
        let r = target.set(self, call.arg(1), call.argument(2), &receiver, false)?;
        Ok(self.to_boolean(r))
    }

    pub fn builtin_reflect_set_prototype_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let target = self.to_object(call.arg(0))?;
        let mut proto = None;
        let arg = call.arg(1);
        if !arg.is_null() {
            proto = Some(self.to_object(arg)?);
        }
        let r = target.set_proto(self, proto, false)?;
        Ok(self.to_boolean(r))
    }

    fn reflect_put_func(&mut self, o: ObjRef, name: &str, f: NativeFn, length: i64) {
        let f = self.new_native_func(f, name, length);
        o._put_prop(self, &name.into(), Value::Object(f), true, false, true);
    }

    pub fn create_reflect(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let o = val;

        self.reflect_put_func(
            o,
            "apply",
            NativeFn::Fn(|r, c| r.builtin_reflect_apply(c)),
            3,
        );
        self.reflect_put_func(
            o,
            "construct",
            NativeFn::Fn(|r, c| r.builtin_reflect_construct(c)),
            2,
        );
        self.reflect_put_func(
            o,
            "defineProperty",
            NativeFn::Fn(|r, c| r.builtin_reflect_define_property(c)),
            3,
        );
        self.reflect_put_func(
            o,
            "deleteProperty",
            NativeFn::Fn(|r, c| r.builtin_reflect_delete_property(c)),
            2,
        );
        self.reflect_put_func(o, "get", NativeFn::Fn(|r, c| r.builtin_reflect_get(c)), 2);
        self.reflect_put_func(
            o,
            "getOwnPropertyDescriptor",
            NativeFn::Fn(|r, c| r.builtin_reflect_get_own_property_descriptor(c)),
            2,
        );
        self.reflect_put_func(
            o,
            "getPrototypeOf",
            NativeFn::Fn(|r, c| r.builtin_reflect_get_prototype_of(c)),
            1,
        );
        self.reflect_put_func(o, "has", NativeFn::Fn(|r, c| r.builtin_reflect_has(c)), 2);
        self.reflect_put_func(
            o,
            "isExtensible",
            NativeFn::Fn(|r, c| r.builtin_reflect_is_extensible(c)),
            1,
        );
        self.reflect_put_func(
            o,
            "ownKeys",
            NativeFn::Fn(|r, c| r.builtin_reflect_own_keys(c)),
            1,
        );
        self.reflect_put_func(
            o,
            "preventExtensions",
            NativeFn::Fn(|r, c| r.builtin_reflect_prevent_extensions(c)),
            1,
        );
        self.reflect_put_func(o, "set", NativeFn::Fn(|r, c| r.builtin_reflect_set(c)), 3);
        self.reflect_put_func(
            o,
            "setPrototypeOf",
            NativeFn::Fn(|r, c| r.builtin_reflect_set_prototype_of(c)),
            2,
        );

        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(Value::String(ascii_string("Reflect")), false, false, true),
        );
    }

    pub fn get_reflect(&mut self) -> ObjRef {
        if let Some(r) = self.global.reflect {
            return r;
        }
        let ret = self.reserve_object();
        self.global.reflect = Some(ret);
        self.create_reflect(ret);
        ret
    }
}
