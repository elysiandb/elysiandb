use crate::builtin_map::incompatible_receiver;
use crate::builtin_symbol::SYM_TO_STRING_TAG;
use crate::func::NativeFn;
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_OBJECT, CLASS_WEAK_MAP, FunctionCall, ObjData, ObjExt,
    to_method,
};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::ascii_string;
use crate::value::{Value, nil_safe, value_prop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct WeakMap(pub u64);

impl WeakMap {
    pub fn new(rt: &mut Runtime) -> WeakMap {
        WeakMap(rt.gen_id())
    }

    pub fn set(self, rt: &mut Runtime, key: ObjRef, value: Value) {
        key.get_weak_refs(rt).insert(self.0, value);
    }

    pub fn get(self, rt: &Runtime, key: ObjRef) -> Option<Value> {
        rt.obj(key)
            .weak_refs
            .as_ref()
            .and_then(|w| w.get(&self.0))
            .cloned()
    }

    pub fn remove(self, rt: &mut Runtime, key: ObjRef) -> bool {
        match &mut rt.obj_mut(key).weak_refs {
            Some(w) => w.remove(&self.0).is_some(),
            None => false,
        }
    }

    pub fn has(self, rt: &Runtime, key: ObjRef) -> bool {
        match &rt.obj(key).weak_refs {
            Some(w) => w.contains_key(&self.0),
            None => false,
        }
    }
}

pub struct WeakMapObject {
    pub m: WeakMap,
}

impl ObjData for WeakMapObject {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl Runtime {
    fn this_weak_map(&mut self, call: &FunctionCall<'_>, method: &str) -> JsResult<WeakMap> {
        let this_obj = self.to_object(call.this)?;
        match self.ext_dyn::<WeakMapObject>(this_obj) {
            Some(wmo) => Ok(wmo.m),
            None => Err(incompatible_receiver(self, method, this_obj)),
        }
    }

    pub fn weak_map_proto_delete(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let wm = self.this_weak_map(&call, "WeakMap.prototype.delete")?;
        if let Value::Object(key) = call.arg(0)
            && wm.remove(self, *key)
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn weak_map_proto_get(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let wm = self.this_weak_map(&call, "WeakMap.prototype.get")?;
        let mut res = None;
        if let Value::Object(key) = call.arg(0) {
            res = wm.get(self, *key);
        }
        Ok(nil_safe(res))
    }

    pub fn weak_map_proto_has(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let wm = self.this_weak_map(&call, "WeakMap.prototype.has")?;
        if let Value::Object(key) = call.arg(0)
            && wm.has(self, *key)
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn weak_map_proto_set(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let wm = self.this_weak_map(&call, "WeakMap.prototype.set")?;
        let key = self.to_object(call.arg(0))?;
        wm.set(self, key, call.argument(1));
        Ok(call.this.clone())
    }

    pub fn need_new(&mut self, name: &str) -> Value {
        self.new_type_error(&format!("Constructor {name} requires 'new'"))
    }

    pub fn builtin_new_weak_map(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("WeakMap")));
        };
        let (def_ctor, def_proto) = (self.global.weak_map, self.global.weak_map_prototype);
        let proto = self.get_prototype_from_ctor(new_target, def_ctor, def_proto)?;
        let m = WeakMap::new(self);
        let o = self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(WeakMapObject { m })),
        );
        if let Some(arg) = args.first()
            && !matches!(arg, Value::Undefined | Value::Null)
        {
            let adder = o.get_str(self, &"set".into(), None)?;
            let is_std =
                matches!(&adder, Some(Value::Object(a)) if Some(*a) == self.global.weak_map_adder);
            let Some(adder_fn) = to_method(self, adder)? else {
                return Err(Throw::Value(self.new_type_error("WeakMap.set in missing")));
            };
            let mut iter = self.get_iterator(arg, None)?;
            if is_std {
                iter.iterate(self, &mut |rt, item| {
                    let item_obj = rt.to_object(&item)?;
                    let k = nil_safe(item_obj.get_idx(rt, 0, None)?);
                    let v = nil_safe(item_obj.get_idx(rt, 1, None)?);
                    let key = rt.to_object(&k)?;
                    m.set(rt, key, v);
                    Ok(())
                })?;
            } else {
                iter.iterate(self, &mut |rt, item| {
                    let item_obj = rt.to_object(&item)?;
                    let k = nil_safe(item_obj.get_idx(rt, 0, None)?);
                    let v = nil_safe(item_obj.get_idx(rt, 1, None)?);
                    adder_fn.call(rt, Value::Object(o), &[k, v])?;
                    Ok(())
                })?;
            }
        }
        Ok(o)
    }

    fn create_weak_map_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let o = val;

        let c = self.get_weak_map();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(c),
            true,
            false,
            true,
        );
        let adder = self.new_native_func(NativeFn::Fn(|r, c| r.weak_map_proto_set(c)), "set", 2);
        self.global.weak_map_adder = Some(adder);
        o._put_prop(self, &"set".into(), Value::Object(adder), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.weak_map_proto_delete(c)), "delete", 1);
        o._put_prop(self, &"delete".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.weak_map_proto_has(c)), "has", 1);
        o._put_prop(self, &"has".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.weak_map_proto_get(c)), "get", 1);
        o._put_prop(self, &"get".into(), Value::Object(f), true, false, true);

        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_WEAK_MAP)),
                false,
                false,
                true,
            ),
        );
    }

    fn create_weak_map(&mut self, val: ObjRef) {
        let proto = self.get_weak_map_prototype();
        self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_weak_map(args, nt),
            Some(proto),
            "WeakMap",
            0,
        );
    }

    pub fn get_weak_map_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.weak_map_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.weak_map_prototype = Some(ret);
        self.create_weak_map_proto(ret);
        ret
    }

    pub fn get_weak_map(&mut self) -> ObjRef {
        if let Some(r) = self.global.weak_map {
            return r;
        }
        let ret = self.reserve_object();
        self.global.weak_map = Some(ret);
        self.create_weak_map(ret);
        ret
    }
}
