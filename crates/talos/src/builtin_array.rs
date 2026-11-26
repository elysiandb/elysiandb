use crate::array::{ArrayIterObject, ArrayObject, go_append, set_array_length, set_array_values};
use crate::builtin_symbol::{
    SYM_IS_CONCAT_SPREADABLE, SYM_ITERATOR, SYM_SPECIES, SYM_TO_STRING_TAG, SYM_UNSCOPABLES,
};
use crate::func::NativeFn;
use crate::heap::ObjRef;
use crate::object::{
    BASE_OBJECT_CLASS, CLASS_ARRAY_ITERATOR, CLASS_OBJECT, CallFn, Flag, FunctionCall, ObjExt,
    PropertyDescriptor, to_method,
};
use crate::object_template::ObjectTemplate;
use crate::runtime::{
    IterationKind, JsResult, Runtime, Throw, create_data_property_or_throw, is_array, to_length,
};
use crate::string::{JsString, STRING_EMPTY, ascii_string};
use crate::string_unicode::StringBuilder;
use crate::value::{MAX_INT, Value, int_to_value, nil_safe, value_prop};
use crate::vm::{STR_CONSTRUCTOR, STR_LENGTH};
use std::sync::OnceLock;

fn get_length(rt: &mut Runtime, o: ObjRef) -> JsResult<i64> {
    let v = o.get_str(rt, &STR_LENGTH, None)?;
    to_length(rt, &v.unwrap_or(Value::Empty))
}

fn go_make_cap(cap: i64) -> Vec<Value> {
    let ok = u64::try_from(cap)
        .ok()
        .and_then(|c| c.checked_mul(16))
        .is_some_and(|m| m <= 1 << 48);
    if !ok {
        panic!("array capacity out of range: {cap}");
    }
    Vec::with_capacity(cap as usize)
}

fn go_make_len(len: i64) -> Vec<Value> {
    let mut v = go_make_cap(len);
    v.resize(len as usize, Value::Empty);
    v
}

fn go_reslice(values: &mut Vec<Value>, n: usize) {
    if n <= values.len() {
        values.truncate(n);
    } else if n <= values.capacity() {
        values.resize(n, Value::Empty);
    } else {
        panic!(
            "array length {n} out of range for capacity {}",
            values.capacity()
        );
    }
}

fn create_data_property_or_throw_opt(
    rt: &mut Runtime,
    o: ObjRef,
    p: &Value,
    v: Value,
) -> JsResult<bool> {
    o.define_own_property(
        rt,
        p,
        PropertyDescriptor {
            writable: Flag::True,
            enumerable: Flag::True,
            configurable: Flag::True,
            value: if v.is_empty() { None } else { Some(v) },
            ..PropertyDescriptor::default()
        },
        true,
    )
}

pub fn array_species_create(rt: &mut Runtime, obj: ObjRef, size: i64) -> JsResult<ObjRef> {
    if is_array(rt, obj)? {
        let mut v = obj
            .get_str(rt, &STR_CONSTRUCTOR, None)?
            .filter(|v| !v.is_empty());
        if let Some(Value::Object(construct_obj)) = &v {
            v = construct_obj
                .get_sym(rt, SYM_SPECIES, None)?
                .filter(|v| !v.is_empty());
            if matches!(v, Some(Value::Null)) {
                v = None;
            }
        }
        if let Some(v) = v
            && !v.is_undefined()
        {
            if let Value::Object(construct_obj) = v
                && let Some(constructor) = construct_obj.assert_constructor(rt)
            {
                return constructor.construct(rt, &[int_to_value(size)], Some(construct_obj));
            }
            return Err(Throw::Value(
                rt.new_type_error("Species is not a constructor"),
            ));
        }
    }
    rt.new_array_length(size)
}

pub fn rel_to_idx(rel: i64, l: i64) -> i64 {
    if rel >= 0 {
        return rel.min(l);
    }
    (l + rel).max(0)
}

fn std_values(rt: &Runtime, o: ObjRef) -> &Vec<Value> {
    match &rt.obj(o).ext {
        ObjExt::Array(a) => &a.values,
        _ => unreachable!("not an array object"),
    }
}

fn std_data_mut(rt: &mut Runtime, o: ObjRef) -> Option<&mut ArrayObject> {
    match &mut rt.obj_mut(o).ext {
        ObjExt::Array(a) => Some(a),
        _ => None,
    }
}

fn int_digits(i: i64, b: &mut [u8; 20]) -> &[u8] {
    let mut n = i.unsigned_abs();
    let mut p = b.len();
    loop {
        p -= 1;
        b[p] = b'0' + (n % 10) as u8;
        n /= 10;
        if n == 0 {
            break;
        }
    }
    if i < 0 {
        p -= 1;
        b[p] = b'-';
    }
    &b[p..]
}

fn ascii_sort_key<'a>(v: &'a Value, b: &'a mut [u8; 20]) -> Option<&'a [u8]> {
    match v {
        Value::Int(i) => Some(int_digits(*i, b)),
        Value::String(JsString::Ascii(s)) => Some(s.as_bytes()),
        _ => None,
    }
}

fn write_value_string(rt: &mut Runtime, buf: &mut StringBuilder, v: &Value) -> JsResult<()> {
    match v {
        Value::String(s) => buf.write_string(s),
        Value::Int(i) => {
            let mut b = [0u8; 20];
            buf.write_ascii(int_digits(*i, &mut b));
        }
        _ => {
            let s = v.to_string(rt)?;
            buf.write_string(&s);
        }
    }
    Ok(())
}

impl Runtime {
    pub fn builtin_new_array(&mut self, args: &[Value], proto: Option<ObjRef>) -> JsResult<ObjRef> {
        let l = args.len();
        if l == 1 {
            if let Value::Int(al) = args[0] {
                let a = self.new_array(proto);
                set_array_length(self, a, al)?;
                return Ok(a);
            } else if let Value::Float(f) = args[0] {
                let al = f as i64;
                if al as f64 == f {
                    return self.new_array_length(al);
                }
                let e = self.get_range_error();
                return Err(Throw::Value(self.new_error(e, "Invalid array length")));
            }
            let a = self.new_array(proto);
            set_array_values(self, a, vec![args[0].clone()]);
            return Ok(a);
        }
        let a = self.new_array(proto);
        set_array_values(self, a, args.to_vec());
        Ok(a)
    }

    pub fn generic_push(&mut self, obj: ObjRef, call: FunctionCall<'_>) -> JsResult<Value> {
        let l = get_length(self, obj)?;
        let nl = l + call.arguments.len() as i64;
        if nl >= MAX_INT {
            self.type_error_result(true, "Invalid array length")?;
        }
        for (i, arg) in call.arguments.iter().enumerate() {
            obj.set_own_idx(self, l + i as i64, arg.clone(), true)?;
        }
        let n = Value::Int(nl);
        obj.set_own_str(self, &STR_LENGTH, n.clone(), true)?;
        Ok(n)
    }

    pub fn arrayproto_push(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.this.to_object(self)?;
        self.generic_push(obj, call)
    }

    pub fn arrayproto_pop_generic(&mut self, obj: ObjRef) -> JsResult<Value> {
        let l = get_length(self, obj)?;
        if l == 0 {
            obj.set_own_str(self, &STR_LENGTH, int_to_value(0), true)?;
            return Ok(Value::Undefined);
        }
        let idx = l - 1;
        let val = obj.get_idx(self, idx, None)?;
        obj.delete_idx(self, idx, true)?;
        obj.set_own_str(self, &STR_LENGTH, Value::Int(idx), true)?;
        Ok(val.unwrap_or(Value::Empty))
    }

    pub fn arrayproto_pop(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.this.to_object(self)?;
        let Some(a) = std_data_mut(self, obj) else {
            return self.arrayproto_pop_generic(obj);
        };
        let mut l = a.length;
        let val;
        if l > 0 {
            l -= 1;
            let bail = match a.values.get(l as usize) {
                None | Some(Value::Empty) | Some(Value::Property(_)) => true,
                Some(_) => false,
            };
            if bail {
                return self.arrayproto_pop_generic(obj);
            }
            val = std::mem::replace(&mut a.values[l as usize], Value::Empty);
            a.values.truncate(l as usize);
        } else {
            val = Value::Undefined;
        }
        if a.length_prop.writable {
            a.length = l;
        } else {
            ArrayObject::set_length(self, obj, 0, true)?;
        }
        Ok(val)
    }

