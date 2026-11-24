use crate::builtin_map::incompatible_receiver;
use crate::builtin_symbol::{SYM_ITERATOR, SYM_TO_STRING_TAG};
use crate::func::NativeFn;
use crate::heap::{ObjRef, Tracer};
use crate::map::OrderedMap;
use crate::object::{
    BASE_OBJECT_CLASS, BaseObject, CLASS_OBJECT, CLASS_SET, CLASS_SET_ITERATOR, FunctionCall,
    ObjData, ObjExt, to_method,
};
use crate::runtime::{JsResult, Runtime, Throw};
use crate::string::ascii_string;
use crate::value::{Value, ValueProperty, int_to_value, nil_safe, value_prop};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum IterationKind {
    Value,
    KeyValue,
}

pub struct SetObject {
    pub m: OrderedMap,
}

impl ObjData for SetObject {
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

impl SetObject {
    pub fn export(&self) -> Vec<Value> {
        let mut a = Vec::with_capacity(self.m.size());
        for (k, _) in self.m.entries() {
            a.push(k.clone());
        }
        a
    }
}

#[derive(Clone, Copy)]
struct OrderedMapIter {
    m: ObjRef,
    it: crate::map::OrderedMapIter,
}

pub struct SetIterObject {
    iter: Option<OrderedMapIter>,
    kind: IterationKind,
}

impl ObjData for SetIterObject {
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

fn set_data(rt: &Runtime, o: ObjRef) -> &SetObject {
    match rt.ext_dyn::<SetObject>(o) {
        Some(s) => s,
        None => unreachable!("not a Set"),
    }
}

fn set_data_mut(rt: &mut Runtime, o: ObjRef) -> &mut SetObject {
    match rt.ext_dyn_mut::<SetObject>(o) {
        Some(s) => s,
        None => unreachable!("not a Set"),
    }
}

pub(crate) fn check_std_array_iter(rt: &mut Runtime, v: &Value) -> JsResult<Option<ObjRef>> {
    let Value::Object(arr) = v else {
        return Ok(None);
    };
    let arr = *arr;
    let std = match &rt.obj(arr).ext {
        ObjExt::Array(a) => {
            a.prop_value_count == 0
                && a.length as usize == a.values.len()
                && a.obj_count as u64 == u64::from(a.length)
        }
        _ => false,
    };
    if !std {
        return Ok(None);
    }
    let it = arr.get_sym(rt, SYM_ITERATOR, None)?;
    let values = rt.get_array_values();
    if matches!(it, Some(Value::Object(f)) if f == values) {
        return Ok(Some(arr));
    }
    Ok(None)
}

pub(crate) fn std_array_len(rt: &Runtime, arr: ObjRef) -> usize {
    match &rt.obj(arr).ext {
        ObjExt::Array(a) => a.values.len(),
        _ => 0,
    }
}

pub(crate) fn std_array_item(rt: &Runtime, arr: ObjRef, i: usize) -> Value {
    match &rt.obj(arr).ext {
        ObjExt::Array(a) => nil_safe(a.values.get(i).cloned()),
        _ => Value::Undefined,
    }
}

impl SetIterObject {
    fn next(rt: &mut Runtime, o: ObjRef) -> Value {
        let Some(mut it) = rt.ext_dyn::<SetIterObject>(o).and_then(|i| i.iter) else {
            return rt.create_iter_result_object(Value::Undefined, true);
        };
        let kind = rt.ext_dyn::<SetIterObject>(o).map(|i| i.kind);
        let entry = set_data_mut(rt, it.m)
            .m
            .iter_next(&mut it.it)
            .map(|(k, _)| k);
        let Some(key) = entry else {
            if let Some(i) = rt.ext_dyn_mut::<SetIterObject>(o) {
                i.iter = None;
            }
            return rt.create_iter_result_object(Value::Undefined, true);
        };
        if let Some(i) = rt.ext_dyn_mut::<SetIterObject>(o) {
            i.iter = Some(it);
        }
        let result = match kind {
            Some(IterationKind::Value) => key,
            _ => Value::Object(rt.new_array_values(vec![key.clone(), key])),
        };
        rt.create_iter_result_object(result, false)
    }
}

impl Runtime {
    fn this_set(&mut self, call: &FunctionCall<'_>, method: &str) -> JsResult<ObjRef> {
        let this_obj = self.to_object(call.this)?;
        if self.ext_dyn::<SetObject>(this_obj).is_none() {
            return Err(incompatible_receiver(self, method, this_obj));
        }
        Ok(this_obj)
    }

