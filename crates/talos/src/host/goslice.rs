use super::{DocValue, SliceHeader, check_host_object_property_descr};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ARRAY, EnumerableFlag, Flag, IterNextFunc, ObjClass, ObjData,
    ObjExt, PropIter, PropIterItem, PropertyDescriptor, UStr, idx_string,
};
use crate::runtime::{JsResult, Runtime, grow_cap, str_to_go_idx, str_to_idx64};
use crate::string::{ascii_string, new_string_value};
use crate::value::{Value, ValueProperty, int_to_value};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

pub enum SliceData {
    Own(SliceHeader),
    Ptr(Rc<RefCell<SliceHeader>>),
}

pub struct GoSlice {
    pub data: SliceData,
    pub length_value: i64,
    pub length_writable: bool,
}

impl GoSlice {
    pub fn orig_is_ptr(&self) -> bool {
        matches!(self.data, SliceData::Ptr(_))
    }
}

impl ObjData for GoSlice {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub static GO_SLICE_CLASS: ObjClass = ObjClass {
    sort_len: GoSlice::sort_len,
    sort_get: GoSlice::sort_get,
    swap: GoSlice::swap,
    get_str: GoSlice::get_str,
    get_idx: GoSlice::get_idx,
    get_own_prop_str: GoSlice::get_own_prop_str,
    get_own_prop_idx: GoSlice::get_own_prop_idx,
    set_own_str: GoSlice::set_own_str,
    set_own_idx: GoSlice::set_own_idx,
    set_foreign_str: GoSlice::set_foreign_str,
    set_foreign_idx: GoSlice::set_foreign_idx,
    has_own_property_str: GoSlice::has_own_property_str,
    has_own_property_idx: GoSlice::has_own_property_idx,
    define_own_property_str: GoSlice::define_own_property_str,
    define_own_property_idx: GoSlice::define_own_property_idx,
    delete_str: GoSlice::delete_str,
    delete_idx: GoSlice::delete_idx,
    iterate_string_keys: GoSlice::iterate_string_keys,
    string_keys: GoSlice::string_keys,
    equal: GoSlice::equal,
    ..BASE_CLASS
};

pub(crate) fn delete_length(
    rt: &mut Runtime,
    o: ObjRef,
    name: &UStr,
    throw: bool,
) -> JsResult<bool> {
    BaseObject::check_delete_prop(rt, o, name, false, throw)
}

impl Runtime {
    pub fn new_go_slice(&mut self, data: SliceData) -> ObjRef {
        let proto = self.get_array_prototype();
        self.alloc_object(
            &GO_SLICE_CLASS,
            BaseObject::new(CLASS_ARRAY, Some(proto)),
            ObjExt::Dyn(Box::new(GoSlice {
                data,
                length_value: 0,
                length_writable: true,
            })),
        )
    }
}

pub(crate) fn define_length(
    rt: &mut Runtime,
    old_len: i64,
    writable: &mut bool,
    descr: &PropertyDescriptor,
    throw: bool,
    setter: &mut dyn FnMut(&mut Runtime, u32) -> JsResult<bool>,
) -> JsResult<bool> {
    let mut new_len = 0u32;
    let mut ret = true;
    'reject: {
        if let Some(v) = &descr.value {
            new_len = rt.to_length_uint32(v)?;
        }
        if descr.configurable == Flag::True
            || descr.enumerable == Flag::True
            || descr.getter.is_some()
            || descr.setter.is_some()
        {
            ret = false;
            break 'reject;
        }
        if descr.value.is_some() {
            if old_len as u32 != new_len {
                ret = setter(rt, new_len)?;
            }
        } else {
            ret = true;
        }
        if descr.writable != Flag::NotSet {
            let w = descr.writable.bool();
            if *writable {
                *writable = w;
            } else if w {
                ret = false;
                break 'reject;
            }
        }
    }
    if !ret {
        rt.type_error_result(throw, "Cannot redefine property: length")?;
    }
    Ok(ret)
}