    pub fn arrayproto_join(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let sep = match call.arg(0) {
            Value::Undefined => ascii_string(","),
            s => s.to_string(self)?,
        };
        if l == 0 {
            return Ok(Value::String(STRING_EMPTY.clone()));
        }
        let mut buf = StringBuilder::new();
        let mark = self.handle_scope_enter();
        let element0 = o.get_idx(self, 0, None)?;
        if let Some(e) = &element0
            && !matches!(e, Value::Empty | Value::Undefined | Value::Null)
        {
            write_value_string(self, &mut buf, e)?;
        }
        self.handle_scope_leave(mark);
        for i in 1..l {
            buf.write_string(&sep);
            let element = o.get_idx(self, i, None)?;
            if let Some(e) = &element
                && !matches!(e, Value::Empty | Value::Undefined | Value::Null)
            {
                write_value_string(self, &mut buf, e)?;
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::String(buf.string()))
    }

    pub fn arrayproto_to_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let array = call.this.to_object(self)?;
        let f = array.get_str(self, &"join".into(), None)?;
        if let Some(Value::Object(f_obj)) = f
            && let Some(fcall) = f_obj.assert_callable(self)
        {
            return fcall.call(self, Value::Object(array), &[]);
        }
        self.objectproto_to_string(FunctionCall::new(&Value::Object(array), &[]))
    }

    fn write_item_locale_string(&mut self, item: &Value, buf: &mut StringBuilder) -> JsResult<()> {
        if !matches!(item, Value::Empty | Value::Undefined | Value::Null) {
            if let Some(Value::Object(f)) = self.get_v_str(item, &"toLocaleString".into())?
                && let Some(c) = f.assert_callable(self)
            {
                let str_val = c.call(self, item.clone(), &[])?;
                let s = str_val.to_string(self)?;
                buf.write_string(&s);
                return Ok(());
            }
            let s = item.fmt_string(self)?;
            self.type_error_result(
                true,
                &format!("Property 'toLocaleString' of object {s} is not a function"),
            )?;
        }
        Ok(())
    }

    pub fn arrayproto_to_locale_string(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let array = call.this.to_object(self)?;
        let mut buf = StringBuilder::new();
        if self.check_std_array_obj(array).is_some() {
            let n = std_values(self, array).len();
            for i in 0..n {
                if i > 0 {
                    buf.write_ascii(b",");
                }
                let item = match &self.obj(array).ext {
                    ObjExt::Array(a) => a.values.get(i).cloned().unwrap_or(Value::Empty),
                    _ => Value::Empty,
                };
                self.write_item_locale_string(&item, &mut buf)?;
            }
        } else {
            let length = get_length(self, array)?;
            for i in 0..length {
                if i > 0 {
                    buf.write_ascii(b",");
                }
                let item = array.get_idx(self, i, None)?.unwrap_or(Value::Empty);
                self.write_item_locale_string(&item, &mut buf)?;
            }
        }
        Ok(Value::String(buf.string()))
    }

    fn arrayproto_concat_append(&mut self, a: ObjRef, item: &Value) -> JsResult<()> {
        let mut a_length = get_length(self, a)?;
        let spreadable = match item {
            Value::Object(obj) => is_concat_spreadable(self, *obj)?,
            _ => false,
        };
        if let (Value::Object(obj), true) = (item, spreadable) {
            let obj = *obj;
            let length = get_length(self, obj)?;
            if a_length + length >= MAX_INT {
                return Err(Throw::Value(self.new_type_error("Invalid array length")));
            }
            let mark = self.handle_scope_enter();
            for i in 0..length {
                let v = obj.get_idx(self, i, None)?;
                if let Some(v) = v
                    && !v.is_empty()
                {
                    create_data_property_or_throw(self, a, &int_to_value(a_length), v)?;
                }
                a_length += 1;
                self.handle_scope_leave(mark);
            }
        } else {
            create_data_property_or_throw(self, a, &int_to_value(a_length), item.clone())?;
            a_length += 1;
        }
        a.set_own_str(self, &STR_LENGTH, int_to_value(a_length), true)?;
        Ok(())
    }

    pub fn arrayproto_concat(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let obj = call.this.to_object(self)?;
        let a = array_species_create(self, obj, 0)?;
        let this = Value::Object(call.this.to_object(self)?);
        self.arrayproto_concat_append(a, &this)?;
        for item in call.arguments {
            self.arrayproto_concat_append(a, item)?;
        }
        Ok(Value::Object(a))
    }

    pub fn arrayproto_slice(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let mut start = rel_to_idx(call.arg(0).to_integer(self)?, length);
        let end = match call.arg(1) {
            Value::Undefined => length,
            e => e.to_integer(self)?,
        };
        let end = rel_to_idx(end, length);
        let count = (end - start).max(0);
        let a = array_species_create(self, o, count)?;
        if self.check_std_array_obj(o).is_some()
            && let Some(dst) = self.check_std_array_obj_with_proto(a)
        {
            let mut values = go_make_cap(count);
            {
                let src = &std_values(self, o)[start as usize..];
                let n = (count as usize).min(src.len());
                values.extend_from_slice(&src[..n]);
            }
            values.resize(count as usize, Value::Empty);
            set_array_values(self, dst, values);
            return Ok(Value::Object(a));
        }
        let mut n = 0i64;
        while start < end {
            let p = o.get_idx(self, start, None)?;
            if let Some(p) = p
                && !p.is_empty()
            {
                create_data_property_or_throw(self, a, &Value::Int(n), p)?;
            }
            start += 1;
            n += 1;
        }
        Ok(Value::Object(a))
    }

    pub fn arrayproto_sort(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let mut compare_fn = None;
        let arg = call.arg(0);
        if !arg.is_undefined() {
            if let Value::Object(f) = arg {
                compare_fn = f.assert_callable(self);
            }
            if compare_fn.is_none() {
                return Err(Throw::Value(self.new_type_error(
                    "The comparison function must be either a function or undefined",
                )));
            }
        }
        if self.check_std_array_obj(o).is_some() {
            let ctx = ArraySortCtx {
                obj: o,
                compare: compare_fn,
            };
            ctx.stable(self)?;
        } else {
            let length = get_length(self, o)?;
            let mut a = go_make_cap(length);
            for i in 0..length {
                if o.has_property_idx(self, i)? {
                    let v = nil_safe(o.get_idx(self, i, None)?);
                    a.push(v);
                }
            }
            let ar = self.new_array_values(a);
            let ctx = ArraySortCtx {
                obj: ar,
                compare: compare_fn,
            };
            ctx.stable(self)?;
            let n = std_values(self, ar).len();
            for i in 0..n {
                let v = std_values(self, ar)[i].clone();
                o.set_own_idx(self, i as i64, v, true)?;
            }
            for i in n as i64..length {
                o.delete_idx(self, i, true)?;
            }
        }
        Ok(Value::Object(o))
    }

