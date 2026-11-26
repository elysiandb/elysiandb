use super::goslice::delete_length;
use super::{DocValue, Pair, check_host_object_property_descr};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ARRAY, EnumerableFlag, IterNextFunc, ObjClass, ObjData, ObjExt,
    PropIter, PropIterItem, PropertyDescriptor, UStr, idx_string,
};
use crate::runtime::{JsResult, Runtime, str_to_go_idx};
use crate::string::ascii_string;
use crate::value::{Value, ValueProperty};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

pub const ARRAY_INDEX_PANIC: &str = "array index out of range";

#[derive(Clone)]
pub enum PairCell {
    InSlice(Rc<RefCell<Vec<Pair>>>, usize),
    Own(Rc<RefCell<Pair>>),
}

impl PairCell {
    pub fn get(&self) -> Pair {
        match self {
            PairCell::InSlice(b, i) => b.borrow()[*i].clone(),
            PairCell::Own(p) => p.borrow().clone(),
        }
    }

    pub fn elem(&self, j: usize) -> DocValue {
        match self {
            PairCell::InSlice(b, i) => b.borrow()[*i][j].clone(),
            PairCell::Own(p) => p.borrow()[j].clone(),
        }
    }

    pub fn set_elem(&self, j: usize, v: DocValue) {
        let old = match self {
            PairCell::InSlice(b, i) => std::mem::replace(&mut b.borrow_mut()[*i][j], v),
            PairCell::Own(p) => std::mem::replace(&mut p.borrow_mut()[j], v),
        };
        drop(old);
    }

    pub fn swap(&self, a: usize, b: usize) {
        match self {
            PairCell::InSlice(v, i) => v.borrow_mut()[*i].swap(a, b),
            PairCell::Own(p) => p.borrow_mut().swap(a, b),
        }
    }

    pub fn same(&self, other: &PairCell) -> bool {
        match (self, other) {
            (PairCell::InSlice(a, i), PairCell::InSlice(b, j)) => Rc::ptr_eq(a, b) && i == j,
            (PairCell::Own(a), PairCell::Own(b)) => Rc::ptr_eq(a, b),
            _ => false,
        }
    }
}

pub struct GoPairArray {
    pub cell: PairCell,
}

impl ObjData for GoPairArray {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

impl GoPairArray {
    pub fn export(&self) -> DocValue {
        DocValue::PairRef(self.cell.clone())
    }

    fn cell(rt: &Runtime, o: ObjRef) -> PairCell {
        match rt.ext_dyn::<GoPairArray>(o) {
            Some(p) => p.cell.clone(),
            None => unreachable!("not a host array"),
        }
    }

    pub fn detach(rt: &mut Runtime, o: ObjRef) {
        if let Some(p) = rt.ext_dyn_mut::<GoPairArray>(o) {
            let v = p.cell.get();
            p.cell = PairCell::Own(Rc::new(RefCell::new(v)));
        }
    }

