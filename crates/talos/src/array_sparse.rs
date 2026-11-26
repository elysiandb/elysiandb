use crate::array::go_append;
use crate::array::{
    ARRAY_OBJECT_CLASS, ArrayObject, LengthOwner, array_base_iterate_string_keys,
    define_array_length, define_idx_own_property, dispatch_set_length,
};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, EnumerableFlag, IterNextFunc, ObjClass, ObjExt, PropIter, PropIterItem,
    PropertyDescriptor, UStr, idx_string, to_idx,
};
use crate::runtime::{JsResult, Runtime, str_to_array_idx, to_int_strict};
use crate::string::ascii_string;
use crate::value::{Value, ValueProperty, int_to_value, property_get, property_set};
use crate::vm::STR_LENGTH;

#[derive(Clone)]
pub struct SparseArrayItem {
    pub idx: u32,
    pub value: Value,
}

pub struct SparseArrayObject {
    pub items: Vec<SparseArrayItem>,
    pub length: u32,
    pub prop_value_count: usize,
    pub length_prop: ValueProperty,
}

impl SparseArrayObject {
    pub fn trace(&self, t: &mut Tracer) {
        for i in &self.items {
            t.value(&i.value);
        }
        t.value(&self.length_prop.value);
    }

    fn data(rt: &Runtime, o: ObjRef) -> &SparseArrayObject {
        match &rt.obj(o).ext {
            ObjExt::SparseArray(a) => a,
            _ => unreachable!("not a sparse array object"),
        }
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut SparseArrayObject {
        match &mut rt.obj_mut(o).ext {
            ObjExt::SparseArray(a) => a,
            _ => unreachable!("not a sparse array object"),
        }
    }

    pub fn find_idx(&self, idx: u32) -> usize {
        self.items.partition_point(|it| it.idx < idx)
    }

    pub fn _set_length_int(rt: &mut Runtime, o: ObjRef, l: u32, throw: bool) -> JsResult<bool> {
        let mut l = l;
        let mut ret = true;
        let a = SparseArrayObject::data_mut(rt, o);
        if l <= a.length && a.prop_value_count > 0 {
            let mut i = a.items.len() as i64 - 1;
            while i >= 0 {
                let item = &a.items[i as usize];
                if item.idx <= l {
                    break;
                }
                if let Value::Property(prop) = &item.value {
                    if !prop.configurable {
                        l = item.idx + 1;
                        ret = false;
                        break;
                    }
                    a.prop_value_count = a.prop_value_count.wrapping_sub(1);
                }
                i -= 1;
            }
        }
        let idx = a.find_idx(l);
        a.items.truncate(idx);
        a.length = l;
        if !ret {
            rt.type_error_result(throw, "Cannot redefine property: length")?;
        }
        Ok(ret)
    }

    pub fn set_length_int(rt: &mut Runtime, o: ObjRef, l: u32, throw: bool) -> JsResult<bool> {
        let a = SparseArrayObject::data(rt, o);
        if l == a.length {
            return Ok(true);
        }
        if !a.length_prop.writable {
            rt.type_error_result(throw, "length is not writable")?;
            return Ok(false);
        }
        SparseArrayObject::_set_length_int(rt, o, l, throw)
    }

    pub fn set_length(rt: &mut Runtime, o: ObjRef, v: u32, throw: bool) -> JsResult<bool> {
        if !SparseArrayObject::data(rt, o).length_prop.writable {
            rt.type_error_result(throw, "length is not writable")?;
            return Ok(false);
        }
        SparseArrayObject::_set_length_int(rt, o, v, throw)
    }

    fn _get_idx(&self, idx: u32) -> Option<&Value> {
        let i = self.find_idx(idx);
        match self.items.get(i) {
            Some(item) if item.idx == idx => Some(&item.value),
            _ => None,
        }
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        if name == "length" {
            let a = SparseArrayObject::data_mut(rt, o);
            let l = int_to_value(i64::from(a.length));
            a.length_prop.value = l.clone();
            return Ok(Some(l));
        }
        let prop = SparseArrayObject::get_own_prop_str(rt, o, name)?;
        BaseObject::get_str_with_own_prop(rt, o, prop, name, receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let prop = SparseArrayObject::get_own_prop_idx(rt, o, idx)?;
        let Some(prop) = prop else {
            if let Some(proto) = rt.obj(o).base.prototype {
                return match receiver {
                    None => proto.get_idx(rt, idx, Some(&Value::Object(o))),
                    Some(r) => proto.get_idx(rt, idx, Some(r)),
                };
            }
            return Ok(None);
        };
        if let Value::Property(p) = &prop {
            let this = receiver.cloned().unwrap_or(Value::Object(o));
            return match p.getter_func {
                None => Ok(Some(if p.value.is_empty() {
                    Value::Undefined
                } else {
                    p.value.clone()
                })),
                Some(g) => property_get(rt, g, &this).map(Some),
            };
        }
        Ok(Some(prop))
    }

    pub fn get_length_prop(rt: &mut Runtime, o: ObjRef) -> Value {
        let a = SparseArrayObject::data_mut(rt, o);
        a.length_prop.value = int_to_value(i64::from(a.length));
        Value::Property(Box::new(a.length_prop.clone()))
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            let a = SparseArrayObject::data(rt, o);
            return Ok(a._get_idx(idx).filter(|v| !v.is_empty()).cloned());
        }
        if name == "length" {
            return Ok(Some(SparseArrayObject::get_length_prop(rt, o)));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        let i = to_idx(idx);
        if i != u32::MAX {
            let a = SparseArrayObject::data(rt, o);
            return Ok(a._get_idx(i).filter(|v| !v.is_empty()).cloned());
        }
        BaseObject::get_own_prop_str(rt, o, &idx_string(idx))
    }

    pub fn add(rt: &mut Runtime, o: ObjRef, idx: u32, val: Value) {
        let a = SparseArrayObject::data_mut(rt, o);
        let i = a.find_idx(idx);
        a.items.insert(i, SparseArrayItem { idx, value: val });
    }

    pub fn _set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: u32,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let a = SparseArrayObject::data_mut(rt, o);
        let i = a.find_idx(idx);
        let mut not_writable = false;
        let mut setter = None;
        let mut found = false;
        if let Some(item) = a.items.get_mut(i)
            && item.idx == idx
            && !item.value.is_empty()
        {
            found = true;
            match &mut item.value {
                Value::Property(prop) => {
                    if !prop.is_writable() {
                        not_writable = true;
                    } else if let Some(s) = prop.setter_func {
                        setter = Some(s);
                    } else {
                        prop.value = val;
                        return Ok(true);
                    }
                }
                slot => {
                    slot.set(val);
                    return Ok(true);
                }
            }
        }
        if found {
            if not_writable {
                rt.type_error_result(throw, "")?;
                return Ok(false);
            }
            if let Some(s) = setter {
                property_set(rt, s, &Value::Object(o), val)?;
            }
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            let (res, ok) =
                proto.set_foreign_idx(rt, i64::from(idx), val.clone(), &Value::Object(o), throw)?;
            if ok {
                return Ok(res);
            }
        }
        if !matches!(rt.obj(o).ext, ObjExt::SparseArray(_)) {
            return Ok(true);
        }
        if !rt.obj(o).base.extensible {
            rt.type_error_result(
                throw,
                &format!("Cannot add property {idx}, object is not extensible"),
            )?;
            return Ok(false);
        }
        if idx >= SparseArrayObject::data(rt, o).length
            && !SparseArrayObject::set_length_int(rt, o, idx + 1, throw)?
        {
            return Ok(false);
        }
        if SparseArrayObject::expand(rt, o, idx) {
            let a = SparseArrayObject::data_mut(rt, o);
            let i = i.min(a.items.len());
            a.items.insert(i, SparseArrayItem { idx, value: val });
        } else {
            let obj = rt.obj_mut(o);
            if let ObjExt::Array(ar) = &mut obj.ext {
                ar.values[idx as usize] = val;
                ar.obj_count = ar.obj_count.wrapping_add(1);
            }
        }
        Ok(true)
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            return SparseArrayObject::_set_own_idx(rt, o, idx, val, throw);
        }
        if name == "length" {
            let l = rt.to_length_uint32(&val)?;
            return dispatch_set_length(rt, o, l, throw);
        }
        BaseObject::set_own_str(rt, o, name, val, throw)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let i = to_idx(idx);
        if i != u32::MAX {
            return SparseArrayObject::_set_own_idx(rt, o, i, val, throw);
        }
        BaseObject::set_own_str(rt, o, &idx_string(idx), val, throw)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = SparseArrayObject::get_own_prop_str(rt, o, name)?;
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = SparseArrayObject::get_own_prop_idx(rt, o, idx)?;
        BaseObject::_set_foreign_idx(rt, o, idx, prop.as_ref(), val, receiver, throw)
    }