    pub fn arrayproto_splice(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let actual_start = rel_to_idx(call.arg(0).to_integer(self)?, length);
        let actual_delete_count = match call.arguments.len() {
            0 => 0,
            1 => length - actual_start,
            _ => call
                .arg(1)
                .to_integer(self)?
                .max(0)
                .min(length - actual_start),
        };
        let item_count = (call.arguments.len() as i64 - 2).max(0);
        let new_length = length - actual_delete_count + item_count;
        if new_length >= MAX_INT {
            return Err(Throw::Value(self.new_type_error("Invalid array length")));
        }
        let a = array_species_create(self, o, actual_delete_count)?;
        if self.check_std_array_obj(o).is_some() {
            let s = actual_start as usize;
            let dc = actual_delete_count as usize;
            let ic = item_count as usize;
            let nl = new_length as usize;
            if let Some(dst) = self.check_std_array_obj_with_proto(a) {
                let mut values = go_make_cap(actual_delete_count);
                {
                    let src = &std_values(self, o)[s..];
                    let n = dc.min(src.len());
                    values.extend_from_slice(&src[..n]);
                }
                values.resize(dc, Value::Empty);
                set_array_values(self, dst, values);
            } else {
                for k in 0..actual_delete_count {
                    let v = match &self.obj(o).ext {
                        ObjExt::Array(src) => src.values[(k + actual_start) as usize].clone(),
                        _ => Value::Empty,
                    };
                    create_data_property_or_throw_opt(self, a, &int_to_value(k), v)?;
                }
                a.set_own_str(self, &STR_LENGTH, int_to_value(actual_delete_count), true)?;
            }
            if let Some(src) = std_data_mut(self, o) {
                let values = &mut src.values;
                if ic < dc {
                    values[s + ic..].rotate_left(dc - ic);
                    go_reslice(values, nl);
                } else if ic > dc {
                    if values.capacity() >= nl {
                        let l = length as usize;
                        go_reslice(values, nl);
                        values[s + dc..nl].rotate_right(nl - l);
                    } else {
                        let old = std::mem::take(values);
                        let mut nv = Vec::with_capacity(nl);
                        nv.extend_from_slice(&old[..s]);
                        nv.resize(s + ic, Value::Empty);
                        nv.extend_from_slice(&old[s + dc..]);
                        nv.resize(nl, Value::Empty);
                        *values = nv;
                    }
                }
                if ic > 0 {
                    for (j, item) in call.arguments[2..].iter().enumerate() {
                        values[s + j] = item.clone();
                    }
                }
                src.obj_count = src.values.len();
            }
        } else {
            for k in 0..actual_delete_count {
                let from = k + actual_start;
                if o.has_property_idx(self, from)? {
                    let v = nil_safe(o.get_idx(self, from, None)?);
                    create_data_property_or_throw(self, a, &Value::Int(k), v)?;
                }
            }
            if item_count < actual_delete_count {
                for k in actual_start..length - actual_delete_count {
                    let from = k + actual_delete_count;
                    let to = k + item_count;
                    if o.has_property_idx(self, from)? {
                        let v = nil_safe(o.get_idx(self, from, None)?);
                        o.set_own_idx(self, to, v, true)?;
                    } else {
                        o.delete_idx(self, to, true)?;
                    }
                }
                let mut k = length;
                while k > length - actual_delete_count + item_count {
                    o.delete_idx(self, k - 1, true)?;
                    k -= 1;
                }
            } else if item_count > actual_delete_count {
                let mut k = length - actual_delete_count;
                while k > actual_start {
                    let from = k + actual_delete_count - 1;
                    let to = k + item_count - 1;
                    if o.has_property_idx(self, from)? {
                        let v = nil_safe(o.get_idx(self, from, None)?);
                        o.set_own_idx(self, to, v, true)?;
                    } else {
                        o.delete_idx(self, to, true)?;
                    }
                    k -= 1;
                }
            }
            if item_count > 0 {
                for (i, item) in call.arguments[2..].iter().enumerate() {
                    o.set_own_idx(self, actual_start + i as i64, item.clone(), true)?;
                }
            }
        }
        o.set_own_str(self, &STR_LENGTH, int_to_value(new_length), true)?;
        Ok(Value::Object(a))
    }

    pub fn arrayproto_unshift(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let arg_count = call.arguments.len() as i64;
        let new_len = int_to_value(length + arg_count);
        if arg_count > 0 {
            let new_size = length + arg_count;
            if new_size >= MAX_INT {
                return Err(Throw::Value(self.new_type_error("Invalid array length")));
            }
            let std_arr = self.check_std_array_obj_with_proto(o);
            if let Some(arr) = std_arr
                && new_size < i64::from(u32::MAX)
                && let Some(a) = std_data_mut(self, arr)
            {
                let ns = new_size as usize;
                let ac = arg_count as usize;
                let l = length as usize;
                if a.values.capacity() >= ns {
                    go_reslice(&mut a.values, ns);
                    a.values[..ns].rotate_right(ns - l);
                } else {
                    let old = std::mem::take(&mut a.values);
                    let mut values = Vec::with_capacity(ns);
                    values.resize(ac, Value::Empty);
                    values.extend_from_slice(&old[..(ns - ac).min(old.len())]);
                    values.resize(ns, Value::Empty);
                    a.values = values;
                }
                for (i, arg) in call.arguments.iter().enumerate() {
                    a.values[i] = arg.clone();
                }
                a.obj_count = a.length as usize;
            } else {
                let mut k = length - 1;
                while k >= 0 {
                    let from = k;
                    let to = k + arg_count;
                    if o.has_property_idx(self, from)? {
                        let v = nil_safe(o.get_idx(self, from, None)?);
                        o.set_own_idx(self, to, v, true)?;
                    } else {
                        o.delete_idx(self, to, true)?;
                    }
                    k -= 1;
                }
                for (k, arg) in call.arguments.iter().enumerate() {
                    o.set_own_idx(self, k as i64, arg.clone(), true)?;
                }
            }
        }
        o.set_own_str(self, &STR_LENGTH, new_len.clone(), true)?;
        Ok(new_len)
    }

    pub fn arrayproto_at(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let mut idx = call.arg(0).to_integer(self)?;
        let length = get_length(self, o)?;
        if idx < 0 {
            idx += length;
        }
        if idx >= length || idx < 0 {
            return Ok(Value::Undefined);
        }
        if o.has_property_idx(self, idx)? {
            return Ok(o.get_idx(self, idx, None)?.unwrap_or(Value::Empty));
        }
        Ok(Value::Undefined)
    }