impl GoSlice {
    fn this(rt: &Runtime, o: ObjRef) -> &GoSlice {
        match rt.ext_dyn::<GoSlice>(o) {
            Some(s) => s,
            None => unreachable!("not a Go slice"),
        }
    }

    fn this_mut(rt: &mut Runtime, o: ObjRef) -> &mut GoSlice {
        match rt.ext_dyn_mut::<GoSlice>(o) {
            Some(s) => s,
            None => unreachable!("not a Go slice"),
        }
    }

    fn head<R>(rt: &Runtime, o: ObjRef, f: impl FnOnce(&SliceHeader) -> R) -> R {
        match &GoSlice::this(rt, o).data {
            SliceData::Own(h) => f(h),
            SliceData::Ptr(p) => f(&p.borrow()),
        }
    }

    fn head_mut<R>(rt: &mut Runtime, o: ObjRef, f: impl FnOnce(&mut SliceHeader) -> R) -> R {
        match &mut GoSlice::this_mut(rt, o).data {
            SliceData::Own(h) => f(h),
            SliceData::Ptr(p) => f(&mut p.borrow_mut()),
        }
    }

    fn backing(rt: &Runtime, o: ObjRef) -> Rc<RefCell<Vec<DocValue>>> {
        GoSlice::head(rt, o, |h| h.backing.clone())
    }

    fn len(rt: &Runtime, o: ObjRef) -> i64 {
        GoSlice::head(rt, o, |h| h.len as i64)
    }

    fn update_len(rt: &mut Runtime, o: ObjRef) -> i64 {
        let s = GoSlice::this_mut(rt, o);
        let l = match &s.data {
            SliceData::Own(h) => h.len,
            SliceData::Ptr(p) => p.borrow().len,
        } as i64;
        s.length_value = l;
        l
    }

    fn length_prop(rt: &Runtime, o: ObjRef) -> Value {
        let s = GoSlice::this(rt, o);
        Value::Property(Box::new(ValueProperty {
            value: int_to_value(s.length_value),
            writable: s.length_writable,
            enumerable: false,
            configurable: false,
            accessor: false,
            getter_func: None,
            setter_func: None,
        }))
    }

    fn _get_idx(rt: &mut Runtime, o: ObjRef, idx: usize) -> JsResult<Value> {
        let b = GoSlice::backing(rt, o);
        let b = b.borrow();
        rt.to_value(&b[idx])
    }

    fn idx_prop(v: Value) -> Value {
        Value::Property(Box::new(ValueProperty {
            value: v,
            writable: true,
            enumerable: true,
            configurable: false,
            accessor: false,
            getter_func: None,
            setter_func: None,
        }))
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let idx = str_to_go_idx(name);
        let own = if idx >= 0 && idx < GoSlice::len(rt, o) {
            Some(GoSlice::_get_idx(rt, o, idx as usize)?)
        } else if name == "length" {
            Some(int_to_value(GoSlice::update_len(rt, o)))
        } else {
            None
        };
        BaseObject::get_str_with_own_prop(rt, o, own, name, receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let dv = GoSlice::head(rt, o, |h| {
            (idx >= 0 && (idx as usize) < h.len).then(|| h.backing.borrow()[idx as usize].clone())
        });
        if let Some(dv) = dv {
            if let Some(x) = crate::export::scalar_value(&dv) {
                return Ok(Some(x));
            }
            return rt.to_value(&dv).map(Some);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            return match receiver {
                None => proto.get_idx(rt, idx, Some(&Value::Object(o))),
                Some(r) => proto.get_idx(rt, idx, Some(r)),
            };
        }
        Ok(None)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx < GoSlice::len(rt, o) {
                let v = GoSlice::_get_idx(rt, o, idx as usize)?;
                return Ok(Some(GoSlice::idx_prop(v)));
            }
            return Ok(None);
        }
        if name == "length" {
            GoSlice::update_len(rt, o);
            return Ok(Some(GoSlice::length_prop(rt, o)));
        }
        Ok(None)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        if idx >= 0 && idx < GoSlice::len(rt, o) {
            let v = GoSlice::_get_idx(rt, o, idx as usize)?;
            return Ok(Some(GoSlice::idx_prop(v)));
        }
        Ok(None)
    }

