use super::goarray_reflect::{GoPairArray, IndexIter, PairCell};
use super::goreflect::describe_value;
use super::goslice::{define_length, delete_length};
use super::{DocValue, Pair, TypedSlice, check_host_object_property_descr};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ARRAY, IterNextFunc, ObjClass, ObjData, ObjExt,
    PropertyDescriptor, UStr, idx_string,
};
use crate::runtime::{JsResult, Runtime, grow_cap, str_to_go_idx, str_to_idx64};
use crate::string::ascii_string;
use crate::value::{Value, ValueProperty, float_to_value, int_to_value};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

pub type Backing<E> = Rc<RefCell<Vec<E>>>;

pub trait GoElem: Clone + Default + 'static {
    fn to_js(&self, rt: &mut Runtime) -> JsResult<Value>;
    fn from_js(rt: &mut Runtime, v: &Value) -> JsResult<Result<Self, String>>;
    fn wrap(s: TypedSlice<Self>) -> DocValue;
    fn wrapper(_rt: &mut Runtime, _b: &Backing<Self>, _i: usize) -> Option<ObjRef> {
        None
    }
    fn detach(_rt: &mut Runtime, _w: ObjRef) {}
    fn repoint(_rt: &mut Runtime, _w: ObjRef, _b: &Backing<Self>, _i: usize) {}
}

macro_rules! int_elem {
    ($t:ty, $conv:path, $variant:ident) => {
        impl GoElem for $t {
            fn to_js(&self, _rt: &mut Runtime) -> JsResult<Value> {
                Ok(int_to_value(*self as i64))
            }
            fn from_js(rt: &mut Runtime, v: &Value) -> JsResult<Result<Self, String>> {
                Ok(Ok($conv(rt, v)?))
            }
            fn wrap(s: TypedSlice<Self>) -> DocValue {
                DocValue::$variant(s)
            }
        }
    };
}

int_elem!(u8, crate::runtime::to_uint8, Bytes);
int_elem!(i8, crate::runtime::to_int8, Int8s);
int_elem!(u16, crate::runtime::to_uint16, Uint16s);
int_elem!(i16, crate::runtime::to_int16, Int16s);
int_elem!(u32, crate::runtime::to_uint32, Uint32s);
int_elem!(i32, crate::runtime::to_int32, Int32s);
int_elem!(i64, crate::runtime::to_int64, Int64s);

impl GoElem for u64 {
    fn to_js(&self, _rt: &mut Runtime) -> JsResult<Value> {
        if *self <= i64::MAX as u64 {
            return Ok(int_to_value(*self as i64));
        }
        Ok(float_to_value(*self as f64))
    }
    fn from_js(rt: &mut Runtime, v: &Value) -> JsResult<Result<Self, String>> {
        Ok(Ok(crate::runtime::to_uint64(rt, v)?))
    }
    fn wrap(s: TypedSlice<Self>) -> DocValue {
        DocValue::Uint64s(s)
    }
}

impl GoElem for f64 {
    fn to_js(&self, _rt: &mut Runtime) -> JsResult<Value> {
        Ok(float_to_value(*self))
    }
    fn from_js(rt: &mut Runtime, v: &Value) -> JsResult<Result<Self, String>> {
        Ok(Ok(v.to_float(rt)?))
    }
    fn wrap(s: TypedSlice<Self>) -> DocValue {
        DocValue::Float64s(s)
    }
}

impl GoElem for f32 {
    fn to_js(&self, _rt: &mut Runtime) -> JsResult<Value> {
        Ok(float_to_value(f64::from(*self)))
    }
    fn from_js(rt: &mut Runtime, v: &Value) -> JsResult<Result<Self, String>> {
        Ok(Ok(crate::runtime::to_float32(rt, v)?))
    }
    fn wrap(s: TypedSlice<Self>) -> DocValue {
        DocValue::Float32s(s)
    }
}

impl GoElem for Pair {
    fn to_js(&self, rt: &mut Runtime) -> JsResult<Value> {
        let cell = PairCell::Own(Rc::new(RefCell::new(self.clone())));
        Ok(Value::Object(rt.new_go_pair_array(cell)))
    }