    pub fn arrayproto_index_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        if length == 0 {
            return Ok(int_to_value(-1));
        }
        let mut n = call.arg(1).to_integer(self)?;
        if n >= length {
            return Ok(int_to_value(-1));
        }
        if n < 0 {
            n = (length + n).max(0);
        }
        let search_element = call.argument(0);
        if self.check_std_array_obj(o).is_some() {
            let values = std_values(self, o);
            for (i, val) in values[n as usize..].iter().enumerate() {
                if search_element.strict_equals(val) {
                    return Ok(int_to_value(n + i as i64));
                }
            }
            return Ok(int_to_value(-1));
        }
        let mark = self.handle_scope_enter();
        while n < length {
            if o.has_property_idx(self, n)?
                && let Some(val) = o.get_idx(self, n, None)?
                && !val.is_empty()
                && search_element.strict_equals(&val)
            {
                return Ok(Value::Int(n));
            }
            self.handle_scope_leave(mark);
            n += 1;
        }
        Ok(int_to_value(-1))
    }

    pub fn arrayproto_includes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        if length == 0 {
            return Ok(Value::Bool(false));
        }
        let mut n = call.arg(1).to_integer(self)?;
        if n >= length {
            return Ok(Value::Bool(false));
        }
        if n < 0 {
            n = (length + n).max(0);
        }
        let mut search_element = call.argument(0);
        if matches!(search_element, Value::Float(f) if f == 0.0) {
            search_element = Value::Int(0);
        }
        if self.check_std_array_obj(o).is_some() {
            let values = std_values(self, o);
            for val in &values[n as usize..] {
                if search_element.same_as(val) {
                    return Ok(Value::Bool(true));
                }
            }
            return Ok(Value::Bool(false));
        }
        let mark = self.handle_scope_enter();
        while n < length {
            let val = nil_safe(o.get_idx(self, n, None)?);
            if search_element.same_as(&val) {
                return Ok(Value::Bool(true));
            }
            self.handle_scope_leave(mark);
            n += 1;
        }
        Ok(Value::Bool(false))
    }

    pub fn arrayproto_last_index_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        if length == 0 {
            return Ok(int_to_value(-1));
        }
        let from_index = if call.arguments.len() < 2 {
            length - 1
        } else {
            let f = call.arg(1).to_integer(self)?;
            if f >= 0 {
                f.min(length - 1)
            } else {
                f + length
            }
        };
        let search_element = call.argument(0);
        if self.check_std_array_obj(o).is_some() {
            let vals = std_values(self, o);
            let mut k = from_index;
            while k >= 0 {
                let v = &vals[k as usize];
                if !v.is_empty() && search_element.strict_equals(v) {
                    return Ok(int_to_value(k));
                }
                k -= 1;
            }
            return Ok(int_to_value(-1));
        }
        let mark = self.handle_scope_enter();
        let mut k = from_index;
        while k >= 0 {
            if o.has_property_idx(self, k)?
                && let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
                && search_element.strict_equals(&val)
            {
                return Ok(Value::Int(k));
            }
            self.handle_scope_leave(mark);
            k -= 1;
        }
        Ok(int_to_value(-1))
    }

    pub fn arrayproto_every(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..length {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                let r = callback_fn.call(
                    self,
                    this.clone(),
                    &[val, Value::Int(k), Value::Object(o)],
                )?;
                if !r.to_boolean() {
                    return Ok(Value::Bool(false));
                }
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Bool(true))
    }

    pub fn arrayproto_some(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..length {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                let r = callback_fn.call(
                    self,
                    this.clone(),
                    &[val, Value::Int(k), Value::Object(o)],
                )?;
                if r.to_boolean() {
                    return Ok(Value::Bool(true));
                }
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Bool(false))
    }

    pub fn arrayproto_for_each(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..length {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                callback_fn.call(self, this.clone(), &[val, Value::Int(k), Value::Object(o)])?;
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Undefined)
    }

    pub fn arrayproto_map(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let a = array_species_create(self, o, length)?;
        if ArrayObject::is_array_object(self, o) && ArrayObject::is_array_object(self, a) {
            let mut values = go_make_cap(length);
            for k in 0..length {
                let r = match o.get_idx(self, k, None)? {
                    Some(val) if !val.is_empty() => callback_fn.call(
                        self,
                        this.clone(),
                        &[val, Value::Int(k), Value::Object(o)],
                    )?,
                    _ => Value::Empty,
                };
                values.push(r);
            }
            set_array_values(self, a, values);
            return Ok(Value::Object(a));
        }
        for k in 0..length {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                let r = callback_fn.call(
                    self,
                    this.clone(),
                    &[val, Value::Int(k), Value::Object(o)],
                )?;
                create_data_property_or_throw_opt(self, a, &Value::Int(k), r)?;
            }
        }
        Ok(Value::Object(a))
    }

    pub fn arrayproto_filter(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_obj = call.arg(0).to_object(self)?;
        let Some(callback_fn) = callback_obj.assert_callable(self) else {
            let s = call.arg(0).fmt_string(self)?;
            return Err(Throw::Value(
                self.new_type_error(&format!("{s} is not a function")),
            ));
        };
        let a = array_species_create(self, o, 0)?;
        let this = call.argument(1);
        if ArrayObject::is_array_object(self, o) && self.check_std_array_obj(a).is_some() {
            let mut values = Vec::new();
            for k in 0..length {
                if let Some(val) = o.get_idx(self, k, None)?
                    && !val.is_empty()
                {
                    let r = callback_fn.call(
                        self,
                        this.clone(),
                        &[val.clone(), Value::Int(k), Value::Object(o)],
                    )?;
                    if r.to_boolean() {
                        go_append(&mut values, val);
                    }
                }
            }
            set_array_values(self, a, values);
            return Ok(Value::Object(a));
        }
        let mut to = 0i64;
        for k in 0..length {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                let r = callback_fn.call(
                    self,
                    this.clone(),
                    &[val.clone(), Value::Int(k), Value::Object(o)],
                )?;
                if r.to_boolean() {
                    create_data_property_or_throw(self, a, &int_to_value(to), val)?;
                    to += 1;
                }
            }
        }
        Ok(Value::Object(a))
    }

    pub fn arrayproto_reduce(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_obj = call.arg(0).to_object(self)?;
        let Some(callback_fn) = callback_obj.assert_callable(self) else {
            let s = call.arg(0).fmt_string(self)?;
            return Err(Throw::Value(
                self.new_type_error(&format!("{s} is not a function")),
            ));
        };
        let mut acc = Value::Empty;
        let mut k = 0i64;
        if call.arguments.len() >= 2 {
            acc = call.argument(1);
        } else {
            while k < length {
                if let Some(val) = o.get_idx(self, k, None)?
                    && !val.is_empty()
                {
                    acc = val;
                    break;
                }
                k += 1;
            }
            if acc.is_empty() {
                self.type_error_result(true, "No initial value")?;
            }
            k += 1;
        }
        let mark = self.handle_scope_enter();
        while k < length {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                let prev = std::mem::replace(&mut acc, Value::Empty);
                acc = callback_fn.call(
                    self,
                    Value::Undefined,
                    &[prev, val, Value::Int(k), Value::Object(o)],
                )?;
            }
            self.handle_scope_keep(mark, &acc);
            k += 1;
        }
        Ok(acc)
    }

    pub fn arrayproto_reduce_right(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let callback_obj = call.arg(0).to_object(self)?;
        let Some(callback_fn) = callback_obj.assert_callable(self) else {
            let s = call.arg(0).fmt_string(self)?;
            return Err(Throw::Value(
                self.new_type_error(&format!("{s} is not a function")),
            ));
        };
        let mut acc = Value::Empty;
        let mut k = length - 1;
        if call.arguments.len() >= 2 {
            acc = call.argument(1);
        } else {
            while k >= 0 {
                if let Some(val) = o.get_idx(self, k, None)?
                    && !val.is_empty()
                {
                    acc = val;
                    break;
                }
                k -= 1;
            }
            if acc.is_empty() {
                self.type_error_result(true, "No initial value")?;
            }
            k -= 1;
        }
        let mark = self.handle_scope_enter();
        while k >= 0 {
            if let Some(val) = o.get_idx(self, k, None)?
                && !val.is_empty()
            {
                let prev = std::mem::replace(&mut acc, Value::Empty);
                acc = callback_fn.call(
                    self,
                    Value::Undefined,
                    &[prev, val, Value::Int(k), Value::Object(o)],
                )?;
            }
            self.handle_scope_keep(mark, &acc);
            k -= 1;
        }
        Ok(acc)
    }

    fn arrayproto_reverse_generic_step(
        &mut self,
        o: ObjRef,
        lower: i64,
        upper: i64,
    ) -> JsResult<()> {
        let mut lower_value = Value::Empty;
        let mut upper_value = Value::Empty;
        let lower_exists = o.has_property_idx(self, lower)?;
        if lower_exists {
            lower_value = nil_safe(o.get_idx(self, lower, None)?);
        }
        let upper_exists = o.has_property_idx(self, upper)?;
        if upper_exists {
            upper_value = nil_safe(o.get_idx(self, upper, None)?);
        }
        if lower_exists && upper_exists {
            o.set_own_idx(self, lower, upper_value, true)?;
            o.set_own_idx(self, upper, lower_value, true)?;
        } else if !lower_exists && upper_exists {
            o.set_own_idx(self, lower, upper_value, true)?;
            o.delete_idx(self, upper, true)?;
        } else if lower_exists && !upper_exists {
            o.delete_idx(self, lower, true)?;
            o.set_own_idx(self, upper, lower_value, true)?;
        }
        Ok(())
    }

    pub fn arrayproto_reverse_generic(&mut self, o: ObjRef, start: i64) -> JsResult<()> {
        let l = get_length(self, o)?;
        let middle = l / 2;
        let mark = self.handle_scope_enter();
        let mut lower = start;
        while lower != middle {
            self.arrayproto_reverse_generic_step(o, lower, l - lower - 1)?;
            self.handle_scope_leave(mark);
            lower += 1;
        }
        Ok(())
    }

    pub fn arrayproto_reverse(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        if self.check_std_array_obj(o).is_some() {
            if let Some(a) = std_data_mut(self, o) {
                a.values.reverse();
            }
        } else {
            self.arrayproto_reverse_generic(o, 0)?;
        }
        Ok(Value::Object(o))
    }

    pub fn arrayproto_shift(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        if let Some(arr) = self.check_std_array_obj_with_proto(o) {
            let Some(a) = std_data_mut(self, arr) else {
                return Ok(Value::Undefined);
            };
            if a.values.is_empty() {
                if !a.length_prop.writable {
                    ArrayObject::set_length(self, arr, 0, true)?;
                }
                return Ok(Value::Undefined);
            }
            let first = a.values.remove(0);
            a.length = a.length.wrapping_sub(1);
            return Ok(first);
        }
        let length = get_length(self, o)?;
        if length == 0 {
            o.set_own_str(self, &STR_LENGTH, int_to_value(0), true)?;
            return Ok(Value::Undefined);
        }
        let first = o.get_idx(self, 0, None)?.unwrap_or(Value::Empty);
        let mark = self.handle_scope_enter();
        for i in 1..length {
            let idx_from = i;
            let idx_to = i - 1;
            if o.has_property_idx(self, idx_from)? {
                let v = nil_safe(o.get_idx(self, idx_from, None)?);
                o.set_own_idx(self, idx_to, v, true)?;
            } else {
                o.delete_idx(self, idx_to, true)?;
            }
            self.handle_scope_keep(mark, &first);
        }
        let lv = length - 1;
        o.delete_idx(self, lv, true)?;
        o.set_own_str(self, &STR_LENGTH, Value::Int(lv), true)?;
        Ok(first)
    }

    pub fn arrayproto_values(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        Ok(self.create_array_iterator(o, IterationKind::Value))
    }

    pub fn arrayproto_keys(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        Ok(self.create_array_iterator(o, IterationKind::Key))
    }

    pub fn arrayproto_copy_within(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let mut to = rel_to_idx(call.arg(0).to_integer(self)?, l);
        let mut from = rel_to_idx(call.arg(1).to_integer(self)?, l);
        let rel_end = match call.arg(2) {
            Value::Undefined => l,
            end => end.to_integer(self)?,
        };
        let final_ = rel_to_idx(rel_end, l);
        let mut count = (final_ - from).min(l - to);
        if self.check_std_array_obj(o).is_some() {
            if count > 0
                && let Some(a) = std_data_mut(self, o)
            {
                let (t, f, c) = (to as usize, from as usize, count as usize);
                if f < t {
                    for i in (0..c).rev() {
                        a.values[t + i] = a.values[f + i].clone();
                    }
                } else {
                    for i in 0..c {
                        a.values[t + i] = a.values[f + i].clone();
                    }
                }
            }
            return Ok(Value::Object(o));
        }
        let dir;
        if from < to && to < from + count {
            dir = -1;
            from = from + count - 1;
            to = to + count - 1;
        } else {
            dir = 1;
        }
        let mark = self.handle_scope_enter();
        while count > 0 {
            if o.has_property_idx(self, from)? {
                let v = nil_safe(o.get_idx(self, from, None)?);
                o.set_own_idx(self, to, v, true)?;
            } else {
                o.delete_idx(self, to, true)?;
            }
            self.handle_scope_leave(mark);
            from += dir;
            to += dir;
            count -= 1;
        }
        Ok(Value::Object(o))
    }

    pub fn arrayproto_entries(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        Ok(self.create_array_iterator(o, IterationKind::KeyValue))
    }

    pub fn arrayproto_fill(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let mut k = rel_to_idx(call.arg(1).to_integer(self)?, l);
        let rel_end = match call.arg(2) {
            Value::Undefined => l,
            end => end.to_integer(self)?,
        };
        let final_ = rel_to_idx(rel_end, l);
        let value = call.argument(0);
        if self.check_std_array_obj(o).is_some() {
            if let Some(a) = std_data_mut(self, o) {
                while k < final_ {
                    a.values[k as usize] = value.clone();
                    k += 1;
                }
            }
        } else {
            while k < final_ {
                o.set_own_idx(self, k, value.clone(), true)?;
                k += 1;
            }
        }
        Ok(Value::Object(o))
    }

    pub fn arrayproto_find(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..l {
            let k_value = o.get_idx(self, k, None)?.unwrap_or(Value::Empty);
            let r = predicate.call(
                self,
                this.clone(),
                &[k_value.clone(), Value::Int(k), Value::Object(o)],
            )?;
            if r.to_boolean() {
                return Ok(k_value);
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Undefined)
    }

    pub fn arrayproto_find_index(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..l {
            let k_value = o.get_idx(self, k, None)?.unwrap_or(Value::Empty);
            let r = predicate.call(
                self,
                this.clone(),
                &[k_value, Value::Int(k), Value::Object(o)],
            )?;
            if r.to_boolean() {
                return Ok(Value::Int(k));
            }
            self.handle_scope_leave(mark);
        }
        Ok(int_to_value(-1))
    }

    pub fn arrayproto_find_last(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        let mut k = l - 1;
        while k >= 0 {
            let k_value = o.get_idx(self, k, None)?.unwrap_or(Value::Empty);
            let r = predicate.call(
                self,
                this.clone(),
                &[k_value.clone(), Value::Int(k), Value::Object(o)],
            )?;
            if r.to_boolean() {
                return Ok(k_value);
            }
            self.handle_scope_leave(mark);
            k -= 1;
        }
        Ok(Value::Undefined)
    }

    pub fn arrayproto_find_last_index(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        let mut k = l - 1;
        while k >= 0 {
            let k_value = o.get_idx(self, k, None)?.unwrap_or(Value::Empty);
            let r = predicate.call(
                self,
                this.clone(),
                &[k_value, Value::Int(k), Value::Object(o)],
            )?;
            if r.to_boolean() {
                return Ok(Value::Int(k));
            }
            self.handle_scope_leave(mark);
            k -= 1;
        }
        Ok(int_to_value(-1))
    }

    pub fn arrayproto_flat(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let mut depth_num = 1i64;
        if !call.arguments.is_empty() {
            depth_num = call.arg(0).to_integer(self)?;
        }
        let a = array_species_create(self, o, 0)?;
        self.flatten_into_array(a, o, l, 0, depth_num, None, &Value::Undefined)?;
        Ok(Value::Object(a))
    }

    #[allow(clippy::too_many_arguments)]
    pub fn flatten_into_array(
        &mut self,
        target: ObjRef,
        source: ObjRef,
        source_len: i64,
        start: i64,
        depth: i64,
        mapper_function: Option<CallFn>,
        this_arg: &Value,
    ) -> JsResult<i64> {
        let mut target_index = start;
        let mut source_index = 0i64;
        while source_index < source_len {
            let p = int_to_value(source_index);
            let ps = p.to_string(self)?;
            if source.has_property(self, &Value::String(ps))? {
                let mut element = nil_safe(source.get(self, &p, Some(&Value::Object(source)))?);
                if let Some(m) = mapper_function {
                    element = m.call(
                        self,
                        this_arg.clone(),
                        &[element, p.clone(), Value::Object(source)],
                    )?;
                }
                let mut element_array = None;
                if depth > 0
                    && let Value::Object(element_obj) = &element
                    && is_array(self, *element_obj)?
                {
                    element_array = Some(*element_obj);
                }
                if let Some(ea) = element_array {
                    let element_len = get_length(self, ea)?;
                    target_index = self.flatten_into_array(
                        target,
                        ea,
                        element_len,
                        target_index,
                        depth - 1,
                        None,
                        &Value::Undefined,
                    )?;
                } else {
                    if target_index >= MAX_INT - 1 {
                        return Err(Throw::Value(self.new_type_error("Invalid array length")));
                    }
                    create_data_property_or_throw_opt(
                        self,
                        target,
                        &int_to_value(target_index),
                        element,
                    )?;
                    target_index += 1;
                }
            }
            source_index += 1;
        }
        Ok(target_index)
    }

    pub fn arrayproto_flat_map(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let l = get_length(self, o)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this_arg = if call.arguments.len() > 1 {
            call.argument(1)
        } else {
            Value::Undefined
        };
        let a = array_species_create(self, o, 0)?;
        self.flatten_into_array(a, o, l, 0, 1, Some(callback_fn), &this_arg)?;
        Ok(Value::Object(a))
    }

    pub fn arrayproto_with(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let relative_index = call.arg(0).to_integer(self)?;
        let value = call.argument(1);
        let length = get_length(self, o)?;
        let actual_index = if relative_index >= 0 {
            relative_index
        } else {
            length + relative_index
        };
        if actual_index >= length || actual_index < 0 {
            let s = call.arg(0).go_string(self)?;
            let e = self.get_range_error();
            return Err(Throw::Value(
                self.new_error(e, &format!("Invalid index {s}")),
            ));
        }
        if self.check_std_array_obj(o).is_some() {
            let mut a = go_make_cap(length);
            {
                let src = std_values(self, o);
                for k in 0..length {
                    if k == actual_index {
                        a.push(value.clone());
                    } else {
                        a.push(src[k as usize].clone());
                    }
                }
            }
            return Ok(Value::Object(self.new_array_values(a)));
        }
        let a = self.new_array_length(length)?;
        for k in 0..length {
            let from_value = if k == actual_index {
                value.clone()
            } else {
                o.get_idx(self, k, None)?.unwrap_or(Value::Empty)
            };
            create_data_property_or_throw_opt(self, a, &Value::Int(k), from_value)?;
        }
        Ok(Value::Object(a))
    }

    pub fn arrayproto_to_reversed(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        if self.check_std_array_obj(o).is_some() {
            let mut a = go_make_cap(length);
            {
                let src = std_values(self, o);
                for k in 0..length {
                    a.push(src[(length - k - 1) as usize].clone());
                }
            }
            return Ok(Value::Object(self.new_array_values(a)));
        }
        let a = self.new_array_length(length)?;
        for k in 0..length {
            let from = length - k - 1;
            let from_value = o.get_idx(self, from, None)?.unwrap_or(Value::Empty);
            create_data_property_or_throw_opt(self, a, &Value::Int(k), from_value)?;
        }
        Ok(Value::Object(a))
    }

    pub fn arrayproto_to_sorted(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut compare_fn = None;
        let arg = call.arg(0);
        if !arg.is_undefined() {
            if let Value::Object(f) = arg {
                compare_fn = f.assert_callable(self);
            }
            if compare_fn.is_none() {
                return Err(Throw::Value(self.new_type_error(
                    "The comparison function must be either a function or undefined",
                )));
            }
        }
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        if length >= i64::from(u32::MAX) {
            let e = self.get_range_error();
            return Err(Throw::Value(self.new_error(e, "Invalid array length")));
        }
        let a = if self.check_std_array_obj(o).is_some() {
            let mut a = go_make_cap(length);
            let src = std_values(self, o);
            let n = (length as usize).min(src.len());
            a.extend_from_slice(&src[..n]);
            a.resize(length as usize, Value::Empty);
            a
        } else {
            let mut a = go_make_cap(length);
            for i in 0..length {
                a.push(nil_safe(o.get_idx(self, i, None)?));
            }
            a
        };
        let ar = self.new_array_values(a);
        let ctx = ArraySortCtx {
            obj: ar,
            compare: compare_fn,
        };
        ctx.stable(self)?;
        Ok(Value::Object(ar))
    }

    pub fn arrayproto_to_spliced(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = call.this.to_object(self)?;
        let length = get_length(self, o)?;
        let actual_start = rel_to_idx(call.arg(0).to_integer(self)?, length);
        let mut actual_skip_count = 0i64;
        if call.arguments.len() == 1 {
            actual_skip_count = length - actual_start;
        } else if call.arguments.len() > 1 {
            actual_skip_count = call
                .arg(1)
                .to_integer(self)?
                .max(0)
                .min(length - actual_start);
        }
        let item_count = (call.arguments.len() as i64 - 2).max(0);
        let new_length = length - actual_skip_count + item_count;
        if new_length >= MAX_INT {
            return Err(Throw::Value(self.new_type_error("Invalid array length")));
        }
        if self.check_std_array_obj(o).is_some() {
            let s = actual_start as usize;
            let sc = actual_skip_count as usize;
            let ic = item_count as usize;
            let mut values = {
                let src = std_values(self, o);
                if item_count == actual_skip_count {
                    src.clone()
                } else {
                    let mut v = go_make_cap(new_length);
                    v.extend_from_slice(&src[..s]);
                    v.resize(s + ic, Value::Empty);
                    let rest = &src[s + sc..];
                    let n = rest.len().min(new_length as usize - (s + ic));
                    v.extend_from_slice(&rest[..n]);
                    v.resize(new_length as usize, Value::Empty);
                    v
                }
            };
            if item_count > 0 {
                for (j, item) in call.arguments[2..].iter().enumerate() {
                    values[s + j] = item.clone();
                }
            }
            return Ok(Value::Object(self.new_array_values(values)));
        }
        let a = self.new_array_length(new_length)?;
        let mut i = 0i64;
        let mut rl = actual_start + actual_skip_count;
        while i < actual_start {
            let i_value = nil_safe(o.get_idx(self, i, None)?);
            create_data_property_or_throw(self, a, &Value::Int(i), i_value)?;
            i += 1;
        }
        if item_count > 0 {
            for item in &call.arguments[2..] {
                create_data_property_or_throw(
                    self,
                    a,
                    &Value::Int(i),
                    nil_safe(Some(item.clone())),
                )?;
                i += 1;
            }
        }
        while i < new_length {
            let from_value = nil_safe(o.get_idx(self, rl, None)?);
            create_data_property_or_throw(self, a, &Value::Int(i), from_value)?;
            i += 1;
            rl += 1;
        }
        Ok(Value::Object(a))
    }

    pub fn check_std_array_obj(&self, obj: ObjRef) -> Option<ObjRef> {
        match &self.obj(obj).ext {
            ObjExt::Array(arr)
                if arr.prop_value_count == 0
                    && arr.length as usize == arr.values.len()
                    && arr.obj_count as u32 == arr.length =>
            {
                Some(obj)
            }
            _ => None,
        }
    }

    pub fn check_std_array_obj_with_proto(&mut self, obj: ObjRef) -> Option<ObjRef> {
        let arr = self.check_std_array_obj(obj)?;
        let Some(p1) = self.obj(arr).base.prototype else {
            panic!("array prototype chain is missing a prototype");
        };
        let ok1 = matches!(&self.obj(p1).ext, ObjExt::Array(a) if a.prop_value_count == 0);
        if !ok1 {
            return None;
        }
        let Some(p2) = self.obj(p1).base.prototype else {
            panic!("array prototype chain is missing a prototype");
        };
        let p2o = self.obj_mut(p2);
        if std::ptr::eq(p2o.class, &BASE_OBJECT_CLASS)
            && matches!(p2o.ext, ObjExt::None)
            && p2o.base.prototype.is_none()
        {
            p2o.base.ensure_prop_order();
            if p2o.base.idx_prop_count == 0 {
                return Some(arr);
            }
        }
        None
    }

    pub fn check_std_array(&self, v: &Value) -> Option<ObjRef> {
        if let Value::Object(obj) = v {
            return self.check_std_array_obj(*obj);
        }
        None
    }

    pub fn check_std_array_iter(&mut self, v: &Value) -> JsResult<Option<ObjRef>> {
        if let Some(arr) = self.check_std_array(v) {
            let it = arr.get_sym(self, SYM_ITERATOR, None)?;
            let values = self.get_array_values();
            if matches!(it, Some(Value::Object(f)) if f == values) {
                return Ok(Some(arr));
            }
        }
        Ok(None)
    }

    pub fn array_from(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut map_fn = None;
        let map_fn_arg = call.arg(1);
        if !map_fn_arg.is_undefined() {
            if let Value::Object(m) = map_fn_arg {
                map_fn = m.assert_callable(self);
            }
            if map_fn.is_none() {
                let s = map_fn_arg.fmt_string(self)?;
                return Err(Throw::Value(
                    self.new_type_error(&format!("{s} is not a function")),
                ));
            }
        }
        let t = call.argument(2);
        let items = call.argument(0);
        let this_is_array = matches!(call.this, Value::Object(o) if Some(*o) == self.global.array);
        if map_fn.is_none()
            && this_is_array
            && let Some(arr) = self.check_std_array_iter(&items)?
        {
            let items = std_values(self, arr).clone();
            return Ok(Value::Object(self.new_array_values(items)));
        }
        let mut ctor = None;
        if !this_is_array
            && let Value::Object(o) = call.this
            && let Some(c) = o.assert_constructor(self)
        {
            ctor = Some(c);
        }
        let it = self.get_v(&items, &Value::Symbol(SYM_ITERATOR))?;
        let arr;
        if let Some(using_iterator) = to_method(self, it)? {
            arr = match ctor {
                Some(c) => c.construct(self, &[], None)?,
                None => self.new_array_values(Vec::new()),
            };
            let mut iter = self.get_iterator(&items, Some(using_iterator))?;
            if map_fn.is_none()
                && let Some(a) = self.check_std_array_obj_with_proto(arr)
            {
                let mut values = Vec::new();
                iter.iterate(self, &mut |_rt, val| {
                    go_append(&mut values, val);
                    Ok(())
                })?;
                set_array_values(self, a, values);
                return Ok(Value::Object(arr));
            }
            let mut k = 0i64;
            iter.iterate(self, &mut |rt, val| {
                let val = match map_fn {
                    Some(m) => m.call(rt, t.clone(), &[val, int_to_value(k)])?,
                    None => val,
                };
                create_data_property_or_throw_opt(rt, arr, &int_to_value(k), val)?;
                k += 1;
                Ok(())
            })?;
            arr.set_own_str(self, &STR_LENGTH, int_to_value(k), true)?;
        } else {
            let array_like = items.to_object(self)?;
            let l = get_length(self, array_like)?;
            arr = match ctor {
                Some(c) => c.construct(self, &[int_to_value(l)], None)?,
                None => self.new_array_values(Vec::new()),
            };
            if map_fn.is_none()
                && let Some(a) = self.check_std_array_obj_with_proto(arr)
            {
                let mut values = go_make_len(l);
                for k in 0..l {
                    values[k as usize] = nil_safe(array_like.get_idx(self, k, None)?);
                }
                set_array_values(self, a, values);
                return Ok(Value::Object(arr));
            }
            for k in 0..l {
                let idx = Value::Int(k);
                let mut item = array_like.get_idx(self, k, None)?.unwrap_or(Value::Empty);
                match map_fn {
                    Some(m) => item = m.call(self, t.clone(), &[item, idx.clone()])?,
                    None => item = nil_safe(Some(item)),
                }
                create_data_property_or_throw_opt(self, arr, &idx, item)?;
            }
            arr.set_own_str(self, &STR_LENGTH, int_to_value(l), true)?;
        }
        Ok(Value::Object(arr))
    }

    pub fn array_is_array(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(o) = call.arg(0)
            && is_array(self, *o)?
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn array_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let mut ctor = None;
        let this_is_array = matches!(call.this, Value::Object(o) if Some(*o) == self.global.array);
        if !this_is_array
            && let Value::Object(o) = call.this
            && let Some(c) = o.assert_constructor(self)
        {
            ctor = Some(c);
        }
        let Some(ctor) = ctor else {
            let values = call.arguments.to_vec();
            return Ok(Value::Object(self.new_array_values(values)));
        };
        let l = int_to_value(call.arguments.len() as i64);
        let arr = ctor.construct(self, std::slice::from_ref(&l), None)?;
        for (i, val) in call.arguments.iter().enumerate() {
            create_data_property_or_throw_opt(self, arr, &int_to_value(i as i64), val.clone())?;
        }
        arr.set_own_str(self, &STR_LENGTH, l, true)?;
        Ok(Value::Object(arr))
    }

    pub fn array_iter_proto_next(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let this_obj = self.to_object(call.this)?;
        if self.ext_dyn::<ArrayIterObject>(this_obj).is_some() {
            return ArrayIterObject::next(self, this_obj);
        }
        let s = self.objectproto_to_string(FunctionCall::new(&Value::Object(this_obj), &[]))?;
        let s = s.go_string(self)?;
        Err(Throw::Value(self.new_type_error(&format!(
            "Method Array Iterator.prototype.next called on incompatible receiver {s}"
        ))))
    }

    pub fn get_array_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_prototype = Some(ret);
        self.new_templated_array_object(get_array_proto_template(), Some(ret));
        ret
    }

    pub fn get_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array = Some(ret);
        self.create_array(ret);
        ret
    }

    pub fn create_array(&mut self, val: ObjRef) {
        let proto = self.get_array_prototype();
        let o = self.new_native_func_construct_obj(
            val,
            |r, args, proto| r.builtin_new_array(args, proto),
            "Array",
            Some(proto),
            1,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.array_from(c)), "from", 1);
        o._put_prop(self, &"from".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.array_is_array(c)), "isArray", 1);
        o._put_prop(self, &"isArray".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.array_of(c)), "of", 0);
        o._put_prop(self, &"of".into(), Value::Object(f), true, false, true);
        self.put_species_return_this(o);
    }

    pub fn create_array_iter_proto(&mut self, val: ObjRef) {
        let proto = self.get_iterator_prototype();
        self.new_base_object_obj(val, Some(proto), CLASS_OBJECT);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.array_iter_proto_next(c)), "next", 0);
        val._put_prop(self, &"next".into(), Value::Object(f), true, false, true);
        val._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string(CLASS_ARRAY_ITERATOR)),
                false,
                false,
                true,
            ),
        );
    }

    pub fn get_array_values(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_values {
            return r;
        }
        let ret = self.new_native_func(NativeFn::Fn(|r, c| r.arrayproto_values(c)), "values", 0);
        self.global.array_values = Some(ret);
        ret
    }

    pub fn get_array_to_string(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_to_string {
            return r;
        }
        let ret = self.new_native_func(
            NativeFn::Fn(|r, c| r.arrayproto_to_string(c)),
            "toString",
            0,
        );
        self.global.array_to_string = Some(ret);
        ret
    }

    pub fn get_array_iterator_prototype(&mut self) -> ObjRef {
        if let Some(o) = self.global.array_iterator_prototype {
            return o;
        }
        let o = self.reserve_object();
        self.global.array_iterator_prototype = Some(o);
        self.create_array_iter_proto(o);
        o
    }
}

