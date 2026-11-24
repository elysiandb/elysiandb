use crate::array_sparse::{SPARSE_ARRAY_OBJECT_CLASS, SparseArrayItem, SparseArrayObject};
use crate::heap::{ObjRef, Tracer};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_ARRAY, CLASS_OBJECT, EnumerableFlag, Flag, IterNextFunc,
    ObjClass, ObjData, ObjExt, PropIter, PropIterItem, PropertyDescriptor, UStr, idx_string,
    to_idx,
};
use crate::object_template::{TemplatedArrayObject, TemplatedObject};
use crate::runtime::{JsResult, Runtime, grow_cap, str_to_array_idx, to_length};
use crate::string::ascii_string;
use crate::value::{Value, ValueProperty, int_to_value, nil_safe, property_get, property_set};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IterationKind {
    Key,
    Value,
    KeyValue,
}

pub struct ArrayIterObject {
    pub obj: Option<ObjRef>,
    pub next_idx: i64,
    pub kind: IterationKind,
}

impl ObjData for ArrayIterObject {
    fn trace(&self, t: &mut Tracer) {
        t.opt_obj(self.obj);
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

pub static ARRAY_ITER_OBJECT_CLASS: ObjClass = BASE_CLASS;

impl ArrayIterObject {
    pub fn next(rt: &mut Runtime, o: ObjRef) -> JsResult<Value> {
        let Some(obj) = rt.ext_dyn::<ArrayIterObject>(o).and_then(|ai| ai.obj) else {
            return Ok(rt.create_iter_result_object(Value::Undefined, true));
        };
        let lv = nil_safe(obj.get_str(rt, &crate::vm::STR_LENGTH, None)?);
        let l = to_length(rt, &lv)?;
        let (index, kind) = match rt.ext_dyn::<ArrayIterObject>(o) {
            Some(ai) => (ai.next_idx, ai.kind),
            None => unreachable!("not an array iterator"),
        };
        if index >= l {
            if let Some(ai) = rt.ext_dyn_mut::<ArrayIterObject>(o) {
                ai.obj = None;
            }
            return Ok(rt.create_iter_result_object(Value::Undefined, true));
        }
        if let Some(ai) = rt.ext_dyn_mut::<ArrayIterObject>(o) {
            ai.next_idx += 1;
        }
        let idx_val = Value::Int(index);
        if kind == IterationKind::Key {
            return Ok(rt.create_iter_result_object(idx_val, false));
        }
        let element_value = nil_safe(obj.get_idx(rt, index, None)?);
        let result = if kind == IterationKind::Value {
            element_value
        } else {
            Value::Object(rt.new_array_values(vec![idx_val, element_value]))
        };
        Ok(rt.create_iter_result_object(result, false))
    }
}

static EMPTY: Value = Value::Empty;

pub struct ArrayObject {
    pub values: Vec<Value>,
    pub length: u32,
    pub obj_count: usize,
    pub prop_value_count: usize,
    pub length_prop: ValueProperty,
}

impl ArrayObject {
    pub fn trace(&self, t: &mut Tracer) {
        t.values(&self.values);
        t.value(&self.length_prop.value);
    }

    pub fn length_placeholder(writable: bool) -> Value {
        Value::Property(Box::new(ValueProperty {
            value: Value::Int(0),
            writable,
            ..ValueProperty::default()
        }))
    }

    fn data(rt: &Runtime, o: ObjRef) -> &ArrayObject {
        match &rt.obj(o).ext {
            ObjExt::Array(a) => a,
            _ => unreachable!("not an array object"),
        }
    }

    fn data_mut(rt: &mut Runtime, o: ObjRef) -> &mut ArrayObject {
        match &mut rt.obj_mut(o).ext {
            ObjExt::Array(a) => a,
            _ => unreachable!("not an array object"),
        }
    }

    fn try_data_mut(rt: &mut Runtime, o: ObjRef) -> Option<&mut ArrayObject> {
        match &mut rt.obj_mut(o).ext {
            ObjExt::Array(a) => Some(a),
            _ => None,
        }
    }

    pub fn is_array_object(rt: &Runtime, o: ObjRef) -> bool {
        matches!(rt.obj(o).ext, ObjExt::Array(_))
    }

    pub fn _set_length_int(rt: &mut Runtime, o: ObjRef, l: u32, throw: bool) -> JsResult<bool> {
        let mut l = l;
        let mut ret = true;
        let a = ArrayObject::data_mut(rt, o);
        if l <= a.length && a.prop_value_count > 0 {
            let mut i = a.values.len() as i64 - 1;
            while i >= i64::from(l) {
                if let Value::Property(prop) = &a.values[i as usize] {
                    if !prop.configurable {
                        l = i as u32 + 1;
                        ret = false;
                        break;
                    }
                    a.prop_value_count = a.prop_value_count.wrapping_sub(1);
                }
                i -= 1;
            }
        }
        let lu = l as usize;
        if lu <= a.values.len() {
            if l >= 16 && l < (a.values.capacity() as u32) >> 2 {
                a.values.truncate(lu);
                a.values.shrink_to(lu);
            } else {
                a.values.truncate(lu);
            }
        }
        a.length = l;
        if !ret {
            rt.type_error_result(throw, "Cannot redefine property: length")?;
        }
        Ok(ret)
    }

    pub fn set_length_int(rt: &mut Runtime, o: ObjRef, l: u32, throw: bool) -> JsResult<bool> {
        let a = ArrayObject::data(rt, o);
        if l == a.length {
            return Ok(true);
        }
        if !a.length_prop.writable {
            rt.type_error_result(throw, "length is not writable")?;
            return Ok(false);
        }
        ArrayObject::_set_length_int(rt, o, l, throw)
    }

    pub fn set_length(rt: &mut Runtime, o: ObjRef, v: u32, throw: bool) -> JsResult<bool> {
        if !ArrayObject::data(rt, o).length_prop.writable {
            rt.type_error_result(throw, "length is not writable")?;
            return Ok(false);
        }
        ArrayObject::_set_length_int(rt, o, v, throw)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let i = to_idx(idx);
        if i != u32::MAX {
            let obj = rt.obj(o);
            let a = match &obj.ext {
                ObjExt::Array(a) => a,
                _ => unreachable!("not an array object"),
            };
            let prop = a.values.get(i as usize).unwrap_or(&EMPTY);
            match prop {
                Value::Empty => {}
                Value::Property(p) => {
                    return match p.getter_func {
                        None => Ok(Some(if p.value.is_empty() {
                            Value::Undefined
                        } else {
                            p.value.clone()
                        })),
                        Some(g) => {
                            let this = receiver.cloned().unwrap_or(Value::Object(o));
                            property_get(rt, g, &this).map(Some)
                        }
                    };
                }
                v => return Ok(Some(v.clone())),
            }
            return match obj.base.prototype {
                Some(proto) => match receiver {
                    None => proto.get_idx(rt, idx, Some(&Value::Object(o))),
                    Some(r) => proto.get_idx(rt, idx, Some(r)),
                },
                None => Ok(None),
            };
        }
        let prop = BaseObject::get_own_prop_str(rt, o, &idx_string(idx))?;
        ArrayObject::get_idx_with_own_prop(rt, o, prop, idx, receiver)
    }

    fn get_idx_with_own_prop(
        rt: &mut Runtime,
        o: ObjRef,
        prop: Option<Value>,
        idx: i64,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let Some(prop) = prop.filter(|p| !p.is_empty()) else {
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
            return p.get(rt, &this).map(Some);
        }
        Ok(Some(prop))
    }

    pub fn get_length_prop(rt: &mut Runtime, o: ObjRef) -> Value {
        let a = ArrayObject::data_mut(rt, o);
        a.length_prop.value = int_to_value(i64::from(a.length));
        Value::Property(Box::new(a.length_prop.clone()))
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let a = ArrayObject::data(rt, o);
        if !a.values.is_empty() {
            let i = str_to_array_idx(name);
            if i != u32::MAX && (i as usize) < a.values.len() {
                let v = &a.values[i as usize];
                return Ok(if v.is_empty() { None } else { Some(v.clone()) });
            }
        }
        if name == "length" {
            return Ok(Some(ArrayObject::get_length_prop(rt, o)));
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        let i = to_idx(idx);
        if i != u32::MAX {
            let a = ArrayObject::data(rt, o);
            return Ok(match a.values.get(i as usize) {
                Some(v) if !v.is_empty() => Some(v.clone()),
                _ => None,
            });
        }
        BaseObject::get_own_prop_str(rt, o, &idx_string(idx))
    }

    pub fn sort_len(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
        Ok(ArrayObject::data(rt, o).values.len() as i64)
    }

    pub fn sort_get(rt: &mut Runtime, o: ObjRef, i: i64) -> JsResult<Value> {
        let v = &ArrayObject::data(rt, o).values[i as usize];
        if let Value::Property(p) = v {
            return match p.getter_func {
                None => Ok(if p.value.is_empty() {
                    Value::Undefined
                } else {
                    p.value.clone()
                }),
                Some(g) => property_get(rt, g, &Value::Object(o)),
            };
        }
        Ok(v.clone())
    }

    pub fn swap(rt: &mut Runtime, o: ObjRef, i: i64, j: i64) -> JsResult<()> {
        ArrayObject::data_mut(rt, o)
            .values
            .swap(i as usize, j as usize);
        Ok(())
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let a = ArrayObject::data_mut(rt, o);
        if !a.values.is_empty() {
            let i = str_to_array_idx(name);
            if i != u32::MAX && (i as usize) < a.values.len() {
                let prop = a.values[i as usize].clone();
                return BaseObject::get_str_with_own_prop(rt, o, Some(prop), name, receiver);
            }
        }
        if name == "length" {
            let l = int_to_value(i64::from(a.length));
            a.length_prop.value = l.clone();
            return Ok(Some(l));
        }
        BaseObject::get_str(rt, o, name, receiver)
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
            return ArrayObject::_set_own_idx(rt, o, i, val, throw);
        }
        BaseObject::set_own_str(rt, o, &idx_string(idx), val, throw)
    }

    pub fn _set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: u32,
        val: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let iu = idx as usize;
        let mut not_writable = false;
        let mut setter = None;
        let a = ArrayObject::data_mut(rt, o);
        if iu < a.values.len() {
            match &mut a.values[iu] {
                Value::Empty => {}
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
        if not_writable {
            rt.type_error_result(throw, "")?;
            return Ok(false);
        }
        if let Some(s) = setter {
            property_set(rt, s, &Value::Object(o), val)?;
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            let (res, ok) =
                proto.set_foreign_idx(rt, i64::from(idx), val.clone(), &Value::Object(o), throw)?;
            if ok {
                return Ok(res);
            }
        }
        if !ArrayObject::is_array_object(rt, o) {
            return Ok(true);
        }
        if !rt.obj(o).base.extensible {
            rt.type_error_result(
                throw,
                &format!("Cannot add property {idx}, object is not extensible"),
            )?;
            return Ok(false);
        }
        if idx >= ArrayObject::data(rt, o).length
            && !ArrayObject::set_length_int(rt, o, idx + 1, throw)?
        {
            return Ok(false);
        }
        if iu >= ArrayObject::data(rt, o).values.len() && !ArrayObject::expand(rt, o, idx) {
            SparseArrayObject::add(rt, o, idx, val);
            return Ok(true);
        }
        let a = ArrayObject::data_mut(rt, o);
        a.obj_count = a.obj_count.wrapping_add(1);
        a.values[iu].set(val);
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
            return ArrayObject::_set_own_idx(rt, o, idx, val, throw);
        }
        if name == "length" {
            let l = rt.to_length_uint32(&val)?;
            return dispatch_set_length(rt, o, l, throw);
        }
        BaseObject::set_own_str(rt, o, name, val, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        val: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = ArrayObject::get_own_prop_idx(rt, o, idx)?;
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
        let prop = ArrayObject::get_own_prop_str(rt, o, name)?;
        BaseObject::_set_foreign_str(rt, o, name, prop.as_ref(), val, receiver, throw)
    }

    pub fn iterate_string_keys(rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        let limit = ArrayObject::data(rt, o).values.len();
        Ok(Box::new(ArrayPropIter {
            o,
            limit,
            idx: 0,
            base: None,
        }))
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        for (i, prop) in ArrayObject::data(rt, o).values.iter().enumerate() {
            if prop.is_empty() {
                continue;
            }
            if !all
                && let Value::Property(p) = prop
                && !p.enumerable
            {
                continue;
            }
            accum.push(Value::String(ascii_string(&i.to_string())));
        }
        BaseObject::string_keys(rt, o, all, accum)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            let a = ArrayObject::data(rt, o);
            return Ok(a.values.get(idx as usize).is_some_and(|v| !v.is_empty()));
        }
        BaseObject::has_own_property_str(rt, o, name)
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        let i = to_idx(idx);
        if i != u32::MAX {
            let a = ArrayObject::data(rt, o);
            return Ok(a.values.get(i as usize).is_some_and(|v| !v.is_empty()));
        }
        BaseObject::has_own_property_str(rt, o, &idx_string(idx))
    }

    pub fn has_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        if ArrayObject::has_own_property_idx(rt, o, idx)? {
            return Ok(true);
        }
        if let Some(proto) = rt.obj(o).base.prototype {
            return proto.has_property_idx(rt, idx);
        }
        Ok(false)
    }

    pub fn expand(rt: &mut Runtime, o: ObjRef, idx: u32) -> bool {
        let target_len = idx + 1;
        let tl = target_len as usize;
        let a = ArrayObject::data_mut(rt, o);
        if tl > a.values.len() {
            if target_len < a.values.capacity() as u32 {
                a.values.resize(tl, Value::Empty);
            } else if idx > 4096 && (a.obj_count == 0 || idx / (a.obj_count as u32) > 10) {
                ArrayObject::switch_to_sparse(rt, o);
                return false;
            } else {
                let new_cap = grow_cap(tl, a.values.len(), a.values.capacity());
                a.values.reserve_exact(new_cap - a.values.len());
                a.values.resize(tl, Value::Empty);
            }
        }
        true
    }

    fn switch_to_sparse(rt: &mut Runtime, o: ObjRef) {
        let obj = rt.obj_mut(o);
        let ObjExt::Array(a) = std::mem::take(&mut obj.ext) else {
            unreachable!("not an array object");
        };
        let mut sa = SparseArrayObject {
            items: Vec::new(),
            length: a.length,
            prop_value_count: a.prop_value_count,
            length_prop: ValueProperty {
                writable: a.length_prop.writable,
                ..ValueProperty::default()
            },
        };
        sa.set_values(&a.values, a.obj_count.wrapping_add(1));
        obj.base._put(
            &UStr::from("length"),
            ArrayObject::length_placeholder(sa.length_prop.writable),
        );
        obj.ext = ObjExt::SparseArray(Box::new(sa));
        obj.class = &SPARSE_ARRAY_OBJECT_CLASS;
    }

    pub fn _define_idx_property(
        rt: &mut Runtime,
        o: ObjRef,
        idx: u32,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let existing = ArrayObject::data(rt, o)
            .values
            .get(idx as usize)
            .cloned()
            .unwrap_or(Value::Empty);
        let name = UStr::from_bytes(idx.to_string().as_bytes());
        let Some(prop) =
            BaseObject::_define_own_property(rt, o, &name, Some(&existing), &desc, throw)?
        else {
            return Ok(false);
        };
        if idx >= ArrayObject::data(rt, o).length
            && !ArrayObject::set_length_int(rt, o, idx + 1, throw)?
        {
            return Ok(false);
        }
        if ArrayObject::expand(rt, o, idx) {
            let a = ArrayObject::data_mut(rt, o);
            let is_prop = matches!(prop, Value::Property(_));
            a.values[idx as usize] = prop;
            a.obj_count = a.obj_count.wrapping_add(1);
            if is_prop {
                a.prop_value_count = a.prop_value_count.wrapping_add(1);
            }
        } else {
            SparseArrayObject::add(rt, o, idx, prop);
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
            return ArrayObject::_define_idx_property(rt, o, idx, descr, throw);
        }
        if name == "length" {
            ArrayObject::get_length_prop(rt, o);
            return define_array_length(rt, o, LengthOwner::Array, &descr, throw);
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
            return ArrayObject::_define_idx_property(rt, o, i, descr, throw);
        }
        BaseObject::define_own_property_str(rt, o, &idx_string(idx), descr, throw)
    }

    pub fn _delete_idx_prop(rt: &mut Runtime, o: ObjRef, idx: u32, throw: bool) -> JsResult<bool> {
        let iu = idx as usize;
        let non_configurable = match ArrayObject::data(rt, o).values.get(iu) {
            None | Some(Value::Empty) => return Ok(true),
            Some(Value::Property(p)) => !p.configurable,
            Some(_) => false,
        };
        if non_configurable {
            let s = Value::Object(o).to_string(rt)?.to_rust_string();
            rt.type_error_result(throw, &format!("Cannot delete property '{idx}' of {s}"))?;
            return Ok(false);
        }
        let Some(a) = ArrayObject::try_data_mut(rt, o) else {
            return Ok(true);
        };
        if let Some(v) = a.values.get_mut(iu) {
            if v.is_empty() {
                return Ok(true);
            }
            if matches!(v, Value::Property(_)) {
                a.prop_value_count = a.prop_value_count.wrapping_sub(1);
            }
            *v = Value::Empty;
            a.obj_count = a.obj_count.wrapping_sub(1);
        }
        Ok(true)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let idx = str_to_array_idx(name);
        if idx != u32::MAX {
            return ArrayObject::_delete_idx_prop(rt, o, idx, throw);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        let i = to_idx(idx);
        if i != u32::MAX {
            return ArrayObject::_delete_idx_prop(rt, o, i, throw);
        }
        BaseObject::delete_str(rt, o, &idx_string(idx), throw)
    }

    pub fn set_values_from_sparse(&mut self, items: &[SparseArrayItem], new_max_idx: usize) {
        let mut values = vec![Value::Empty; new_max_idx + 1];
        for item in items {
            values[item.idx as usize] = item.value.clone();
        }
        self.values = values;
        self.obj_count = items.len();
    }
}

pub struct ArrayPropIter {
    o: ObjRef,
    limit: usize,
    idx: usize,
    base: Option<IterNextFunc>,
}

impl PropIter for ArrayPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.base.is_none() {
            match &rt.obj(self.o).ext {
                ObjExt::Array(a) => {
                    while self.idx < a.values.len() && self.idx < self.limit {
                        let i = self.idx;
                        self.idx += 1;
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
                ObjExt::SparseArray(sa) if self.idx < self.limit => {
                    let i = sa.find_idx(self.idx as u32);
                    if let Some(item) = sa.items.get(i)
                        && (item.idx as usize) < self.limit
                    {
                        self.idx = item.idx as usize + 1;
                        return Ok(Some(PropIterItem {
                            name: Value::String(ascii_string(&item.idx.to_string())),
                            value: Some(item.value.clone()),
                            enumerable: EnumerableFlag::Unknown,
                        }));
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

pub struct ArrayBasePropIter {
    o: ObjRef,
    inner: IterNextFunc,
}

impl PropIter for ArrayBasePropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        let Some(mut item) = self.inner.next(rt)? else {
            return Ok(None);
        };
        if let Value::String(s) = &item.name
            && s.length() == 6
            && s.string() == "length"
            && let Some(lp) = current_length_prop(rt, self.o)
        {
            item.value = Some(lp);
        }
        Ok(Some(item))
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.o);
        self.inner.trace(t);
    }
}

pub(crate) fn array_base_iterate_string_keys(
    rt: &mut Runtime,
    o: ObjRef,
) -> JsResult<IterNextFunc> {
    let inner = BaseObject::iterate_string_keys(rt, o)?;
    Ok(Box::new(ArrayBasePropIter { o, inner }))
}

fn current_length_prop(rt: &Runtime, o: ObjRef) -> Option<Value> {
    match &rt.obj(o).ext {
        ObjExt::Array(a) => Some(Value::Property(Box::new(a.length_prop.clone()))),
        ObjExt::SparseArray(a) => Some(Value::Property(Box::new(a.length_prop.clone()))),
        _ => None,
    }
}

pub(crate) fn dispatch_set_length(
    rt: &mut Runtime,
    o: ObjRef,
    l: u32,
    throw: bool,
) -> JsResult<bool> {
    match &rt.obj(o).ext {
        ObjExt::Array(_) => ArrayObject::set_length(rt, o, l, throw),
        ObjExt::SparseArray(_) => SparseArrayObject::set_length(rt, o, l, throw),
        _ => Ok(true),
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum LengthOwner {
    Array,
    Sparse,
    Templated,
}

impl LengthOwner {
    fn is_current(self, rt: &Runtime, o: ObjRef) -> bool {
        match self {
            LengthOwner::Array => matches!(rt.obj(o).ext, ObjExt::Array(_)),
            LengthOwner::Sparse => matches!(rt.obj(o).ext, ObjExt::SparseArray(_)),
            LengthOwner::Templated => true,
        }
    }

    fn prop_value(self, rt: &Runtime, o: ObjRef) -> Value {
        match (self, &rt.obj(o).ext) {
            (LengthOwner::Array, ObjExt::Array(a)) => a.length_prop.value.clone(),
            (LengthOwner::Sparse, ObjExt::SparseArray(a)) => a.length_prop.value.clone(),
            (LengthOwner::Templated, _) => match rt.obj(o).base.values.get(&UStr::from("length")) {
                Some(Value::Property(p)) => p.value.clone(),
                _ => Value::Undefined,
            },
            _ => Value::Undefined,
        }
    }

    fn writable(self, rt: &Runtime, o: ObjRef) -> bool {
        match (self, &rt.obj(o).ext) {
            (LengthOwner::Array, ObjExt::Array(a)) => a.length_prop.writable,
            (LengthOwner::Sparse, ObjExt::SparseArray(a)) => a.length_prop.writable,
            (LengthOwner::Templated, _) => match rt.obj(o).base.values.get(&UStr::from("length")) {
                Some(Value::Property(p)) => p.writable,
                _ => false,
            },
            _ => false,
        }
    }

    fn set_writable(self, rt: &mut Runtime, o: ObjRef, w: bool) {
        let obj = rt.obj_mut(o);
        match (self, &mut obj.ext) {
            (LengthOwner::Array, ObjExt::Array(a)) => a.length_prop.writable = w,
            (LengthOwner::Sparse, ObjExt::SparseArray(a)) => a.length_prop.writable = w,
            (LengthOwner::Templated, _) => {
                if let Some(Value::Property(p)) = obj.base.values.get_mut(&UStr::from("length")) {
                    p.writable = w;
                }
            }
            _ => {}
        }
    }

    fn set_length(self, rt: &mut Runtime, o: ObjRef, l: u32, throw: bool) -> JsResult<bool> {
        match self {
            LengthOwner::Array => ArrayObject::set_length(rt, o, l, throw),
            LengthOwner::Sparse => SparseArrayObject::set_length(rt, o, l, throw),
            LengthOwner::Templated => TemplatedArrayObject::set_length(rt, o, l, throw),
        }
    }
}

pub(crate) fn define_array_length(
    rt: &mut Runtime,
    o: ObjRef,
    owner: LengthOwner,
    descr: &PropertyDescriptor,
    throw: bool,
) -> JsResult<bool> {
    let mut new_len = 0u32;
    let mut ret = true;
    let stale_value = owner.prop_value(rt, o);
    let stale_writable = owner.writable(rt, o);
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
            let current = owner.is_current(rt, o);
            let pv = if current {
                owner.prop_value(rt, o)
            } else {
                stale_value
            };
            let old_len = pv.prim_to_integer() as u32;
            if old_len != new_len {
                ret = if current {
                    owner.set_length(rt, o, new_len, false)?
                } else {
                    stale_writable
                };
            }
        } else {
            ret = true;
        }
        if descr.writable != Flag::NotSet {
            let w = descr.writable.bool();
            let current = owner.is_current(rt, o);
            let writable = if current {
                owner.writable(rt, o)
            } else {
                stale_writable
            };
            if writable {
                if current {
                    owner.set_writable(rt, o, w);
                }
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

pub static ARRAY_OBJECT_CLASS: ObjClass = ObjClass {
    sort_len: ArrayObject::sort_len,
    sort_get: ArrayObject::sort_get,
    swap: ArrayObject::swap,
    get_str: ArrayObject::get_str,
    get_idx: ArrayObject::get_idx,
    get_own_prop_str: ArrayObject::get_own_prop_str,
    get_own_prop_idx: ArrayObject::get_own_prop_idx,
    set_own_str: ArrayObject::set_own_str,
    set_own_idx: ArrayObject::set_own_idx,
    set_foreign_str: ArrayObject::set_foreign_str,
    set_foreign_idx: ArrayObject::set_foreign_idx,
    has_property_idx: ArrayObject::has_property_idx,
    has_own_property_str: ArrayObject::has_own_property_str,
    has_own_property_idx: ArrayObject::has_own_property_idx,
    define_own_property_str: ArrayObject::define_own_property_str,
    define_own_property_idx: ArrayObject::define_own_property_idx,
    delete_str: ArrayObject::delete_str,
    delete_idx: ArrayObject::delete_idx,
    iterate_string_keys: ArrayObject::iterate_string_keys,
    string_keys: ArrayObject::string_keys,
    ..BASE_CLASS
};

pub fn with_own_value_property(
    rt: &mut Runtime,
    o: ObjRef,
    name: &Value,
    f: &dyn Fn(&mut ValueProperty),
) -> bool {
    if matches!(name, Value::Symbol(_)) {
        return false;
    }
    let is_array = matches!(rt.obj(o).ext, ObjExt::Array(_) | ObjExt::SparseArray(_));
    if !is_array {
        return false;
    }
    let key = name.prim_string();
    let idx = str_to_array_idx(&key);
    let obj = rt.obj_mut(o);
    if idx != u32::MAX {
        let slot = match &mut obj.ext {
            ObjExt::Array(a) => a.values.get_mut(idx as usize),
            ObjExt::SparseArray(a) => {
                let i = a.find_idx(idx);
                match a.items.get_mut(i) {
                    Some(item) if item.idx == idx => Some(&mut item.value),
                    _ => None,
                }
            }
            _ => None,
        };
        if let Some(Value::Property(p)) = slot {
            f(p);
        }
        return true;
    }
    if key == "length" {
        match &mut obj.ext {
            ObjExt::Array(a) => f(&mut a.length_prop),
            ObjExt::SparseArray(a) => f(&mut a.length_prop),
            _ => {}
        }
        let w = match &obj.ext {
            ObjExt::Array(a) => a.length_prop.writable,
            ObjExt::SparseArray(a) => a.length_prop.writable,
            _ => true,
        };
        if let Some(Value::Property(p)) = obj.base.values.get_mut(&key) {
            p.writable = w;
        }
        return true;
    }
    false
}

pub fn check_std_array_obj_values(rt: &mut Runtime, o: ObjRef) -> Option<Vec<Value>> {
    rt.check_std_array_obj(o)?;
    match &rt.obj(o).ext {
        ObjExt::Array(a) => Some(a.values.clone()),
        _ => None,
    }
}

pub fn define_array_length_templated(
    rt: &mut Runtime,
    o: ObjRef,
    descr: PropertyDescriptor,
    throw: bool,
) -> JsResult<bool> {
    TemplatedObject::get_own_prop_str(rt, o, &UStr::from("length"))?;
    define_array_length(rt, o, LengthOwner::Templated, &descr, throw)
}

const GO_VALUE_SIZE: usize = 16;

const GO_CLASS_TO_SIZE: [usize; 68] = [
    0, 8, 16, 24, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288, 320,
    352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 896, 1024, 1152, 1280, 1408, 1536, 1792,
    2048, 2304, 2688, 3072, 3200, 3456, 4096, 4864, 5376, 6144, 6528, 6784, 6912, 8192, 9472, 9728,
    10240, 10880, 12288, 13568, 14336, 16384, 18432, 19072, 20480, 21760, 24576, 27264, 28672,
    32768,
];

fn go_roundupsize(size: usize) -> usize {
    const MAX_SMALL_SIZE: usize = 32768;
    const MALLOC_HEADER_SIZE: usize = 8;
    const MIN_SIZE_FOR_MALLOC_HEADER: usize = 512;
    const PAGE_SIZE: usize = 8192;
    let mut req_size = size;
    if req_size <= MAX_SMALL_SIZE - MALLOC_HEADER_SIZE {
        if req_size > MIN_SIZE_FOR_MALLOC_HEADER {
            req_size += MALLOC_HEADER_SIZE;
        }
        let class = GO_CLASS_TO_SIZE
            .iter()
            .copied()
            .find(|&c| c >= req_size)
            .unwrap_or(MAX_SMALL_SIZE);
        return class - (req_size - size);
    }
    req_size += PAGE_SIZE - 1;
    if req_size < size {
        return size;
    }
    req_size & !(PAGE_SIZE - 1)
}

fn go_nextslicecap(new_len: usize, old_cap: usize) -> usize {
    let mut newcap = old_cap;
    let doublecap = newcap + newcap;
    if new_len > doublecap {
        return new_len;
    }
    const THRESHOLD: usize = 256;
    if old_cap < THRESHOLD {
        return doublecap;
    }
    loop {
        newcap += (newcap + 3 * THRESHOLD) >> 2;
        if newcap >= new_len {
            break;
        }
    }
    newcap
}

pub fn go_append(values: &mut Vec<Value>, v: Value) {
    if values.len() == values.capacity() {
        let newcap = go_nextslicecap(values.len() + 1, values.capacity());
        let newcap = go_roundupsize(newcap * GO_VALUE_SIZE) / GO_VALUE_SIZE;
        values.reserve_exact(newcap - values.len());
    }
    values.push(v);
}

impl Runtime {
    pub fn new_array(&mut self, prototype: Option<ObjRef>) -> ObjRef {
        let mut base = BaseObject::new(CLASS_ARRAY, prototype);
        base._put(&UStr::from("length"), ArrayObject::length_placeholder(true));
        self.alloc_object(
            &ARRAY_OBJECT_CLASS,
            base,
            ObjExt::Array(Box::new(ArrayObject {
                values: Vec::new(),
                length: 0,
                obj_count: 0,
                prop_value_count: 0,
                length_prop: ValueProperty {
                    writable: true,
                    ..ValueProperty::default()
                },
            })),
        )
    }

    pub fn new_array_object(&mut self) -> ObjRef {
        let proto = self.get_array_prototype();
        self.new_array(Some(proto))
    }

    pub fn new_array_values(&mut self, values: Vec<Value>) -> ObjRef {
        let o = self.new_array_object();
        set_array_values(self, o, values);
        o
    }

    pub fn new_array_length(&mut self, l: i64) -> JsResult<ObjRef> {
        let o = self.new_array_object();
        set_array_length(self, o, l)?;
        Ok(o)
    }

    pub fn create_array_iterator(&mut self, iter_obj: ObjRef, kind: IterationKind) -> Value {
        let proto = self.get_array_iterator_prototype();
        let o = self.alloc_object(
            &ARRAY_ITER_OBJECT_CLASS,
            BaseObject::new(CLASS_OBJECT, Some(proto)),
            ObjExt::Dyn(Box::new(ArrayIterObject {
                obj: Some(iter_obj),
                next_idx: 0,
                kind,
            })),
        );
        Value::Object(o)
    }
}

pub fn set_array_values(rt: &mut Runtime, o: ObjRef, values: Vec<Value>) {
    if let Some(a) = ArrayObject::try_data_mut(rt, o) {
        a.length = values.len() as u32;
        a.obj_count = values.len();
        a.values = values;
    }
}

pub fn set_array_length(rt: &mut Runtime, o: ObjRef, l: i64) -> JsResult<()> {
    o.set_own_str(rt, &UStr::from("length"), int_to_value(l), true)?;
    Ok(())
}