    pub fn repoint(rt: &mut Runtime, o: ObjRef, cell: PairCell) {
        if let Some(p) = rt.ext_dyn_mut::<GoPairArray>(o) {
            p.cell = cell;
        }
    }
}

pub static GO_PAIR_ARRAY_CLASS: ObjClass = ObjClass {
    sort_len: GoPairArray::sort_len,
    sort_get: GoPairArray::sort_get,
    swap: GoPairArray::swap,
    get_str: GoPairArray::get_str,
    get_idx: GoPairArray::get_idx,
    get_own_prop_str: GoPairArray::get_own_prop_str,
    get_own_prop_idx: GoPairArray::get_own_prop_idx,
    set_own_str: GoPairArray::set_own_str,
    set_own_idx: GoPairArray::set_own_idx,
    set_foreign_str: GoPairArray::set_foreign_str,
    set_foreign_idx: GoPairArray::set_foreign_idx,
    has_own_property_str: GoPairArray::has_own_property_str,
    has_own_property_idx: GoPairArray::has_own_property_idx,
    define_own_property_str: GoPairArray::define_own_property_str,
    define_own_property_idx: GoPairArray::define_own_property_idx,
    delete_str: GoPairArray::delete_str,
    delete_idx: GoPairArray::delete_idx,
    iterate_string_keys: GoPairArray::iterate_string_keys,
    string_keys: GoPairArray::string_keys,
    equal: GoPairArray::equal,
    ..BASE_CLASS
};

impl Runtime {
    pub fn new_go_pair_array(&mut self, cell: PairCell) -> ObjRef {
        let proto = self.get_array_prototype();
        self.alloc_object(
            &GO_PAIR_ARRAY_CLASS,
            BaseObject::new(CLASS_ARRAY, Some(proto)),
            ObjExt::Dyn(Box::new(GoPairArray { cell })),
        )
    }
}

fn length_prop() -> Value {
    Value::Property(Box::new(ValueProperty {
        value: Value::Int(2),
        writable: false,
        enumerable: false,
        configurable: false,
        accessor: false,
        getter_func: None,
        setter_func: None,
    }))
}

fn elem_prop(v: Value) -> Value {
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

impl GoPairArray {
    fn _get_idx(rt: &mut Runtime, o: ObjRef, idx: usize) -> JsResult<Value> {
        let v = GoPairArray::cell(rt, o).elem(idx);
        rt.to_value(&v)
    }

    fn _put_idx(rt: &mut Runtime, o: ObjRef, idx: i64, v: &Value) -> JsResult<bool> {
        if !(0..2).contains(&idx) {
            panic!("{}", ARRAY_INDEX_PANIC);
        }
        let cell = GoPairArray::cell(rt, o);
        let dv = rt.export(v)?;
        cell.set_elem(idx as usize, dv);
        Ok(true)
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let idx = str_to_go_idx(name);
        let own = if (0..2).contains(&idx) {
            Some(GoPairArray::_get_idx(rt, o, idx as usize)?)
        } else if name == "length" {
            Some(Value::Int(2))
        } else {
            BaseObject::get_own_prop_str(rt, o, name)?
        };
        BaseObject::get_str_with_own_prop(rt, o, own, name, receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if (0..2).contains(&idx) {
            return GoPairArray::_get_idx(rt, o, idx as usize).map(Some);
        }
        BaseObject::get_str(rt, o, &idx_string(idx), receiver)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx < 2 {
                let v = GoPairArray::_get_idx(rt, o, idx as usize)?;
                return Ok(Some(elem_prop(v)));
            }
            return Ok(None);
        }
        if name == "length" {
            return Ok(Some(length_prop()));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        if (0..2).contains(&idx) {
            let v = GoPairArray::_get_idx(rt, o, idx as usize)?;
            return Ok(Some(elem_prop(v)));
        }
        Ok(None)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        if idx >= 0 {
            if idx >= 2 {
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
            return GoPairArray::_put_idx(rt, o, idx, &val);
        }
        let name = idx_string(idx);
        GoPairArray::set_foreign_or_fail(rt, o, &name, val, throw)
    }

    fn set_foreign_or_fail(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
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

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx >= 2 {
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
            return GoPairArray::_put_idx(rt, o, idx, &val);
        }
        GoPairArray::set_foreign_or_fail(rt, o, name, val, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = (0..2).contains(&idx).then_some(Value::Bool(true));
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
        let present = GoPairArray::has_own_property_str(rt, o, name)?;
        let prop = present.then_some(Value::Bool(true));
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn has_own_property_idx(_rt: &mut Runtime, _o: ObjRef, idx: i64) -> JsResult<bool> {
        Ok((0..2).contains(&idx))
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let idx = crate::runtime::str_to_idx64(name);
        if (0..2).contains(&idx) || name == "length" {
            return Ok(true);
        }
        BaseObject::has_own_property_str(rt, o, name)
    }

    pub fn define_own_property_idx(
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
            return GoPairArray::_put_idx(rt, o, idx, &val);
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
            return GoPairArray::_put_idx(rt, o, idx, &val);
        }
        rt.type_error_result(
            throw,
            &format!("Cannot define property '{name}' on a host array"),
        )?;
        Ok(false)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let idx = str_to_go_idx(name);
        if idx >= 0 {
            if idx < 2 {
                GoPairArray::cell(rt, o).set_elem(idx as usize, DocValue::Nil);
            }
            return Ok(true);
        }
        if name == "length" {
            return delete_length(rt, o, name, throw);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, _throw: bool) -> JsResult<bool> {
        if (0..2).contains(&idx) {
            GoPairArray::cell(rt, o).set_elem(idx as usize, DocValue::Nil);
        }
        Ok(true)
    }

    pub fn iterate_string_keys(_rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        Ok(Box::new(IndexIter {
            o,
            idx: 0,
            limit: 2,
        }))
    }

    pub fn string_keys(
        _rt: &mut Runtime,
        _o: ObjRef,
        _all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        accum.push(Value::String(ascii_string("0")));
        accum.push(Value::String(ascii_string("1")));
        Ok(accum)
    }

    pub fn equal(rt: &Runtime, o: ObjRef, other: ObjRef) -> bool {
        match rt.ext_dyn::<GoPairArray>(other) {
            Some(p) => GoPairArray::cell(rt, o).same(&p.cell),
            None => false,
        }
    }

    pub fn sort_len(_rt: &mut Runtime, _o: ObjRef) -> JsResult<i64> {
        Ok(2)
    }

    pub fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        Ok(GoPairArray::get_idx(rt, o, i, None)?.unwrap_or(Value::Undefined))
    }

    pub fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        GoPairArray::cell(rt, o).swap(i as usize, j as usize);
        Ok(())
    }
}

pub(crate) struct IndexIter {
    pub o: ObjRef,
    pub idx: i64,
    pub limit: i64,
}

impl PropIter for IndexIter {
    fn next(&mut self, _rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.idx < self.limit {
            let name = self.idx.to_string();
            self.idx += 1;
            return Ok(Some(PropIterItem {
                name: Value::String(ascii_string(&name)),
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