    fn grow(rt: &mut Runtime, o: ObjRef, size: usize) {
        GoSlice::head_mut(rt, o, |h| GoSlice::grow_header(h, size));
    }

    fn grow_header(h: &mut SliceHeader, size: usize) {
        let oldcap = h.cap();
        if oldcap < size {
            let newcap = grow_cap(size, h.len, oldcap);
            let mut n: Vec<DocValue> = Vec::with_capacity(newcap);
            n.extend_from_slice(&h.backing.borrow()[..h.len]);
            n.resize_with(newcap, DocValue::default);
            h.backing = Rc::new(RefCell::new(n));
            h.len = size;
        } else {
            let start = h.len;
            let removed: Vec<DocValue> = {
                let mut b = h.backing.borrow_mut();
                b[start..size].iter_mut().map(std::mem::take).collect()
            };
            drop(removed);
            h.len = size;
        }
    }

    fn shrink(rt: &mut Runtime, o: ObjRef, size: usize) {
        GoSlice::head_mut(rt, o, |h| GoSlice::shrink_header(h, size));
    }

    fn shrink_header(h: &mut SliceHeader, size: usize) {
        let end = h.len;
        let removed: Vec<DocValue> = {
            let mut b = h.backing.borrow_mut();
            b[size..end].iter_mut().map(std::mem::take).collect()
        };
        drop(removed);
        h.len = size;
    }

    fn put_idx(rt: &mut Runtime, o: ObjRef, idx: usize, v: &Value) -> JsResult<()> {
        if idx as i64 >= GoSlice::len(rt, o) {
            GoSlice::grow(rt, o, idx + 1);
        }
        let backing = GoSlice::backing(rt, o);
        let dv = rt.export(v)?;
        let old = std::mem::replace(&mut backing.borrow_mut()[idx], dv);
        drop(old);
        Ok(())
    }