fn is_concat_spreadable(rt: &mut Runtime, obj: ObjRef) -> JsResult<bool> {
    let spreadable = obj.get_sym(rt, SYM_IS_CONCAT_SPREADABLE, None)?;
    if let Some(s) = spreadable
        && !s.is_undefined()
        && !s.is_empty()
    {
        return Ok(s.to_boolean());
    }
    is_array(rt, obj)
}

fn create_array_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("length", |_| value_prop(Value::Int(0), true, false, false));

    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_array()), true, false, true)
    });

    t.put_str("at", |r| r.method_prop(|r, c| r.arrayproto_at(c), "at", 1));
    t.put_str("concat", |r| {
        r.method_prop(|r, c| r.arrayproto_concat(c), "concat", 1)
    });
    t.put_str("copyWithin", |r| {
        r.method_prop(|r, c| r.arrayproto_copy_within(c), "copyWithin", 2)
    });
    t.put_str("entries", |r| {
        r.method_prop(|r, c| r.arrayproto_entries(c), "entries", 0)
    });
    t.put_str("every", |r| {
        r.method_prop(|r, c| r.arrayproto_every(c), "every", 1)
    });
    t.put_str("fill", |r| {
        r.method_prop(|r, c| r.arrayproto_fill(c), "fill", 1)
    });
    t.put_str("filter", |r| {
        r.method_prop(|r, c| r.arrayproto_filter(c), "filter", 1)
    });
    t.put_str("find", |r| {
        r.method_prop(|r, c| r.arrayproto_find(c), "find", 1)
    });
    t.put_str("findIndex", |r| {
        r.method_prop(|r, c| r.arrayproto_find_index(c), "findIndex", 1)
    });
    t.put_str("findLast", |r| {
        r.method_prop(|r, c| r.arrayproto_find_last(c), "findLast", 1)
    });
    t.put_str("findLastIndex", |r| {
        r.method_prop(|r, c| r.arrayproto_find_last_index(c), "findLastIndex", 1)
    });
    t.put_str("flat", |r| {
        r.method_prop(|r, c| r.arrayproto_flat(c), "flat", 0)
    });
    t.put_str("flatMap", |r| {
        r.method_prop(|r, c| r.arrayproto_flat_map(c), "flatMap", 1)
    });
    t.put_str("forEach", |r| {
        r.method_prop(|r, c| r.arrayproto_for_each(c), "forEach", 1)
    });
    t.put_str("includes", |r| {
        r.method_prop(|r, c| r.arrayproto_includes(c), "includes", 1)
    });
    t.put_str("indexOf", |r| {
        r.method_prop(|r, c| r.arrayproto_index_of(c), "indexOf", 1)
    });
    t.put_str("join", |r| {
        r.method_prop(|r, c| r.arrayproto_join(c), "join", 1)
    });
    t.put_str("keys", |r| {
        r.method_prop(|r, c| r.arrayproto_keys(c), "keys", 0)
    });
    t.put_str("lastIndexOf", |r| {
        r.method_prop(|r, c| r.arrayproto_last_index_of(c), "lastIndexOf", 1)
    });
    t.put_str("map", |r| {
        r.method_prop(|r, c| r.arrayproto_map(c), "map", 1)
    });
    t.put_str("pop", |r| {
        r.method_prop(|r, c| r.arrayproto_pop(c), "pop", 0)
    });
    t.put_str("push", |r| {
        r.method_prop(|r, c| r.arrayproto_push(c), "push", 1)
    });
    t.put_str("reduce", |r| {
        r.method_prop(|r, c| r.arrayproto_reduce(c), "reduce", 1)
    });
    t.put_str("reduceRight", |r| {
        r.method_prop(|r, c| r.arrayproto_reduce_right(c), "reduceRight", 1)
    });
    t.put_str("reverse", |r| {
        r.method_prop(|r, c| r.arrayproto_reverse(c), "reverse", 0)
    });
    t.put_str("shift", |r| {
        r.method_prop(|r, c| r.arrayproto_shift(c), "shift", 0)
    });
    t.put_str("slice", |r| {
        r.method_prop(|r, c| r.arrayproto_slice(c), "slice", 2)
    });
    t.put_str("some", |r| {
        r.method_prop(|r, c| r.arrayproto_some(c), "some", 1)
    });
    t.put_str("sort", |r| {
        r.method_prop(|r, c| r.arrayproto_sort(c), "sort", 1)
    });
    t.put_str("splice", |r| {
        r.method_prop(|r, c| r.arrayproto_splice(c), "splice", 2)
    });
    t.put_str("toLocaleString", |r| {
        r.method_prop(|r, c| r.arrayproto_to_locale_string(c), "toLocaleString", 0)
    });
    t.put_str("toString", |r| {
        value_prop(Value::Object(r.get_array_to_string()), true, false, true)
    });
    t.put_str("unshift", |r| {
        r.method_prop(|r, c| r.arrayproto_unshift(c), "unshift", 1)
    });
    t.put_str("with", |r| {
        r.method_prop(|r, c| r.arrayproto_with(c), "with", 2)
    });
    t.put_str("toReversed", |r| {
        r.method_prop(|r, c| r.arrayproto_to_reversed(c), "toReversed", 0)
    });
    t.put_str("toSorted", |r| {
        r.method_prop(|r, c| r.arrayproto_to_sorted(c), "toSorted", 1)
    });
    t.put_str("toSpliced", |r| {
        r.method_prop(|r, c| r.arrayproto_to_spliced(c), "toSpliced", 2)
    });
    t.put_str("values", |r| {
        value_prop(Value::Object(r.get_array_values()), true, false, true)
    });

    t.put_sym(SYM_ITERATOR, |r| {
        value_prop(Value::Object(r.get_array_values()), true, false, true)
    });
    t.put_sym(SYM_UNSCOPABLES, |r| {
        let bl = r.new_base_object(None, CLASS_OBJECT);
        for name in [
            "copyWithin",
            "entries",
            "fill",
            "find",
            "findIndex",
            "findLast",
            "findLastIndex",
            "flat",
            "flatMap",
            "includes",
            "keys",
            "values",
            "groupBy",
            "groupByToMap",
            "toReversed",
            "toSorted",
            "toSpliced",
        ] {
            let _ = bl.set_own_str(r, &name.into(), Value::Bool(true), true);
        }
        value_prop(Value::Object(bl), false, false, true)
    });

    t
}