    pub fn iterate_string_keys(_rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        Ok(Box::new(SparseArrayPropIter {
            o,
            idx: 0,
            next_array_idx: 0,
            base: None,
        }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        for item in &SparseArrayObject::data(rt, o).items {
            if !all
                && let Value::Property(p) = &item.value
                && !p.enumerable
            {
                continue;
            }
            go_append(
                &mut accum,
                Value::String(ascii_string(&item.idx.to_string())),
            );
        }
        BaseObject::string_keys(rt, o, all, accum)
    }

    pub fn set_values(&mut self, values: &[Value], obj_count: usize) {
        self.items = Vec::with_capacity(obj_count);
        for (i, val) in values.iter().enumerate() {
            if !val.is_empty() {
                self.items.push(SparseArrayItem {
                    idx: i as u32,
                    value: val.clone(),
                });
            }
        }
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            let a = SparseArrayObject::data(rt, o);
            let i = a.find_idx(idx);
            return Ok(i < a.items.len() && a.items[i].idx == idx);
        }
        BaseObject::has_own_property_str(rt, o, name)
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        let i = to_idx(idx);
        if i != u32::MAX {
            let a = SparseArrayObject::data(rt, o);
            let j = a.find_idx(i);
            return Ok(j < a.items.len() && a.items[j].idx == i);
        }
        BaseObject::has_own_property_str(rt, o, &idx_string(idx))
    }

    pub fn has_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        if SparseArrayObject::has_own_property_idx(rt, o, idx)? {
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            return proto.has_property_idx(rt, idx);
        }
        Ok(false)
    }