    fn put_length(rt: &mut Runtime, o: ObjRef, v: u32) -> JsResult<bool> {
        let new_len = v as usize;
        let cur_len = GoSlice::len(rt, o) as usize;
        if new_len > cur_len {
            GoSlice::grow(rt, o, new_len);
        } else if new_len < cur_len {
            GoSlice::shrink(rt, o, new_len);
        }
        Ok(true)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 {
            if idx >= GoSlice::len(rt, o) {
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
            GoSlice::put_idx(rt, o, idx as usize, &val)?;
            return Ok(true);
        }
        let name = idx_string(idx);
        let (res, ok) =
            BaseObject::_set_foreign_str(rt, o, &name, None, val, &Value::Object(o), throw)?;
        if !ok {
            rt.type_error_result(
                throw,
                &format!("Can't set property '{name}' on a host array"),
            )?;
            return Ok(false);
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
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx >= GoSlice::len(rt, o) {
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
            GoSlice::put_idx(rt, o, idx as usize, &val)?;
            return Ok(true);
        }
        if name == "length" {
            let l = rt.to_length_uint32(&val)?;
            return GoSlice::put_length(rt, o, l);
        }
        let (res, ok) =
            BaseObject::_set_foreign_str(rt, o, name, None, val, &Value::Object(o), throw)?;
        if !ok {
            rt.type_error_result(
                throw,
                &format!("Can't set property '{name}' on a host array"),
            )?;
            return Ok(false);
        }
        Ok(res)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let present = GoSlice::has_own_property_idx(rt, o, idx)?;
        let prop = if present {
            Some(Value::Bool(true))
        } else {
            None
        };
        BaseObject::_set_foreign_idx(rt, o, idx, prop.as_ref(), val, receiver, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let present = GoSlice::has_own_property_str(rt, o, name)?;
        let prop = if present {
            Some(Value::Bool(true))
        } else {
            None
        };
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        Ok(idx >= 0 && idx < GoSlice::len(rt, o))
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let idx = str_to_idx64(name);
        if idx >= 0 {
            return Ok(idx < GoSlice::len(rt, o));
        }
        Ok(name == "length")
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 {
            let name = idx_string(idx);
            if !check_host_object_property_descr(rt, &name, &descr, throw)? {
                return Ok(false);
            }
            let val = descr.value.clone().unwrap_or(Value::Undefined);
            GoSlice::put_idx(rt, o, idx as usize, &val)?;
            return Ok(true);
        }
        rt.type_error_result(
            throw,
            &format!("Cannot define property '{idx}' on a host array"),
        )?;
        Ok(false)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if !check_host_object_property_descr(rt, name, &descr, throw)? {
                return Ok(false);
            }
            let val = descr.value.clone().unwrap_or(Value::Undefined);
            GoSlice::put_idx(rt, o, idx as usize, &val)?;
            return Ok(true);
        }
        if name == "length" {
            let (old_len, mut writable) = {
                let s = GoSlice::this(rt, o);
                (s.length_value, s.length_writable)
            };
            let r = define_length(rt, old_len, &mut writable, &descr, throw, &mut |rt, v| {
                GoSlice::put_length(rt, o, v)
            });
            GoSlice::this_mut(rt, o).length_writable = writable;
            return r;
        }
        rt.type_error_result(
            throw,
            &format!("Cannot define property '{name}' on a host array"),
        )?;
        Ok(false)
    }

    fn _delete_idx(rt: &Runtime, o: ObjRef, idx: i64) {
        if idx < GoSlice::len(rt, o) {
            let b = GoSlice::backing(rt, o);
            let old = std::mem::take(&mut b.borrow_mut()[idx as usize]);
            drop(old);
        }
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let idx = str_to_idx64(name);
        if idx >= 0 {
            GoSlice::_delete_idx(rt, o, idx);
            return Ok(true);
        }
        if name == "length" {
            return delete_length(rt, o, name, throw);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, _throw: bool) -> JsResult<bool> {
        if idx >= 0 {
            GoSlice::_delete_idx(rt, o, idx);
        }
        Ok(true)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let limit = GoSlice::len(rt, o);
        Ok(Box::new(GoSlicePropIter { o, idx: 0, limit }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        _all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        for i in 0..GoSlice::len(rt, o) {
            accum.push(Value::String(ascii_string(&i.to_string())));
        }
        Ok(accum)
    }

    pub fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        match (rt.ext_dyn::<GoSlice>(other), &GoSlice::this(rt, o).data) {
            (Some(s), SliceData::Ptr(a)) => match &s.data {
                SliceData::Ptr(b) => Rc::ptr_eq(a, b),
                SliceData::Own(_) => false,
            },
            _ => o == other,
        }
    }

    pub fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        Ok(GoSlice::len(rt, o))
    }

    pub fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        Ok(GoSlice::get_idx(rt, o, i, None)?.unwrap_or(Value::Undefined))
    }

    pub fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        let b = GoSlice::backing(rt, o);
        b.borrow_mut().swap(i as usize, j as usize);
        Ok(())
    }

    pub fn export(rt: &Runtime, o: ObjRef) -> DocValue {
        match &GoSlice::this(rt, o).data {
            SliceData::Ptr(p) => DocValue::SlicePtr(p.clone()),
            SliceData::Own(h) => DocValue::Slice(h.clone()),
        }
    }
}

struct GoSlicePropIter {
    o: ObjRef,
    idx: i64,
    limit: i64,
}

impl PropIter for GoSlicePropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.idx < self.limit && self.idx < GoSlice::len(rt, self.o) {
            let name = self.idx.to_string();
            self.idx += 1;
            return Ok(Some(PropIterItem {
                name: Value::String(new_string_value(name.as_bytes())),
                value: None,
                enumerable: EnumerableFlag::True,
            }));
        }
        Ok(None)
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
    }
}