pub fn get_array_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_array_proto_template)
}

pub struct ArraySortCtx {
    pub obj: ObjRef,
    pub compare: Option<CallFn>,
}

impl ArraySortCtx {
    pub fn sort_compare(&self, rt: &mut Runtime, x: Value, y: Value) -> JsResult<i32> {
        if x.is_empty() && y.is_empty() {
            return Ok(0);
        }
        if x.is_empty() {
            return Ok(1);
        }
        if y.is_empty() {
            return Ok(-1);
        }
        if x.is_undefined() && y.is_undefined() {
            return Ok(0);
        }
        if x.is_undefined() {
            return Ok(1);
        }
        if y.is_undefined() {
            return Ok(-1);
        }
        if let Some(compare) = self.compare {
            let f = compare.call(rt, Value::Undefined, &[x, y])?.to_float(rt)?;
            if f > 0.0 {
                return Ok(1);
            }
            if f < 0.0 {
                return Ok(-1);
            }
            if f.is_sign_negative() {
                return Ok(-1);
            }
            return Ok(0);
        }
        let (mut bx, mut by) = ([0u8; 20], [0u8; 20]);
        if let (Some(kx), Some(ky)) = (ascii_sort_key(&x, &mut bx), ascii_sort_key(&y, &mut by)) {
            return Ok(match kx.cmp(ky) {
                std::cmp::Ordering::Less => -1,
                std::cmp::Ordering::Equal => 0,
                std::cmp::Ordering::Greater => 1,
            });
        }
        let xs = x.to_string(rt)?;
        let ys = y.to_string(rt)?;
        Ok(xs.compare_to(&ys))
    }

