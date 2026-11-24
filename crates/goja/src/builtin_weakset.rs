use crate::builtin_map::incompatible_receiver;
use crate::builtin_set::{check_std_array_iter, std_array_item, std_array_len};
use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::builtin_weakmap::WeakMap;
use crate::func::NativeFn;
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_OBJECT, CLASS_WEAK_SET, FunctionCall, ObjData, ObjExt,
    to_method,
};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::ascii_string;
use crate::value::{Value, value_prop};

pub struct WeakSetObject {
    pub s: WeakMap,
}

impl ObjData for WeakSetObject {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Runtime {
    fn this_weak_set(&mut self, call: &FunctionCall<'_>, method: &str) -> JsResult<WeakMap> {
        let this_obj = self.to_object(call.this)?;
        match self.ext_dyn::<WeakSetObject>(this_obj) {
            Some(wso) => Ok(wso.s),
            None => Err(incompatible_receiver(self, method, this_obj)),
        }
    }

    pub fn weak_set_proto_add(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let s = self.this_weak_set(&call, "WeakSet.prototype.add")?;
        let key = self.to_object(call.arg(0))?;
        s.set(self, key, Value::Empty);
        Ok(call.this.clone())
    }

    pub fn weak_set_proto_delete(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let s = self.this_weak_set(&call, "WeakSet.prototype.delete")?;
        if let Value::Object(obj) = call.arg(0)
            && s.remove(self, *obj)
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn weak_set_proto_has(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let s = self.this_weak_set(&call, "WeakSet.prototype.has")?;
        if let Value::Object(obj) = call.arg(0)
            && s.has(self, *obj)
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn builtin_new_weak_set(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("WeakSet")));
        };
        let (def_ctor, def_proto) = (self.global.weak_set, self.global.weak_set_prototype);
        let proto = self.get_prototype_from_ctor(new_target, def_ctor, def_proto)?;
        let s = WeakMap::new(self);
        let o = self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(WeakSetObject { s })),
        );
        if let Some(arg) = args.first()
            && !matches!(arg, Value::Undefined | Value::Null)
        {
            let adder = o.get_str(self, &"add".into(), None)?;
            let std_arr = check_std_array_iter(self, arg)?;
            if matches!(&adder, Some(Value::Object(a)) if Some(*a) == self.global.weak_set_adder) {
                if let Some(arr) = std_arr {
                    for i in 0..std_array_len(self, arr) {
                        let v = std_array_item(self, arr, i);
                        let key = self.to_object(&v)?;
                        s.set(self, key, Value::Empty);
                    }
                } else {
                    let mut iter = self.get_iterator(arg, None)?;
                    iter.iterate(self, &mut |rt, item| {
                        let key = rt.to_object(&item)?;
                        s.set(rt, key, Value::Empty);
                        Ok(())
                    })?;
                }
            } else {
                let Some(adder_fn) = to_method(self, adder)? else {
                    return Err(Throw::Value(self.new_type_error("WeakSet.add in missing")));
                };
                if let Some(arr) = std_arr {
                    for i in 0..std_array_len(self, arr) {
                        let item = std_array_item(self, arr, i);
                        adder_fn.call(self, Value::Object(o), &[item])?;
                    }
                } else {
                    let mut iter = self.get_iterator(arg, None)?;
                    iter.iterate(self, &mut |rt, item| {
                        adder_fn.call(rt, Value::Object(o), &[item])?;
                        Ok(())
                    })?;
                }
            }
        }
        Ok(o)
    }

    fn create_weak_set_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let o = val;

        let c = match self.global.weak_set {
            Some(c) => Value::Object(c),
            None => Value::Undefined,
        };
        o._put_prop(self, &"constructor".into(), c, true, false, true);
        let adder = self.new_native_func(NativeFn::Fn(|r, c| r.weak_set_proto_add(c)), "add", 1);
        self.global.weak_set_adder = Some(adder);
        o._put_prop(self, &"add".into(), Value::Object(adder), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.weak_set_proto_delete(c)), "delete", 1);
        o._put_prop(self, &"delete".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.weak_set_proto_has(c)), "has", 1);
        o._put_prop(self, &"has".into(), Value::Object(f), true, false, true);

        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_WEAK_SET)),
                false,
                false,
                true,
            ),
        );
    }

    fn create_weak_set(&mut self, val: ObjRef) {
        let proto = self.get_weak_set_prototype();
        self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_weak_set(args, nt),
            Some(proto),
            "WeakSet",
            0,
        );
    }

    pub fn get_weak_set_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.weak_set_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.weak_set_prototype = Some(ret);
        self.create_weak_set_proto(ret);
        ret
    }

    pub fn get_weak_set(&mut self) -> ObjRef {
        if let Some(r) = self.global.weak_set {
            return r;
        }
        let ret = self.reserve_object();
        self.global.weak_set = Some(ret);
        self.create_weak_set(ret);
        ret
    }
}
