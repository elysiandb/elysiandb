use crate::builtin_symbol::{SYM_ITERATOR, SYM_TO_STRING_TAG};
use crate::func::NativeFn;
use crate::heap::{ObjRef, Tracer};
use crate::map::OrderedMap;
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_MAP, CLASS_MAP_ITERATOR, CLASS_OBJECT, FunctionCall,
    ObjData, ObjExt, to_method,
};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::ascii_string;
use crate::value::{Value, ValueProperty, int_to_value, nil_safe, value_prop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IterationKind {
    Key,
    Value,
    KeyValue,
}

pub struct MapObject {
    pub m: OrderedMap,
}

impl ObjData for MapObject {
    fn trace(&self, t: &mut Tracer) {
        self.m.trace(t);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

impl MapObject {
    pub fn export(&self) -> Vec<[Value; 2]> {
        let mut m = Vec::with_capacity(self.m.size());
        for (k, v) in self.m.entries() {
            m.push([k.clone(), v.clone()]);
        }
        m
    }
}

#[derive(Clone, Copy)]
struct OrderedMapIter {
    m: ObjRef,
    it: crate::map::OrderedMapIter,
}

pub struct MapIterObject {
    iter: Option<OrderedMapIter>,
    kind: IterationKind,
}

impl ObjData for MapIterObject {
    fn trace(&self, t: &mut Tracer) {
        if let Some(it) = &self.iter {
            t.obj(it.m);
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

fn map_data(rt: &Runtime, o: ObjRef) -> &MapObject {
    match rt.ext_dyn::<MapObject>(o) {
        Some(m) => m,
        None => unreachable!("not a Map"),
    }
}

fn map_data_mut(rt: &mut Runtime, o: ObjRef) -> &mut MapObject {
    match rt.ext_dyn_mut::<MapObject>(o) {
        Some(m) => m,
        None => unreachable!("not a Map"),
    }
}

pub(crate) fn incompatible_receiver(rt: &mut Runtime, method: &str, this_obj: ObjRef) -> Throw {
    let this = Value::Object(this_obj);
    match rt.objectproto_to_string(FunctionCall::new(&this, &[])) {
        Ok(s) => {
            let s = s.prim_to_string().to_rust_string();
            Throw::Value(rt.new_type_error(&format!(
                "Method {method} called on incompatible receiver {s}"
            )))
        }
        Err(e) => e,
    }
}

impl MapIterObject {
    fn next(rt: &mut Runtime, o: ObjRef) -> Value {
        let Some(mut it) = rt.ext_dyn::<MapIterObject>(o).and_then(|i| i.iter) else {
            return rt.create_iter_result_object(Value::Undefined, true);
        };
        let kind = rt.ext_dyn::<MapIterObject>(o).map(|i| i.kind);
        let entry = map_data_mut(rt, it.m).m.iter_next(&mut it.it);
        let Some((key, value)) = entry else {
            if let Some(i) = rt.ext_dyn_mut::<MapIterObject>(o) {
                i.iter = None;
            }
            return rt.create_iter_result_object(Value::Undefined, true);
        };
        if let Some(i) = rt.ext_dyn_mut::<MapIterObject>(o) {
            i.iter = Some(it);
        }
        let result = match kind {
            Some(IterationKind::Key) => key,
            Some(IterationKind::Value) => value,
            _ => Value::Object(rt.new_array_values(vec![key, value])),
        };
        rt.create_iter_result_object(result, false)
    }
}

impl Runtime {
    fn this_map(&mut self, call: &FunctionCall<'_>, method: &str) -> JsResult<ObjRef> {
        let this_obj = self.to_object(call.this)?;
        if self.ext_dyn::<MapObject>(this_obj).is_none() {
            return Err(incompatible_receiver(self, method, this_obj));
        }
        Ok(this_obj)
    }

    pub fn map_proto_clear(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mo = self.this_map(&call, "Map.prototype.clear")?;
        map_data_mut(self, mo).m.clear();
        Ok(Value::Undefined)
    }

    pub fn map_proto_delete(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mo = self.this_map(&call, "Map.prototype.delete")?;
        let r = map_data_mut(self, mo).m.remove(call.arg(0));
        Ok(self.to_boolean(r))
    }

    pub fn map_proto_get(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mo = self.this_map(&call, "Map.prototype.get")?;
        Ok(nil_safe(map_data(self, mo).m.get(call.arg(0)).cloned()))
    }

    pub fn map_proto_has(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mo = self.this_map(&call, "Map.prototype.has")?;
        if map_data(self, mo).m.has(call.arg(0)) {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn map_proto_set(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mo = self.this_map(&call, "Map.prototype.set")?;
        map_data_mut(self, mo)
            .m
            .set(call.argument(0), call.argument(1));
        Ok(call.this.clone())
    }

    pub fn map_proto_entries(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.create_map_iterator(call.this, IterationKind::KeyValue)
    }

    pub fn map_proto_for_each(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.this_map(&call, "Map.prototype.forEach")?;
        let cb = self.to_object(call.arg(0))?;
        let Some(callback_fn) = cb.assert_callable(self) else {
            return Err(Throw::Value(
                self.new_type_error("object is not a function %!s(MISSING)"),
            ));
        };
        let t = call.argument(1);
        let mut it = crate::map::OrderedMapIter::new();
        loop {
            let entry = map_data_mut(self, this_obj).m.iter_next(&mut it);
            let Some((key, value)) = entry else {
                break;
            };
            if let Err(e) =
                callback_fn.call(self, t.clone(), &[value, key, Value::Object(this_obj)])
            {
                map_data_mut(self, this_obj).m.iter_close(&mut it);
                return Err(e);
            }
        }
        Ok(Value::Undefined)
    }

    pub fn map_proto_keys(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.create_map_iterator(call.this, IterationKind::Key)
    }

    pub fn map_proto_values(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.create_map_iterator(call.this, IterationKind::Value)
    }

    pub fn map_proto_get_size(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mo = self.this_map(&call, "get Map.prototype.size")?;
        Ok(int_to_value(map_data(self, mo).m.size() as i64))
    }

    pub fn builtin_new_map(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("Map")));
        };
        let (def_ctor, def_proto) = (self.global.map, self.global.map_prototype);
        let proto = self.get_prototype_from_ctor(new_target, def_ctor, def_proto)?;
        let o = self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(MapObject {
                m: OrderedMap::new(),
            })),
        );
        if let Some(arg) = args.first()
            && !matches!(arg, Value::Undefined | Value::Null)
        {
            let adder = o.get_str(self, &"set".into(), None)?;
            let is_std =
                matches!(&adder, Some(Value::Object(a)) if Some(*a) == self.global.map_adder);
            let Some(adder_fn) = to_method(self, adder)? else {
                return Err(Throw::Value(self.new_type_error("Map.set in missing")));
            };
            let mut iter = self.get_iterator(arg, None)?;
            if is_std {
                iter.iterate(self, &mut |rt, item| {
                    let item_obj = rt.to_object(&item)?;
                    let k = nil_safe(item_obj.get_idx(rt, 0, None)?);
                    let v = nil_safe(item_obj.get_idx(rt, 1, None)?);
                    map_data_mut(rt, o).m.set(k, v);
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

    fn create_map_iterator(&mut self, map_value: &Value, kind: IterationKind) -> JsResult<Value> {
        let obj = self.to_object(map_value)?;
        if self.ext_dyn::<MapObject>(obj).is_none() {
            return Err(Throw::Value(self.new_type_error("Object is not a Map")));
        }
        let proto = self.get_map_iterator_prototype();
        let o = self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, Some(proto)),
            ObjExt::Dyn(Box::new(MapIterObject {
                iter: Some(OrderedMapIter {
                    m: obj,
                    it: crate::map::OrderedMapIter::new(),
                }),
                kind,
            })),
        );
        Ok(Value::Object(o))
    }

    pub fn map_iter_proto_next(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        if self.ext_dyn::<MapIterObject>(this_obj).is_some() {
            return Ok(MapIterObject::next(self, this_obj));
        }
        Err(incompatible_receiver(
            self,
            "Map Iterator.prototype.next",
            this_obj,
        ))
    }

    fn create_map_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let o = val;

        let c = self.get_map();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(c),
            true,
            false,
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_clear(c)), "clear", 0);
        o._put_prop(self, &"clear".into(), Value::Object(f), true, false, true);
        let adder = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_set(c)), "set", 2);
        self.global.map_adder = Some(adder);
        o._put_prop(self, &"set".into(), Value::Object(adder), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_delete(c)), "delete", 1);
        o._put_prop(self, &"delete".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_for_each(c)), "forEach", 1);
        o._put_prop(self, &"forEach".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_has(c)), "has", 1);
        o._put_prop(self, &"has".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_get(c)), "get", 1);
        o._put_prop(self, &"get".into(), Value::Object(f), true, false, true);
        let g = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_get_size(c)), "get size", 0);
        let _ = o.set_own_str(
            self,
            &"size".into(),
            Value::Property(Box::new(ValueProperty {
                getter_func: Some(g),
                accessor: true,
                writable: true,
                configurable: true,
                ..ValueProperty::default()
            })),
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_keys(c)), "keys", 0);
        o._put_prop(self, &"keys".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_values(c)), "values", 0);
        o._put_prop(self, &"values".into(), Value::Object(f), true, false, true);

        let entries_func =
            self.new_native_func(NativeFn::Fn(|r, c| r.map_proto_entries(c)), "entries", 0);
        o._put_prop(
            self,
            &"entries".into(),
            Value::Object(entries_func),
            true,
            false,
            true,
        );
        o._put_sym(
            self,
            SYM_ITERATOR,
            value_prop(Value::Object(entries_func), true, false, true),
        );
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(Value::String(ascii_string(CLASS_MAP)), false, false, true),
        );
    }

    fn create_map(&mut self, val: ObjRef) {
        let proto = self.get_map_prototype();
        let o = self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_map(args, nt),
            Some(proto),
            "Map",
            0,
        );
        self.put_species_return_this(o);
    }

    fn create_map_iter_proto(&mut self, val: ObjRef) {
        let ip = self.get_iterator_prototype();
        self.new_base_object_obj(val, Some(ip), CLASS_OBJECT);
        let o = val;

        let f = self.new_native_func(NativeFn::Fn(|r, c| r.map_iter_proto_next(c)), "next", 0);
        o._put_prop(self, &"next".into(), Value::Object(f), true, false, true);
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_MAP_ITERATOR)),
                false,
                false,
                true,
            ),
        );
    }

    pub fn get_map_iterator_prototype(&mut self) -> ObjRef {
        if let Some(o) = self.global.map_iterator_prototype {
            return o;
        }
        let o = self.reserve_object();
        self.global.map_iterator_prototype = Some(o);
        self.create_map_iter_proto(o);
        o
    }

    pub fn get_map_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.map_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.map_prototype = Some(ret);
        self.create_map_proto(ret);
        ret
    }

    pub fn get_map(&mut self) -> ObjRef {
        if let Some(r) = self.global.map {
            return r;
        }
        let ret = self.reserve_object();
        self.global.map = Some(ret);
        self.create_map(ret);
        ret
    }
}