    fn from_js(rt: &mut Runtime, v: &Value) -> JsResult<Result<Self, String>> {
        let Value::Object(o) = v else {
            let d = describe_value(rt, v);
            return Ok(Err(format!("could not convert {d} to an array")));
        };
        let o = *o;
        if let Some(p) = rt.ext_dyn::<GoPairArray>(o) {
            return Ok(Ok(p.cell.get()));
        }
        let is_array = o.class_name(rt) == CLASS_ARRAY;
        let len_v = o.get_str(rt, &"length".into(), None)?;
        let Some(len_v) = len_v.filter(|_| o.assert_callable(rt).is_none()) else {
            let d = describe_value(rt, v);
            return Ok(Err(format!("cannot convert {d} to an array")));
        };
        let n = crate::runtime::to_length(rt, &len_v)?;
        if n != 2 {
            let what = if is_array {
                "an Array"
            } else {
                "an array-like object"
            };
            return Ok(Err(format!(
                "cannot convert {what} into an array, lengths mismatch (have {n}, need 2)"
            )));
        }
        let a = o.get_idx(rt, 0, None)?.unwrap_or(Value::Undefined);
        let a = rt.export(&a)?;
        let b = o.get_idx(rt, 1, None)?.unwrap_or(Value::Undefined);
        let b = rt.export(&b)?;
        Ok(Ok([a, b]))
    }

    fn wrap(s: TypedSlice<Self>) -> DocValue {
        DocValue::Pairs(s)
    }

    fn wrapper(rt: &mut Runtime, b: &Backing<Self>, i: usize) -> Option<ObjRef> {
        Some(rt.new_go_pair_array(PairCell::InSlice(b.clone(), i)))
    }

    fn detach(rt: &mut Runtime, w: ObjRef) {
        GoPairArray::detach(rt, w);
    }

    fn repoint(rt: &mut Runtime, w: ObjRef, b: &Backing<Self>, i: usize) {
        GoPairArray::repoint(rt, w, PairCell::InSlice(b.clone(), i));
    }
}

pub struct GoTypedSlice<E: GoElem> {
    pub data: TypedSlice<E>,
    pub length_value: i64,
    pub length_writable: bool,
    pub cache: Vec<Option<ObjRef>>,
}