    pub fn len(&self, rt: &mut Runtime) -> JsResult<usize> {
        Ok(self.obj.sort_len(rt)? as usize)
    }

    pub fn less(&self, rt: &mut Runtime, j: usize, k: usize) -> JsResult<bool> {
        let mark = rt.handle_scope_enter();
        let x = self.obj.sort_get(rt, j as i64)?;
        let y = self.obj.sort_get(rt, k as i64)?;
        let r = self.sort_compare(rt, x, y);
        rt.handle_scope_leave(mark);
        Ok(r? < 0)
    }

    pub fn swap(&self, rt: &mut Runtime, j: usize, k: usize) -> JsResult<()> {
        self.obj.swap(rt, j as i64, k as i64)
    }

    pub fn stable(&self, rt: &mut Runtime) -> JsResult<()> {
        let n = self.len(rt)?;
        let mut block_size = 20;
        let (mut a, mut b) = (0, block_size);
        while b <= n {
            self.insertion_sort(rt, a, b)?;
            a = b;
            b += block_size;
        }
        self.insertion_sort(rt, a, n)?;
        while block_size < n {
            a = 0;
            b = 2 * block_size;
            while b <= n {
                self.sym_merge(rt, a, a + block_size, b)?;
                a = b;
                b += 2 * block_size;
            }
            let m = a + block_size;
            if m < n {
                self.sym_merge(rt, a, m, n)?;
            }
            block_size *= 2;
        }
        Ok(())
    }