    pub fn expand(rt: &mut Runtime, o: ObjRef, idx: u32) -> bool {
        let a = SparseArrayObject::data(rt, o);
        let l = a.items.len();
        if l >= 1024 {
            let mut idx = idx;
            let ii = a.items[l - 1].idx;
            if ii > idx {
                idx = ii;
            }
            if (idx as usize) >> 3 < l {
                SparseArrayObject::switch_to_dense(rt, o, idx as usize);
                return false;
            }
        }
        true
    }

    fn switch_to_dense(rt: &mut Runtime, o: ObjRef, new_max_idx: usize) {
        let obj = rt.obj_mut(o);
        let ObjExt::SparseArray(sa) = std::mem::take(&mut obj.ext) else {
            unreachable!("not a sparse array object");
        };
        let mut ar = ArrayObject {
            values: Vec::new(),
            length: sa.length,
            obj_count: 0,
            prop_value_count: sa.prop_value_count,
            length_prop: ValueProperty {
                writable: sa.length_prop.writable,
                ..ValueProperty::default()
            },
        };
        ar.set_values_from_sparse(&sa.items, new_max_idx);
        obj.base._put(
            &STR_LENGTH,
            ArrayObject::length_placeholder(ar.length_prop.writable),
        );
        obj.ext = ObjExt::Array(Box::new(ar));
        obj.class = &ARRAY_OBJECT_CLASS;
    }

    pub fn _define_idx_property(
        rt: &mut Runtime,
        o: ObjRef,
        idx: u32,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let a = SparseArrayObject::data(rt, o);
        let i = a.find_idx(idx);
        let existing = match a.items.get(i) {
            Some(item) if item.idx == idx => item.value.clone(),
            _ => Value::Empty,
        };
        let Some(prop) = define_idx_own_property(rt, o, idx, &existing, &desc, throw)? else {
            return Ok(false);
        };
        if idx >= SparseArrayObject::data(rt, o).length
            && !SparseArrayObject::set_length_int(rt, o, idx + 1, throw)?
        {
            return Ok(false);
        }
        let is_prop = matches!(prop, Value::Property(_));
        let a = SparseArrayObject::data(rt, o);
        let present = i < a.items.len() && a.items[i].idx == idx;
        if !present {
            if SparseArrayObject::expand(rt, o, idx) {
                let a = SparseArrayObject::data_mut(rt, o);
                let i = i.min(a.items.len());
                a.items.insert(i, SparseArrayItem { idx, value: prop });
                if idx >= a.length {
                    a.length = idx + 1;
                }
            } else {
                if let ObjExt::Array(ar) = &mut rt.obj_mut(o).ext {
                    ar.values[idx as usize] = prop;
                }
                return Ok(true);
            }
        } else {
            SparseArrayObject::data_mut(rt, o).items[i].value = prop;
        }
        if is_prop {
            let a = SparseArrayObject::data_mut(rt, o);
            a.prop_value_count = a.prop_value_count.wrapping_add(1);
        }
        Ok(true)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            return SparseArrayObject::_define_idx_property(rt, o, idx, descr, throw);
        }
        if name == "length" {
            SparseArrayObject::get_length_prop(rt, o);
            return define_array_length(rt, o, LengthOwner::Sparse, &descr, throw);
        }
        BaseObject::define_own_property_str(rt, o, name, descr, throw)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        descr: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let i = to_idx(idx);
        if i != u32::MAX {
            return SparseArrayObject::_define_idx_property(rt, o, i, descr, throw);
        }
        BaseObject::define_own_property_str(rt, o, &idx_string(idx), descr, throw)
    }

    pub fn _delete_idx_prop(rt: &mut Runtime, o: ObjRef, idx: u32, throw: bool) -> JsResult<bool> {
        let a = SparseArrayObject::data(rt, o);
        let i = a.find_idx(idx);
        if i < a.items.len() && a.items[i].idx == idx {
            if let Value::Property(p) = &a.items[i].value
                && !p.configurable
            {
                let s = Value::Object(o).to_string(rt)?.to_rust_string();
                rt.type_error_result(throw, &format!("Cannot delete property '{idx}' of {s}"))?;
                return Ok(false);
            }
            let Some(a) = (match &mut rt.obj_mut(o).ext {
                ObjExt::SparseArray(a) => Some(a),
                _ => None,
            }) else {
                return Ok(true);
            };
            if i < a.items.len() && a.items[i].idx == idx {
                if matches!(a.items[i].value, Value::Property(_)) {
                    a.prop_value_count = a.prop_value_count.wrapping_sub(1);
                }
                a.items.remove(i);
            }
        }
        Ok(true)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            return SparseArrayObject::_delete_idx_prop(rt, o, idx, throw);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        let i = to_idx(idx);
        if i != u32::MAX {
            return SparseArrayObject::_delete_idx_prop(rt, o, i, throw);
        }
        BaseObject::delete_str(rt, o, &idx_string(idx), throw)
    }

    pub fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        let a = SparseArrayObject::data(rt, o);
        match a.items.last() {
            Some(item) => Ok(to_int_strict(i64::from(item.idx) + 1)),
            None => Ok(0),
        }
    }
}