impl<E: GoElem> ObjData for GoTypedSlice<E> {
    fn trace(&self, t: &mut Tracer) {
        for w in self.cache.iter().flatten() {
            t.obj(*w);
        }
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub fn typed_slice_export(a: &dyn Any) -> Option<DocValue> {
    macro_rules! try_kind {
        ($($t:ty),*) => {
            $(if let Some(s) = a.downcast_ref::<GoTypedSlice<$t>>() {
                return Some(<$t>::wrap(s.data.clone()));
            })*
        };
    }
    try_kind!(Pair, u8, i8, u16, i16, u32, i32, i64, u64, f32, f64);
    None
}

impl Runtime {
    pub fn new_go_typed_slice(&mut self, v: &DocValue) -> ObjRef {
        match v {
            DocValue::Pairs(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Bytes(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Int8s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Uint16s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Int16s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Uint32s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Int32s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Float32s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Float64s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Int64s(s) => GoTypedSlice::alloc(self, s.clone()),
            DocValue::Uint64s(s) => GoTypedSlice::alloc(self, s.clone()),
            _ => self.new_object(),
        }
    }
}

fn prop(v: Value, writable: bool, enumerable: bool) -> Value {
    Value::Property(Box::new(ValueProperty {
        value: v,
        writable,
        enumerable,
        configurable: false,
        accessor: false,
        getter_func: None,
        setter_func: None,
    }))
}

impl<E: GoElem> GoTypedSlice<E> {
    const CLASS: ObjClass = ObjClass {
        sort_len: Self::sort_len,
        sort_get: Self::sort_get,
        swap: Self::swap,
        get_str: Self::get_str,
        get_idx: Self::get_idx,
        get_own_prop_str: Self::get_own_prop_str,
        get_own_prop_idx: Self::get_own_prop_idx,
        set_own_str: Self::set_own_str,
        set_own_idx: Self::set_own_idx,
        set_foreign_str: Self::set_foreign_str,
        set_foreign_idx: Self::set_foreign_idx,
        has_own_property_str: Self::has_own_property_str,
        has_own_property_idx: Self::has_own_property_idx,
        define_own_property_str: Self::define_own_property_str,
        define_own_property_idx: Self::define_own_property_idx,
        delete_str: Self::delete_str,
        delete_idx: Self::delete_idx,
        iterate_string_keys: Self::iterate_string_keys,
        string_keys: Self::string_keys,
        equal: Self::equal,
        ..BASE_CLASS
    };

    pub fn alloc(rt: &mut Runtime, data: TypedSlice<E>) -> ObjRef {
        let proto = rt.get_array_prototype();
        let len = data.len as i64;
        rt.alloc_object(
            &Self::CLASS,
            BaseObject::new(CLASS_ARRAY, Some(proto)),
            ObjExt::Dyn(Box::new(GoTypedSlice {
                data,
                length_value: len,
                length_writable: true,
                cache: Vec::new(),
            })),
        )
    }

    fn this(rt: &Runtime, o: ObjRef) -> &GoTypedSlice<E> {
        match rt.ext_dyn::<GoTypedSlice<E>>(o) {
            Some(s) => s,
            None => unreachable!("not a Go typed slice"),
        }
    }

    fn this_mut(rt: &mut Runtime, o: ObjRef) -> &mut GoTypedSlice<E> {
        match rt.ext_dyn_mut::<GoTypedSlice<E>>(o) {
            Some(s) => s,
            None => unreachable!("not a Go typed slice"),
        }
    }

    fn len(rt: &Runtime, o: ObjRef) -> i64 {
        Self::this(rt, o).data.len as i64
    }

    fn length_prop(rt: &mut Runtime, o: ObjRef) -> Value {
        let l = Self::len(rt, o);
        let s = Self::this_mut(rt, o);
        s.length_value = l;
        prop(int_to_value(l), s.length_writable, false)
    }

    fn cached(rt: &Runtime, o: ObjRef, i: usize) -> Option<ObjRef> {
        Self::this(rt, o).cache.get(i).copied().flatten()
    }

    fn cache_put(rt: &mut Runtime, o: ObjRef, i: usize, w: Option<ObjRef>) {
        let c = &mut Self::this_mut(rt, o).cache;
        if c.len() <= i {
            if w.is_none() {
                return;
            }
            c.resize(i + 1, None);
        }
        c[i] = w;
    }

    fn _get_idx(rt: &mut Runtime, o: ObjRef, idx: usize) -> JsResult<Value> {
        if let Some(w) = Self::cached(rt, o, idx) {
            return Ok(Value::Object(w));
        }
        let b = Self::this(rt, o).data.backing.clone();
        if let Some(w) = E::wrapper(rt, &b, idx) {
            Self::cache_put(rt, o, idx, Some(w));
            return Ok(Value::Object(w));
        }
        let e = b.borrow()[idx].clone();
        e.to_js(rt)
    }

    fn grow(rt: &mut Runtime, o: ObjRef, size: usize) {
        let s = Self::this_mut(rt, o);
        let oldcap = s.data.cap();
        if oldcap < size {
            let newcap = grow_cap(size, s.data.len, oldcap);
            let mut n: Vec<E> = Vec::with_capacity(newcap);
            n.extend_from_slice(&s.data.backing.borrow()[..s.data.len]);
            n.resize_with(newcap, E::default);
            s.data.backing = Rc::new(RefCell::new(n));
            s.data.len = size;
            let b = s.data.backing.clone();
            let l = s.cache.len().min(size);
            let ws: Vec<(usize, ObjRef)> = s.cache[..l]
                .iter()
                .enumerate()
                .filter_map(|(i, w)| w.map(|w| (i, w)))
                .collect();
            for (i, w) in ws {
                E::repoint(rt, w, &b, i);
            }
        } else {
            let start = s.data.len;
            for e in &mut s.data.backing.borrow_mut()[start..size] {
                *e = E::default();
            }
            s.data.len = size;
        }
    }

    fn shrink(rt: &mut Runtime, o: ObjRef, size: usize) {
        let tail: Vec<ObjRef> = {
            let s = Self::this_mut(rt, o);
            if s.cache.len() > size {
                s.cache.drain(size..).flatten().collect()
            } else {
                Vec::new()
            }
        };
        for w in tail {
            E::detach(rt, w);
        }
        let s = Self::this_mut(rt, o);
        let end = s.data.len;
        for e in &mut s.data.backing.borrow_mut()[size..end] {
            *e = E::default();
        }
        s.data.len = size;
    }

    fn put_idx(rt: &mut Runtime, o: ObjRef, idx: usize, v: &Value, throw: bool) -> JsResult<bool> {
        if idx as i64 >= Self::len(rt, o) {
            Self::grow(rt, o, idx + 1);
        }
        let cached = Self::cached(rt, o, idx);
        if let Some(w) = cached {
            E::detach(rt, w);
        }
        let b = Self::this(rt, o).data.backing.clone();
        match E::from_js(rt, v)? {
            Ok(e) => {
                b.borrow_mut()[idx] = e;
                if cached.is_some() {
                    Self::cache_put(rt, o, idx, None);
                }
                Ok(true)
            }
            Err(msg) => {
                if let Some(w) = cached {
                    E::repoint(rt, w, &b, idx);
                }
                rt.type_error_result(throw, &format!("Go type conversion error: {msg}"))?;
                Ok(false)
            }
        }
    }

    fn put_length(rt: &mut Runtime, o: ObjRef, v: u32) -> JsResult<bool> {
        let new_len = v as usize;
        let cur = Self::len(rt, o) as usize;
        if new_len > cur {
            Self::grow(rt, o, new_len);
        } else if new_len < cur {
            Self::shrink(rt, o, new_len);
        }
        Ok(true)
    }

    fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let idx = str_to_go_idx(name);
        let own = if idx >= 0 && idx < Self::len(rt, o) {
            Some(Self::_get_idx(rt, o, idx as usize)?)
        } else if name == "length" {
            let l = Self::len(rt, o);
            Self::this_mut(rt, o).length_value = l;
            Some(int_to_value(l))
        } else {
            BaseObject::get_own_prop_str(rt, o, name)?
        };
        BaseObject::get_str_with_own_prop(rt, o, own, name, receiver)
    }

    fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if idx >= 0 && idx < Self::len(rt, o) {
            return Self::_get_idx(rt, o, idx as usize).map(Some);
        }
        BaseObject::get_str(rt, o, &idx_string(idx), receiver)
    }

    fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx < Self::len(rt, o) {
                let v = Self::_get_idx(rt, o, idx as usize)?;
                return Ok(Some(prop(v, true, true)));
            }
            return Ok(None);
        }
        if name == "length" {
            return Ok(Some(Self::length_prop(rt, o)));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        if idx >= 0 && idx < Self::len(rt, o) {
            let v = Self::_get_idx(rt, o, idx as usize)?;
            return Ok(Some(prop(v, true, true)));
        }
        Ok(None)
    }

    fn fail_set(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let (res, ok) =
            BaseObject::_set_foreign_str(rt, o, name, None, val, &Value::Object(o), throw)?;
        if !ok {
            rt.type_error_result(throw, &format!("Can't set property '{name}' on Go slice"))?;
            return Ok(false);
        }
        Ok(res)
    }

    fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 {
            if idx >= Self::len(rt, o) {
                let (res, ok) = BaseObject::_set_foreign_idx(
                    rt,
                    o,
                    idx,
                    None,
                    val.clone(),
                    &Value::Object(o),
                    throw,
                )?;
                if ok {
                    return Ok(res);
                }
            }
            return Self::put_idx(rt, o, idx as usize, &val, throw);
        }
        Self::fail_set(rt, o, &idx_string(idx), val, throw)
    }

    fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if name == "length" {
            let l = rt.to_length_uint32(&val)?;
            return Self::put_length(rt, o, l);
        }
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx >= Self::len(rt, o) {
                let (res, ok) = BaseObject::_set_foreign_str(
                    rt,
                    o,
                    name,
                    None,
                    val.clone(),
                    &Value::Object(o),
                    throw,
                )?;
                if ok {
                    return Ok(res);
                }
            }
            return Self::put_idx(rt, o, idx as usize, &val, throw);
        }
        Self::fail_set(rt, o, name, val, throw)
    }

    fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let present = idx >= 0 && idx < Self::len(rt, o);
        let p = present.then_some(Value::Bool(true));
        BaseObject::_set_foreign_idx(rt, o, idx, p.as_ref(), val, receiver, throw)
    }

    fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let present = Self::has_own_property_str(rt, o, name)?;
        let p = present.then_some(Value::Bool(true));
        BaseObject::_set_foreign_str(rt, o, name, p.as_ref(), val, receiver, throw)
    }

    fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        Ok(idx >= 0 && idx < Self::len(rt, o))
    }

    fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let idx = str_to_idx64(name);
        if (idx >= 0 && idx < Self::len(rt, o)) || name == "length" {
            return Ok(true);
        }
        BaseObject::has_own_property_str(rt, o, name)
    }

    fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 {
            if !check_host_object_property_descr(rt, &idx_string(idx), &descr, throw)? {
                return Ok(false);
            }
            let val = descr.value.clone().unwrap_or(Value::Undefined);
            return Self::put_idx(rt, o, idx as usize, &val, throw);
        }
        rt.type_error_result(
            throw,
            &format!("Cannot define property '{idx}' on a Go slice"),
        )?;
        Ok(false)
    }

    fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if name == "length" {
            let (old, mut writable) = {
                let s = Self::this(rt, o);
                (s.length_value, s.length_writable)
            };
            let r = define_length(rt, old, &mut writable, &descr, throw, &mut |rt, v| {
                Self::put_length(rt, o, v)
            });
            Self::this_mut(rt, o).length_writable = writable;
            return r;
        }
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if !check_host_object_property_descr(rt, name, &descr, throw)? {
                return Ok(false);
            }
            let val = descr.value.clone().unwrap_or(Value::Undefined);
            return Self::put_idx(rt, o, idx as usize, &val, throw);
        }
        rt.type_error_result(
            throw,
            &format!("Cannot define property '{name}' on a Go slice"),
        )?;
        Ok(false)
    }

    fn _delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64) {
        if idx < Self::len(rt, o) {
            if let Some(w) = Self::cached(rt, o, idx as usize) {
                E::detach(rt, w);
                Self::cache_put(rt, o, idx as usize, None);
            }
            let b = Self::this(rt, o).data.backing.clone();
            b.borrow_mut()[idx as usize] = E::default();
        }
    }

    fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            Self::_delete_idx(rt, o, idx);
            return Ok(true);
        }
        if name == "length" {
            return delete_length(rt, o, name, throw);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, _throw: bool) -> JsResult<bool> {
        if idx >= 0 {
            Self::_delete_idx(rt, o, idx);
        }
        Ok(true)
    }

    fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let limit = Self::len(rt, o);
        Ok(Box::new(IndexIter { o, idx: 0, limit }))
    }

    fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        _all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        for i in 0..Self::len(rt, o) {
            accum.push(Value::String(ascii_string(&i.to_string())));
        }
        Ok(accum)
    }

    fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        match rt.ext_dyn::<GoTypedSlice<E>>(other) {
            Some(s) => s.data.same_data(&Self::this(rt, o).data),
            None => false,
        }
    }

    fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        Ok(Self::len(rt, o))
    }

    fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        Ok(Self::get_idx(rt, o, i, None)?.unwrap_or(Value::Undefined))
    }

    fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        let (i, j) = (i as usize, j as usize);
        let b = Self::this(rt, o).data.backing.clone();
        b.borrow_mut().swap(i, j);
        let ci = Self::cached(rt, o, i);
        let cj = Self::cached(rt, o, j);
        match ci {
            Some(w) => {
                E::repoint(rt, w, &b, j);
                Self::cache_put(rt, o, j, Some(w));
            }
            None => Self::cache_put(rt, o, j, None),
        }
        match cj {
            Some(w) => {
                E::repoint(rt, w, &b, i);
                Self::cache_put(rt, o, i, Some(w));
            }
            None => Self::cache_put(rt, o, i, None),
        }
        Ok(())
    }
}