    pub fn set_proto_add(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let so = self.this_set(&call, "Set.prototype.add")?;
        set_data_mut(self, so).m.set(call.argument(0), Value::Empty);
        Ok(call.this.clone())
    }

    pub fn set_proto_clear(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let so = self.this_set(&call, "Set.prototype.clear")?;
        set_data_mut(self, so).m.clear();
        Ok(Value::Undefined)
    }

    pub fn set_proto_delete(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let so = self.this_set(&call, "Set.prototype.delete")?;
        let r = set_data_mut(self, so).m.remove(call.arg(0));
        Ok(self.to_boolean(r))
    }

    pub fn set_proto_entries(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.create_set_iterator(call.this, IterationKind::KeyValue)
    }

    pub fn set_proto_for_each(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.this_set(&call, "Set.prototype.forEach")?;
        let cb = self.to_object(call.arg(0))?;
        let Some(callback_fn) = cb.assert_callable(self) else {
            return Err(Throw::Value(
                self.new_type_error("object is not a function %!s(MISSING)"),
            ));
        };
        let t = call.argument(1);
        let mut it = crate::map::OrderedMapIter::new();
        loop {
            let entry = set_data_mut(self, this_obj)
                .m
                .iter_next(&mut it)
                .map(|(k, _)| k);
            let Some(key) = entry else {
                break;
            };
            if let Err(e) = callback_fn.call(
                self,
                t.clone(),
                &[key.clone(), key, Value::Object(this_obj)],
            ) {
                set_data_mut(self, this_obj).m.iter_close(&mut it);
                return Err(e);
            }
        }
        Ok(Value::Undefined)
    }

    pub fn set_proto_has(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let so = self.this_set(&call, "Set.prototype.has")?;
        let r = set_data(self, so).m.has(call.arg(0));
        Ok(self.to_boolean(r))
    }

    pub fn set_proto_get_size(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let so = self.this_set(&call, "get Set.prototype.size")?;
        Ok(int_to_value(set_data(self, so).m.size() as i64))
    }

    pub fn set_proto_values(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        self.create_set_iterator(call.this, IterationKind::Value)
    }