    fn insertion_sort(&self, rt: &mut Runtime, a: usize, b: usize) -> JsResult<()> {
        let mut i = a + 1;
        while i < b {
            let mut j = i;
            while j > a && self.less(rt, j, j - 1)? {
                self.swap(rt, j, j - 1)?;
                j -= 1;
            }
            i += 1;
        }
        Ok(())
    }

    fn sym_merge(&self, rt: &mut Runtime, a: usize, m: usize, b: usize) -> JsResult<()> {
        if m - a == 1 {
            let mut i = m;
            let mut j = b;
            while i < j {
                let h = (i + j) >> 1;
                if self.less(rt, h, a)? {
                    i = h + 1;
                } else {
                    j = h;
                }
            }
            let mut k = a;
            while k + 1 < i {
                self.swap(rt, k, k + 1)?;
                k += 1;
            }
            return Ok(());
        }
        if b - m == 1 {
            let mut i = a;
            let mut j = m;
            while i < j {
                let h = (i + j) >> 1;
                if !self.less(rt, m, h)? {
                    i = h + 1;
                } else {
                    j = h;
                }
            }
            let mut k = m;
            while k > i {
                self.swap(rt, k, k - 1)?;
                k -= 1;
            }
            return Ok(());
        }
        let mid = (a + b) >> 1;
        let n = mid + m;
        let (mut start, mut r) = if m > mid { (n - b, mid) } else { (a, m) };
        let p = n - 1;
        while start < r {
            let c = (start + r) >> 1;
            if !self.less(rt, p - c, c)? {
                start = c + 1;
            } else {
                r = c;
            }
        }
        let end = n - start;
        if start < m && m < end {
            self.rotate(rt, start, m, end)?;
        }
        if a < start && start < mid {
            self.sym_merge(rt, a, start, mid)?;
        }
        if mid < end && end < b {
            self.sym_merge(rt, mid, end, b)?;
        }
        Ok(())
    }

    fn swap_range(&self, rt: &mut Runtime, a: usize, b: usize, n: usize) -> JsResult<()> {
        for i in 0..n {
            self.swap(rt, a + i, b + i)?;
        }
        Ok(())
    }

    fn rotate(&self, rt: &mut Runtime, a: usize, m: usize, b: usize) -> JsResult<()> {
        let mut i = m - a;
        let mut j = b - m;
        while i != j {
            if i > j {
                self.swap_range(rt, m - i, m, j)?;
                i -= j;
            } else {
                self.swap_range(rt, m - i, m + j - i, i)?;
                j -= i;
            }
        }
        self.swap_range(rt, m - i, m, i)
    }
}