pub struct SparseArrayPropIter {
    o: ObjRef,
    idx: usize,
    next_array_idx: usize,
    base: Option<IterNextFunc>,
}

impl PropIter for SparseArrayPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.base.is_none() {
            match &rt.obj(self.o).ext {
                ObjExt::SparseArray(a) => {
                    while self.idx < a.items.len() {
                        let item = &a.items[self.idx];
                        self.idx += 1;
                        if !item.value.is_empty() {
                            self.next_array_idx = item.idx as usize + 1;
                            return Ok(Some(PropIterItem {
                                name: Value::String(ascii_string(&item.idx.to_string())),
                                value: Some(item.value.clone()),
                                enumerable: EnumerableFlag::Unknown,
                            }));
                        }
                    }
                }
                ObjExt::Array(a) => {
                    while self.next_array_idx < a.values.len() {
                        let i = self.next_array_idx;
                        self.next_array_idx += 1;
                        let prop = &a.values[i];
                        if !prop.is_empty() {
                            return Ok(Some(PropIterItem {
                                name: Value::String(ascii_string(&i.to_string())),
                                value: Some(prop.clone()),
                                enumerable: EnumerableFlag::Unknown,
                            }));
                        }
                    }
                }
                _ => {}
            }
            self.base = Some(array_base_iterate_string_keys(rt, self.o)?);
        }
        match &mut self.base {
            Some(b) => b.next(rt),
            None => Ok(None),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        if let Some(b) = &self.base {
            b.trace(t);
        }
    }
}

pub static SPARSE_ARRAY_OBJECT_CLASS: ObjClass = ObjClass {
    sort_len: SparseArrayObject::sort_len,
    get_str: SparseArrayObject::get_str,
    get_idx: SparseArrayObject::get_idx,
    get_own_prop_str: SparseArrayObject::get_own_prop_str,
    get_own_prop_idx: SparseArrayObject::get_own_prop_idx,
    set_own_str: SparseArrayObject::set_own_str,
    set_own_idx: SparseArrayObject::set_own_idx,
    set_foreign_str: SparseArrayObject::set_foreign_str,
    set_foreign_idx: SparseArrayObject::set_foreign_idx,
    has_property_idx: SparseArrayObject::has_property_idx,
    has_own_property_str: SparseArrayObject::has_own_property_str,
    has_own_property_idx: SparseArrayObject::has_own_property_idx,
    define_own_property_str: SparseArrayObject::define_own_property_str,
    define_own_property_idx: SparseArrayObject::define_own_property_idx,
    delete_str: SparseArrayObject::delete_str,
    delete_idx: SparseArrayObject::delete_idx,
    iterate_string_keys: SparseArrayObject::iterate_string_keys,
    string_keys: SparseArrayObject::string_keys,
    ..BASE_CLASS
};