    pub fn builtin_new_set(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("Set")));
        };
        let (def_ctor, def_proto) = (self.global.set, self.global.set_prototype);
        let proto = self.get_prototype_from_ctor(new_target, def_ctor, def_proto)?;
        let o = self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(SetObject {
                m: OrderedMap::new(),
            })),
        );
        if let Some(arg) = args.first()
            && !matches!(arg, Value::Undefined | Value::Null)
        {
            let adder = o.get_str(self, &"add".into(), None)?;
            let std_arr = check_std_array_iter(self, arg)?;
            if matches!(&adder, Some(Value::Object(a)) if Some(*a) == self.global.set_adder) {
                if let Some(arr) = std_arr {
                    for i in 0..std_array_len(self, arr) {
                        let v = std_array_item(self, arr, i);
                        set_data_mut(self, o).m.set(v, Value::Empty);
                    }
                } else {
                    let mut iter = self.get_iterator(arg, None)?;
                    iter.iterate(self, &mut |rt, item| {
                        set_data_mut(rt, o).m.set(item, Value::Empty);
                        Ok(())
                    })?;
                }
            } else {
                let Some(adder_fn) = to_method(self, adder)? else {
                    return Err(Throw::Value(self.new_type_error("Set.add in missing")));
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

    fn create_set_iterator(&mut self, set_value: &Value, kind: IterationKind) -> JsResult<Value> {
        let obj = self.to_object(set_value)?;
        if self.ext_dyn::<SetObject>(obj).is_none() {
            return Err(Throw::Value(self.new_type_error("Object is not a Set")));
        }
        let proto = self.get_set_iterator_prototype();
        let o = self.alloc_object(
            &BASE_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, Some(proto)),
            ObjExt::Dyn(Box::new(SetIterObject {
                iter: Some(OrderedMapIter {
                    m: obj,
                    it: crate::map::OrderedMapIter::new(),
                }),
                kind,
            })),
        );
        Ok(Value::Object(o))
    }

    pub fn set_iter_proto_next(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        if self.ext_dyn::<SetIterObject>(this_obj).is_some() {
            return Ok(SetIterObject::next(self, this_obj));
        }
        Err(incompatible_receiver(
            self,
            "Set Iterator.prototype.next",
            this_obj,
        ))
    }

    fn create_set_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let o = val;

        let c = self.get_set();
        o._put_prop(
            self,
            &"constructor".into(),
            Value::Object(c),
            true,
            false,
            true,
        );
        let adder = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_add(c)), "add", 1);
        self.global.set_adder = Some(adder);
        o._put_prop(self, &"add".into(), Value::Object(adder), true, false, true);

        let f = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_clear(c)), "clear", 0);
        o._put_prop(self, &"clear".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_delete(c)), "delete", 1);
        o._put_prop(self, &"delete".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_for_each(c)), "forEach", 1);
        o._put_prop(self, &"forEach".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_has(c)), "has", 1);
        o._put_prop(self, &"has".into(), Value::Object(f), true, false, true);
        let g = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_get_size(c)), "get size", 0);
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

        let values_func =
            self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_values(c)), "values", 0);
        o._put_prop(
            self,
            &"values".into(),
            Value::Object(values_func),
            true,
            false,
            true,
        );
        o._put_prop(
            self,
            &"keys".into(),
            Value::Object(values_func),
            true,
            false,
            true,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.set_proto_entries(c)), "entries", 0);
        o._put_prop(self, &"entries".into(), Value::Object(f), true, false, true);
        o._put_sym(
            self,
            SYM_ITERATOR,
            value_prop(Value::Object(values_func), true, false, true),
        );
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(Value::String(ascii_string(CLASS_SET)), false, false, true),
        );
    }

    fn create_set(&mut self, val: ObjRef) {
        let proto = self.get_set_prototype();
        let o = self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_set(args, nt),
            Some(proto),
            "Set",
            0,
        );
        self.put_species_return_this(o);
    }

    fn create_set_iter_proto(&mut self, val: ObjRef) {
        let ip = self.get_iterator_prototype();
        self.new_base_object_obj(val, Some(ip), CLASS_OBJECT);
        let o = val;

        let f = self.new_native_func(NativeFn::Fn(|r, c| r.set_iter_proto_next(c)), "next", 0);
        o._put_prop(self, &"next".into(), Value::Object(f), true, false, true);
        o._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_SET_ITERATOR)),
                false,
                false,
                true,
            ),
        );
    }

    pub fn get_set_iterator_prototype(&mut self) -> ObjRef {
        if let Some(o) = self.global.set_iterator_prototype {
            return o;
        }
        let o = self.reserve_object();
        self.global.set_iterator_prototype = Some(o);
        self.create_set_iter_proto(o);
        o
    }

    pub fn get_set_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.set_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.set_prototype = Some(ret);
        self.create_set_proto(ret);
        ret
    }

    pub fn get_set(&mut self) -> ObjRef {
        if let Some(r) = self.global.set {
            return r;
        }
        let ret = self.reserve_object();
        self.global.set = Some(ret);
        self.create_set(ret);
        ret
    }
}
