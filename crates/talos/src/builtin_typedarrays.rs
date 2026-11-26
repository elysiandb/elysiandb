use crate::builtin_array::rel_to_idx;
use crate::builtin_bigint::to_big_int;
use crate::builtin_symbol::{SYM_ITERATOR, SYM_TO_STRING_TAG};
use crate::func::{NativeCtor, NativeFn};
use crate::heap::ObjRef;
use crate::object::{CLASS_OBJECT, CallFn, FunctionCall, to_method};
use crate::object_template::ObjectTemplate;
use crate::runtime::{
    IterationKind, JsResult, Runtime, Throw, to_float32, to_int_strict, to_length,
};
use crate::string::{STRING_EMPTY, ascii_string};
use crate::string_unicode::StringBuilder;
use crate::typedarrays::{
    ByteBuf, TypedArray, TypedArrayObjectCtor, TypedKind, ensure_not_detached, to_big_int64,
    to_big_uint64,
};
use crate::value::{
    ERR_MIX_BIG_INT_TYPE, Value, ValueProperty, float_to_value, int_to_value, new_big_int,
    nil_safe, value_prop,
};
use std::rc::Rc;
use std::sync::OnceLock;

const MAX_ALLOC: i64 = 1 << 48;

pub trait StableSort {
    fn len(&self) -> usize;

    fn less(&mut self, rt: &mut Runtime, i: usize, j: usize) -> JsResult<bool>;

    fn swap(&mut self, i: usize, j: usize);

    fn is_empty(&self) -> bool {
        self.len() == 0
    }

    fn stable(&mut self, rt: &mut Runtime) -> JsResult<()> {
        let n = self.len();
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

    fn insertion_sort(&mut self, rt: &mut Runtime, a: usize, b: usize) -> JsResult<()> {
        let mut i = a + 1;
        while i < b {
            let mut j = i;
            while j > a && self.less(rt, j, j - 1)? {
                self.swap(j, j - 1);
                j -= 1;
            }
            i += 1;
        }
        Ok(())
    }

    fn sym_merge(&mut self, rt: &mut Runtime, a: usize, m: usize, b: usize) -> JsResult<()> {
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
                self.swap(k, k + 1);
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
                self.swap(k, k - 1);
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
            self.rotate(start, m, end);
        }
        if a < start && start < mid {
            self.sym_merge(rt, a, start, mid)?;
        }
        if mid < end && end < b {
            self.sym_merge(rt, mid, end, b)?;
        }
        Ok(())
    }

    fn swap_range(&mut self, a: usize, b: usize, n: usize) {
        for i in 0..n {
            self.swap(a + i, b + i);
        }
    }

    fn rotate(&mut self, a: usize, m: usize, b: usize) {
        let mut i = m - a;
        let mut j = b - m;
        while i != j {
            if i > j {
                self.swap_range(m - i, m, j);
                i -= j;
            } else {
                self.swap_range(m - i, m + j - i, i);
                j -= i;
            }
        }
        self.swap_range(m - i, m, i);
    }
}

pub struct TypedArraySortCtx {
    pub ta: TypedArray,
    pub compare: Option<CallFn>,
    pub need_validate: bool,
    pub detached: bool,
}

impl TypedArraySortCtx {
    fn check_detached(&mut self) {
        if !self.detached && self.need_validate {
            self.detached = self.ta.data.borrow().detached;
            self.need_validate = false;
        }
    }
}

impl StableSort for TypedArraySortCtx {
    fn len(&self) -> usize {
        self.ta.length as usize
    }

    fn less(&mut self, rt: &mut Runtime, i: usize, j: usize) -> JsResult<bool> {
        self.check_detached();
        if self.detached {
            return Ok(false);
        }
        let offset = self.ta.offset;
        if let Some(compare) = self.compare {
            let x = self.ta.get(offset + i as i64);
            let y = self.ta.get(offset + j as i64);
            let mark = rt.handle_scope_enter();
            let res = compare.call(rt, Value::Undefined, &[x, y])?.to_number(rt)?;
            rt.handle_scope_leave(mark);
            self.need_validate = true;
            if let Value::Int(i) = res {
                return Ok(i < 0);
            }
            let f = res.prim_to_float();
            if f < 0.0 {
                return Ok(true);
            }
            if f > 0.0 {
                return Ok(false);
            }
            if f.is_sign_negative() {
                return Ok(true);
            }
            return Ok(false);
        }
        Ok(self.ta.less(offset + i as i64, offset + j as i64))
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.check_detached();
        if self.detached {
            return;
        }
        let offset = self.ta.offset;
        self.ta.swap(offset + i as i64, offset + j as i64);
    }
}

pub struct RawTypedArraySort<'a> {
    pub kind: TypedKind,
    pub bytes: &'a mut [u8],
    pub offset: usize,
    pub n: usize,
}

impl StableSort for RawTypedArraySort<'_> {
    fn len(&self) -> usize {
        self.n
    }

    fn less(&mut self, _rt: &mut Runtime, i: usize, j: usize) -> JsResult<bool> {
        Ok(self.kind.less(self.bytes, self.offset + i, self.offset + j))
    }

    fn swap(&mut self, i: usize, j: usize) {
        self.kind.swap(self.bytes, self.offset + i, self.offset + j);
    }
}

fn sort_typed_array(rt: &mut Runtime, ta: TypedArray, compare: Option<CallFn>) -> JsResult<()> {
    if compare.is_none() {
        let data = ta.data.clone();
        let mut b = data.borrow_mut();
        if !b.detached {
            let mut s = RawTypedArraySort {
                kind: ta.kind,
                bytes: b.bytes_mut(),
                offset: ta.offset as usize,
                n: ta.length as usize,
            };
            return s.stable(rt);
        }
    }
    let mut ctx = TypedArraySortCtx {
        ta,
        compare,
        need_validate: false,
        detached: false,
    };
    ctx.stable(rt)
}

pub fn alloc_byte_slice(size: i64) -> JsResult<Vec<u8>> {
    if !(0..=MAX_ALLOC).contains(&size) {
        return Err(Throw::RangeError(format!(
            "Buffer size is too large: {size}"
        )));
    }
    Ok(vec![0u8; size as usize])
}

fn is_nan(v: &Value) -> bool {
    matches!(v, Value::Float(f) if f.is_nan())
}

fn normalize_zero(v: Value) -> Value {
    match v {
        Value::Float(0.0) => Value::Int(0),
        v => v,
    }
}

fn go_fmt_value(rt: &mut Runtime, v: Option<Value>) -> JsResult<String> {
    match v {
        Some(v) if !v.is_empty() => v.fmt_string(rt),
        _ => Ok("%!s(<nil>)".to_string()),
    }
}

impl Runtime {
    fn incompatible_receiver_error(&mut self, method: &str, this: &Value) -> Throw {
        match self.objectproto_to_string(FunctionCall::new(this, &[])) {
            Ok(s) => {
                let s = s.prim_to_string().to_rust_string();
                Throw::Value(self.new_type_error(&format!(
                    "Method {method} called on incompatible receiver {s}"
                )))
            }
            Err(e) => e,
        }
    }

    fn this_typed_array(&mut self, this: &Value, method: &str) -> JsResult<TypedArray> {
        let o = self.to_object(this)?;
        if let Some(ta) = self.typed_array(o) {
            return Ok(ta);
        }
        Err(self.incompatible_receiver_error(method, this))
    }

    fn this_valid_typed_array(&mut self, this: &Value) -> JsResult<TypedArray> {
        let o = this.to_object(self)?;
        if let Some(ta) = self.typed_array(o) {
            return Ok(ta);
        }
        let s = match self.objectproto_to_string(FunctionCall::new(this, &[])) {
            Ok(s) => s.prim_to_string().to_rust_string(),
            Err(e) => return Err(e),
        };
        Err(Throw::Value(
            self.new_type_error(&format!("{s} is not a valid TypedArray")),
        ))
    }

    fn this_data_view(&mut self, this: &Value, method: &str) -> JsResult<ObjRef> {
        let o = self.to_object(this)?;
        if self.data_view(o).is_some() {
            return Ok(o);
        }
        Err(self.incompatible_receiver_error(method, this))
    }

    fn object_is_not_array_buffer(&mut self, o: ObjRef) -> Throw {
        match Value::Object(o).fmt_string(self) {
            Ok(s) => Throw::Value(self.new_type_error(&format!("Object is not ArrayBuffer: {s}"))),
            Err(e) => e,
        }
    }

    fn convert_copy(
        &mut self,
        src: &TypedArray,
        dst: &TypedArray,
        src_offset: i64,
        dst_offset: i64,
        segs: &[(i64, i64, bool)],
    ) -> JsResult<()> {
        let (sk, dk) = (src.kind, dst.kind);
        let pure = sk.is_big_int() == dk.is_big_int();
        if Rc::ptr_eq(&src.data, &dst.data) {
            let data = dst.data.clone();
            let mut b = data.borrow_mut();
            if pure {
                let bytes = b.bytes_mut();
                for &(lo, hi, fwd) in segs {
                    for k in 0..(hi - lo).max(0) {
                        let i = if fwd { lo + k } else { hi - 1 - k };
                        let n = sk.get_num(bytes, (src_offset + i) as usize);
                        dk.set_raw(bytes, (dst_offset + i) as usize, dk.num_to_bits(n));
                    }
                }
                return Ok(());
            }
            for &(lo, hi, fwd) in segs {
                for k in 0..(hi - lo).max(0) {
                    let i = if fwd { lo + k } else { hi - 1 - k };
                    let v = sk.get(b.bytes(), (src_offset + i) as usize);
                    let bits = dk.set_bits(self, &v)?;
                    dk.set_raw(b.bytes_mut(), (dst_offset + i) as usize, bits);
                }
            }
            return Ok(());
        }
        let (sdata, ddata) = (src.data.clone(), dst.data.clone());
        let sb = sdata.borrow();
        let mut db = ddata.borrow_mut();
        if pure {
            let (s, d) = (sb.bytes(), db.bytes_mut());
            for &(lo, hi, fwd) in segs {
                for k in 0..(hi - lo).max(0) {
                    let i = if fwd { lo + k } else { hi - 1 - k };
                    let n = sk.get_num(s, (src_offset + i) as usize);
                    dk.set_raw(d, (dst_offset + i) as usize, dk.num_to_bits(n));
                }
            }
            return Ok(());
        }
        for &(lo, hi, fwd) in segs {
            for k in 0..(hi - lo).max(0) {
                let i = if fwd { lo + k } else { hi - 1 - k };
                let v = sk.get(sb.bytes(), (src_offset + i) as usize);
                let bits = dk.set_bits(self, &v)?;
                dk.set_raw(db.bytes_mut(), (dst_offset + i) as usize, bits);
            }
        }
        Ok(())
    }

    fn range_error(&mut self, msg: &str) -> Throw {
        let e = self.get_range_error();
        Throw::Value(self.new_error(e, msg))
    }

    pub fn builtin_new_array_buffer(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("ArrayBuffer")));
        };
        let ctor = self.get_array_buffer();
        let def_proto = self.get_array_buffer_prototype();
        let proto = self.get_prototype_from_ctor(new_target, Some(ctor), Some(def_proto))?;
        let b = self._new_array_buffer(proto, None);
        if let Some(a) = args.first() {
            let size = self.to_index(a)?;
            let data = alloc_byte_slice(size)?;
            *self.array_buffer_shared(b).borrow_mut() = ByteBuf::from_vec(data);
        }
        Ok(b)
    }

    pub fn array_buffer_proto_get_byte_length(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let o = self.to_object(call.this)?;
        if let Some(data) = self.array_buffer_data(o) {
            if ensure_not_detached(self, &data, false)? {
                return Ok(int_to_value(data.borrow().len() as i64));
            }
            return Ok(int_to_value(0));
        }
        Err(self.object_is_not_array_buffer(o))
    }

    pub fn array_buffer_proto_slice(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.to_object(call.this)?;
        let Some(b) = self.array_buffer_data(o) else {
            return Err(self.object_is_not_array_buffer(o));
        };
        let l = b.borrow().len() as i64;
        let start = rel_to_idx(call.arg(0).to_integer(self)?, l);
        let stop = match call.arg(1) {
            Value::Undefined => l,
            arg => arg.to_integer(self)?,
        };
        let stop = rel_to_idx(stop, l);
        let new_len = (stop - start).max(0);
        let ab_ctor = self.get_array_buffer();
        let c = self.species_constructor(o, ab_ctor)?;
        let ret = c.construct(self, &[int_to_value(new_len)], None)?;
        if let Some(ab) = self.array_buffer_data(ret) {
            if new_len > 0 {
                ensure_not_detached(self, &b, true)?;
                if ret == o {
                    return Err(Throw::Value(self.new_type_error(
                        "Species constructor returned the same ArrayBuffer",
                    )));
                }
                let ab_len = ab.borrow().len() as i64;
                if ab_len < new_len {
                    return Err(Throw::Value(self.new_type_error(&format!(
                        "Species constructor returned an ArrayBuffer that is too small: {ab_len}"
                    ))));
                }
                ensure_not_detached(self, &ab, true)?;
                let (s, e) = (start as usize, stop as usize);
                if Rc::ptr_eq(&ab, &b) {
                    let mut d = ab.borrow_mut();
                    let n = (e - s).min(d.len());
                    d.bytes_mut().copy_within(s..s + n, 0);
                } else {
                    let src = b.borrow();
                    let mut dst = ab.borrow_mut();
                    let dst = dst.bytes_mut();
                    let n = (e - s).min(dst.len());
                    dst[..n].copy_from_slice(&src.bytes()[s..s + n]);
                }
            }
            return Ok(Value::Object(ret));
        }
        let s = Value::Object(ret).go_string(self)?;
        Err(Throw::Value(self.new_type_error(&format!(
            "Species constructor did not return an ArrayBuffer: {s}"
        ))))
    }

    pub fn array_buffer_is_view(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(o) = call.arg(0)
            && (self.data_view(*o).is_some() || self.is_typed_array(*o))
        {
            return Ok(Value::Bool(true));
        }
        Ok(Value::Bool(false))
    }

    pub fn new_data_view(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("DataView")));
        };
        let buffer = match args.first() {
            Some(Value::Object(o)) if self.array_buffer_data(*o).is_some() => *o,
            _ => {
                return Err(Throw::Value(self.new_type_error(
                    "First argument to DataView constructor must be an ArrayBuffer",
                )));
            }
        };
        let data = self.array_buffer_shared(buffer);
        let mut byte_offset = 0i64;
        if args.len() > 1 {
            let offset_arg = if args[1].is_empty() {
                Value::Undefined
            } else {
                args[1].clone()
            };
            byte_offset = self.to_index(&offset_arg)?;
            ensure_not_detached(self, &data, true)?;
            if byte_offset > data.borrow().len() as i64 {
                let s = offset_arg.go_string(self)?;
                return Err(self.range_error(&format!(
                    "Start offset {s} is outside the bounds of the buffer"
                )));
            }
        }
        let byte_len;
        if args.len() > 2 && !args[2].is_empty() && !args[2].is_undefined() {
            byte_len = self.to_index(&args[2])?;
            if byte_offset + byte_len > data.borrow().len() as i64 {
                return Err(self.range_error(&format!("Invalid DataView length {byte_len}")));
            }
        } else {
            byte_len = data.borrow().len() as i64 - byte_offset;
        }
        let ctor = self.get_data_view();
        let def_proto = self.get_data_view_prototype();
        let proto = self.get_prototype_from_ctor(new_target, Some(ctor), Some(def_proto))?;
        ensure_not_detached(self, &data, true)?;
        if byte_offset > data.borrow().len() as i64 {
            return Err(self.range_error(&format!(
                "Start offset {byte_offset} is outside the bounds of the buffer"
            )));
        }
        if byte_offset + byte_len > data.borrow().len() as i64 {
            return Err(self.range_error(&format!("Invalid DataView length {byte_len}")));
        }
        Ok(self.new_data_view_object(proto, buffer, byte_offset, byte_len))
    }

    pub fn data_view_proto_get_buffer(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.this_data_view(call.this, "get DataView.prototype.buffer")?;
        let (buf, ..) = self.data_view(o).expect("data view");
        Ok(Value::Object(buf))
    }

    pub fn data_view_proto_get_byte_len(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.this_data_view(call.this, "get DataView.prototype.byteLength")?;
        let (_, data, byte_len, _) = self.data_view(o).expect("data view");
        ensure_not_detached(self, &data, true)?;
        Ok(int_to_value(byte_len))
    }

    pub fn data_view_proto_get_byte_offset(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.this_data_view(call.this, "get DataView.prototype.byteOffset")?;
        let (_, data, _, byte_offset) = self.data_view(o).expect("data view");
        ensure_not_detached(self, &data, true)?;
        Ok(int_to_value(byte_offset))
    }

    fn data_view_get(
        &mut self,
        call: &FunctionCall<'_>,
        method: &str,
        idx_arg: Value,
        size: i64,
    ) -> JsResult<(ObjRef, crate::typedarrays::DataViewIdx)> {
        let o = self.this_data_view(call.this, method)?;
        let idx = self.to_index(&idx_arg)?;
        let p = self.data_view_get_idx_and_byte_order(o, idx, Some(call.arg(1)), size)?;
        Ok((o, p))
    }

    fn data_view_bytes(&self, o: ObjRef) -> crate::typedarrays::SharedBytes {
        self.data_view(o).expect("data view").1
    }

    pub fn data_view_proto_get_float32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getFloat32", call.argument(0), 4)?;
        let v = self.data_view_bytes(o).borrow().get_float32(p.idx, p.bo);
        Ok(float_to_value(f64::from(v)))
    }

    pub fn data_view_proto_get_float64(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getFloat64", call.argument(0), 8)?;
        let v = self.data_view_bytes(o).borrow().get_float64(p.idx, p.bo);
        Ok(float_to_value(v))
    }

    pub fn data_view_proto_get_int8(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getInt8", call.argument(0), 1)?;
        let v = self.data_view_bytes(o).borrow().get_int8(p.idx);
        Ok(int_to_value(i64::from(v)))
    }

    pub fn data_view_proto_get_int16(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getInt16", call.argument(0), 2)?;
        let v = self.data_view_bytes(o).borrow().get_int16(p.idx, p.bo);
        Ok(int_to_value(i64::from(v)))
    }

    pub fn data_view_proto_get_int32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getInt32", call.argument(0), 4)?;
        let v = self.data_view_bytes(o).borrow().get_int32(p.idx, p.bo);
        Ok(int_to_value(i64::from(v)))
    }

    pub fn data_view_proto_get_uint8(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getUint8", call.argument(0), 1)?;
        let v = self.data_view_bytes(o).borrow().get_uint8(p.idx);
        Ok(int_to_value(i64::from(v)))
    }

    pub fn data_view_proto_get_uint16(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getUint16", call.argument(0), 2)?;
        let v = self.data_view_bytes(o).borrow().get_uint16(p.idx, p.bo);
        Ok(int_to_value(i64::from(v)))
    }

    pub fn data_view_proto_get_uint32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, p) =
            self.data_view_get(&call, "DataView.prototype.getUint32", call.argument(0), 4)?;
        let v = self.data_view_bytes(o).borrow().get_uint32(p.idx, p.bo);
        Ok(int_to_value(i64::from(v)))
    }

    pub fn data_view_proto_get_big_int64(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.this_data_view(call.this, "DataView.prototype.getBigInt64")?;
        let n = call.arg(0).to_number(self)?;
        let idx = self.to_index(&n)?;
        let p = self.data_view_get_idx_and_byte_order(o, idx, Some(call.arg(1)), 8)?;
        let v = self.data_view_bytes(o).borrow().get_big_int64(p.idx, p.bo);
        Ok(new_big_int(v))
    }

    pub fn data_view_proto_get_big_uint64(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.this_data_view(call.this, "DataView.prototype.getBigUint64")?;
        let n = call.arg(0).to_number(self)?;
        let idx = self.to_index(&n)?;
        let p = self.data_view_get_idx_and_byte_order(o, idx, Some(call.arg(1)), 8)?;
        let v = self.data_view_bytes(o).borrow().get_big_uint64(p.idx, p.bo);
        Ok(new_big_int(v))
    }

    fn data_view_set_target(
        &mut self,
        call: &FunctionCall<'_>,
        method: &str,
    ) -> JsResult<(ObjRef, i64)> {
        let o = self.this_data_view(call.this, method)?;
        let idx = self.to_index(call.arg(0))?;
        Ok((o, idx))
    }

    pub fn data_view_proto_set_float32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setFloat32")?;
        let val = to_float32(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 4)?;
        let data = self.data_view_bytes(o);
        ensure_not_detached(self, &data, true)?;
        data.borrow_mut().set_float32(p.idx, val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_float64(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setFloat64")?;
        let val = call.arg(1).to_float(self)?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 8)?;
        self.data_view_bytes(o)
            .borrow_mut()
            .set_float64(p.idx, val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_int8(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setInt8")?;
        let val = crate::runtime::to_int8(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 1)?;
        self.data_view_bytes(o).borrow_mut().set_int8(p.idx, val);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_int16(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setInt16")?;
        let val = crate::runtime::to_int16(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 2)?;
        self.data_view_bytes(o)
            .borrow_mut()
            .set_int16(p.idx, val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_int32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setInt32")?;
        let val = crate::runtime::to_int32(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 4)?;
        let data = self.data_view_bytes(o);
        ensure_not_detached(self, &data, true)?;
        data.borrow_mut().set_int32(p.idx, val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_uint8(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setUint8")?;
        let val = crate::runtime::to_uint8(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 1)?;
        self.data_view_bytes(o).borrow_mut().set_uint8(p.idx, val);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_uint16(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setUint16")?;
        let val = crate::runtime::to_uint16(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 2)?;
        self.data_view_bytes(o)
            .borrow_mut()
            .set_uint16(p.idx, val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_uint32(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setUint32")?;
        let val = crate::runtime::to_uint32(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 4)?;
        let data = self.data_view_bytes(o);
        ensure_not_detached(self, &data, true)?;
        data.borrow_mut().set_uint32(p.idx, val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_big_int64(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setBigInt64")?;
        let val = to_big_int64(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 8)?;
        self.data_view_bytes(o)
            .borrow_mut()
            .set_big_int64(p.idx, &val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn data_view_proto_set_big_uint64(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let (o, idx_val) = self.data_view_set_target(&call, "DataView.prototype.setBigUint64")?;
        let val = to_big_uint64(self, call.arg(1))?;
        let p = self.data_view_get_idx_and_byte_order(o, idx_val, Some(call.arg(2)), 8)?;
        self.data_view_bytes(o)
            .borrow_mut()
            .set_big_uint64(p.idx, &val, p.bo);
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_get_buffer(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "get TypedArray.prototype.buffer")?;
        Ok(Value::Object(ta.viewed_array_buf))
    }

    pub fn typed_array_proto_get_byte_len(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "get TypedArray.prototype.byteLength")?;
        if ta.data.borrow().is_nil() {
            return Ok(Value::Int(0));
        }
        Ok(int_to_value(ta.length * ta.elem_size))
    }

    pub fn typed_array_proto_get_length(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "get TypedArray.prototype.length")?;
        if ta.data.borrow().is_nil() {
            return Ok(Value::Int(0));
        }
        Ok(int_to_value(ta.length))
    }

    pub fn typed_array_proto_get_byte_offset(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "get TypedArray.prototype.byteOffset")?;
        if ta.data.borrow().is_nil() {
            return Ok(Value::Int(0));
        }
        Ok(int_to_value(ta.offset * ta.elem_size))
    }

    pub fn typed_array_proto_copy_within(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.copyWithin")?;
        ensure_not_detached(self, &ta.data, true)?;
        let l = ta.length;
        let to = to_int_strict(rel_to_idx(call.arg(0).to_integer(self)?, l));
        let from = to_int_strict(rel_to_idx(call.arg(1).to_integer(self)?, l));
        let rel_end = match call.arg(2) {
            Value::Undefined => l,
            end => end.to_integer(self)?,
        };
        let fin = to_int_strict(rel_to_idx(rel_end, l));
        let offset = ta.offset;
        let elem_size = ta.elem_size;
        if fin > from {
            ensure_not_detached(self, &ta.data, true)?;
            let mut b = ta.data.borrow_mut();
            let data = b.bytes_mut();
            let dst = ((offset + to) * elem_size) as usize;
            let s0 = ((offset + from) * elem_size) as usize;
            let s1 = ((offset + fin) * elem_size) as usize;
            assert!(
                dst <= data.len() && s1 <= data.len(),
                "slice bounds out of range"
            );
            let n = (data.len() - dst).min(s1 - s0);
            data.copy_within(s0..s0 + n, dst);
        }
        Ok(call.this.clone())
    }

    pub fn typed_array_proto_entries(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.entries")?;
        ensure_not_detached(self, &ta.data, true)?;
        Ok(self.create_array_iterator(ta.obj, IterationKind::KeyValue))
    }

    pub fn typed_array_proto_every(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.every")?;
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..ta.length {
            let v = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Undefined
            };
            let r =
                callback_fn.call(self, this.clone(), &[v, int_to_value(k), call.this.clone()])?;
            if !r.to_boolean() {
                return Ok(Value::Bool(false));
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Bool(true))
    }

    pub fn typed_array_proto_fill(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.fill")?;
        ensure_not_detached(self, &ta.data, true)?;
        let l = ta.length;
        let mut k = to_int_strict(rel_to_idx(call.arg(1).to_integer(self)?, l));
        let rel_end = match call.arg(2) {
            Value::Undefined => l,
            end => end.to_integer(self)?,
        };
        let fin = to_int_strict(rel_to_idx(rel_end, l));
        let value = ta.kind.to_raw(self, call.arg(0))?;
        ensure_not_detached(self, &ta.data, true)?;
        while k < fin {
            ta.set_raw(ta.offset + k, value);
            k += 1;
        }
        Ok(call.this.clone())
    }

    pub fn typed_array_proto_filter(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.to_object(call.this)?;
        let Some(ta) = self.typed_array(o) else {
            return Err(self.incompatible_receiver_error("TypedArray.prototype.filter", call.this));
        };
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let es = ta.elem_size as usize;
        let mut buf: Vec<u8> = Vec::with_capacity(ta.length as usize * es);
        let mut captured = 0i64;
        let mut raw_val = vec![0u8; es];
        let mark = self.handle_scope_enter();
        for k in 0..ta.length {
            let v = if ta.is_valid_integer_index(k) {
                let v = ta.get(ta.offset + k);
                let i = ((ta.offset + k) as usize) * es;
                let b = ta.data.borrow();
                let src = &b.bytes()[i..];
                let n = src.len().min(es);
                raw_val[..n].copy_from_slice(&src[..n]);
                v
            } else {
                raw_val.iter_mut().for_each(|b| *b = 0);
                Value::Undefined
            };
            let r =
                callback_fn.call(self, this.clone(), &[v, int_to_value(k), call.this.clone()])?;
            if r.to_boolean() {
                buf.extend_from_slice(&raw_val);
                captured += 1;
            }
            self.handle_scope_leave(mark);
        }
        let c = self.species_constructor_obj(o, ta.default_ctor)?;
        let abp = self.get_array_buffer_prototype();
        let ab = self._new_array_buffer(Some(abp), None);
        *self.array_buffer_shared(ab).borrow_mut() = ByteBuf::from_vec(buf);
        let ctor = self.to_constructor(&Value::Object(ta.default_ctor))?;
        let kept = ctor.construct(self, &[Value::Object(ab)], Some(ta.default_ctor))?;
        if c == ta.default_ctor {
            return Ok(Value::Object(kept));
        }
        let ret = self.typed_array_create(c, &[int_to_value(captured)])?;
        let kept_ta = self.typed_array(kept).expect("typed array");
        for i in 0..captured {
            let v = kept_ta.get(kept_ta.offset + i);
            ret.set(self, i, &v)?;
        }
        Ok(Value::Object(ret.obj))
    }

    pub fn typed_array_proto_find(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.find")?;
        ensure_not_detached(self, &ta.data, true)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..ta.length {
            let val = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Empty
            };
            let r = predicate.call(
                self,
                this.clone(),
                &[val.clone(), int_to_value(k), call.this.clone()],
            )?;
            if r.to_boolean() {
                return Ok(val);
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_find_index(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.findIndex")?;
        ensure_not_detached(self, &ta.data, true)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..ta.length {
            let v = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Undefined
            };
            let r = predicate.call(self, this.clone(), &[v, int_to_value(k), call.this.clone()])?;
            if r.to_boolean() {
                return Ok(int_to_value(k));
            }
            self.handle_scope_leave(mark);
        }
        Ok(int_to_value(-1))
    }

    pub fn typed_array_proto_find_last(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.findLast")?;
        ensure_not_detached(self, &ta.data, true)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        let mut k = ta.length - 1;
        while k >= 0 {
            let val = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Empty
            };
            let r = predicate.call(
                self,
                this.clone(),
                &[val.clone(), int_to_value(k), call.this.clone()],
            )?;
            if r.to_boolean() {
                return Ok(val);
            }
            self.handle_scope_leave(mark);
            k -= 1;
        }
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_find_last_index(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.findLastIndex")?;
        ensure_not_detached(self, &ta.data, true)?;
        let predicate = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        let mut k = ta.length - 1;
        while k >= 0 {
            let v = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Undefined
            };
            let r = predicate.call(self, this.clone(), &[v, int_to_value(k), call.this.clone()])?;
            if r.to_boolean() {
                return Ok(int_to_value(k));
            }
            self.handle_scope_leave(mark);
            k -= 1;
        }
        Ok(int_to_value(-1))
    }

    pub fn typed_array_proto_for_each(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.forEach")?;
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..ta.length {
            let val = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Empty
            };
            callback_fn.call(
                self,
                this.clone(),
                &[val, int_to_value(k), call.this.clone()],
            )?;
            self.handle_scope_leave(mark);
        }
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_includes(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.includes")?;
        ensure_not_detached(self, &ta.data, true)?;
        let length = ta.length;
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
        let search_element = normalize_zero(call.argument(0));
        let start_idx = to_int_strict(n);
        if !ensure_not_detached(self, &ta.data, false)? {
            if search_element.is_undefined() && start_idx < ta.length {
                return Ok(Value::Bool(true));
            }
            return Ok(Value::Bool(false));
        }
        if ta.kind.type_match(&search_element) {
            let se = ta.kind.to_raw(self, &search_element)?;
            for k in start_idx..ta.length {
                if ta.get_raw(ta.offset + k) == se {
                    return Ok(Value::Bool(true));
                }
            }
        }
        Ok(Value::Bool(false))
    }

    pub fn typed_array_proto_at(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.at")?;
        ensure_not_detached(self, &ta.data, true)?;
        let mut idx = call.arg(0).to_integer(self)?;
        let length = ta.length;
        if idx < 0 {
            idx += length;
        }
        if idx >= length || idx < 0 {
            return Ok(Value::Undefined);
        }
        if ensure_not_detached(self, &ta.data, false)? {
            return Ok(ta.get(ta.offset + idx));
        }
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_index_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.indexOf")?;
        ensure_not_detached(self, &ta.data, true)?;
        let length = ta.length;
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
        if ensure_not_detached(self, &ta.data, false)? {
            let search_element = normalize_zero(call.argument(0));
            if !is_nan(&search_element) && ta.kind.type_match(&search_element) {
                let se = ta.kind.to_raw(self, &search_element)?;
                for k in to_int_strict(n)..ta.length {
                    if ta.get_raw(ta.offset + k) == se {
                        return Ok(int_to_value(k));
                    }
                }
            }
        }
        Ok(int_to_value(-1))
    }

    pub fn typed_array_proto_join(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let o = self.to_object(call.this)?;
        let Some(ta) = self.typed_array(o) else {
            return Err(Throw::Value(self.new_type_error(
                "Method TypedArray.prototype.join called on incompatible receiver",
            )));
        };
        ensure_not_detached(self, &ta.data, true)?;
        let sep = match call.arg(0) {
            Value::Undefined => ascii_string(","),
            s => s.to_string(self)?,
        };
        let l = ta.length;
        if l == 0 {
            return Ok(Value::String(STRING_EMPTY.clone()));
        }
        let mut buf = StringBuilder::new();
        if ta.is_valid_integer_index(0) {
            let element0 = ta.get(ta.offset);
            if !matches!(element0, Value::Empty | Value::Undefined | Value::Null) {
                buf.write_string(&element0.prim_to_string());
            }
        }
        for i in 1..l {
            buf.write_string(&sep);
            if ta.is_valid_integer_index(i) {
                let element = ta.get(ta.offset + i);
                if !matches!(element, Value::Empty | Value::Undefined | Value::Null) {
                    buf.write_string(&element.prim_to_string());
                }
            }
        }
        Ok(Value::String(buf.string()))
    }

    pub fn typed_array_proto_keys(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.keys")?;
        ensure_not_detached(self, &ta.data, true)?;
        Ok(self.create_array_iterator(ta.obj, IterationKind::Key))
    }

    pub fn typed_array_proto_last_index_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.lastIndexOf")?;
        ensure_not_detached(self, &ta.data, true)?;
        let length = ta.length;
        if length == 0 {
            return Ok(int_to_value(-1));
        }
        let mut from_index;
        if call.arguments.len() < 2 {
            from_index = length - 1;
        } else {
            from_index = call.arg(1).to_integer(self)?;
            if from_index >= 0 {
                from_index = from_index.min(length - 1);
            } else {
                from_index += length;
                if from_index < 0 {
                    from_index = -1;
                }
            }
        }
        if ensure_not_detached(self, &ta.data, false)? {
            let search_element = normalize_zero(call.argument(0));
            if !is_nan(&search_element) && ta.kind.type_match(&search_element) {
                let se = ta.kind.to_raw(self, &search_element)?;
                let mut k = to_int_strict(from_index);
                while k >= 0 {
                    if ta.get_raw(ta.offset + k) == se {
                        return Ok(int_to_value(k));
                    }
                    k -= 1;
                }
            }
        }
        Ok(int_to_value(-1))
    }

    pub fn typed_array_proto_map(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.map")?;
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let dst = self.typed_array_species_create(&ta, &[int_to_value(ta.length)])?;
        let mark = self.handle_scope_enter();
        for i in 0..ta.length {
            let v = if ta.is_valid_integer_index(i) {
                ta.get(ta.offset + i)
            } else {
                Value::Undefined
            };
            let r =
                callback_fn.call(self, this.clone(), &[v, int_to_value(i), call.this.clone()])?;
            dst.set(self, i, &r)?;
            self.handle_scope_leave(mark);
        }
        Ok(Value::Object(dst.obj))
    }

    pub fn typed_array_proto_reduce(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.reduce")?;
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let mut k = 0i64;
        let mut acc = None;
        if call.arguments.len() >= 2 {
            acc = Some(call.argument(1));
        } else if ta.length > 0 {
            acc = Some(ta.get(ta.offset));
            k = 1;
        }
        let Some(mut acc) = acc else {
            return Err(Throw::Value(
                self.new_type_error("Reduce of empty array with no initial value"),
            ));
        };
        let mark = self.handle_scope_enter();
        while k < ta.length {
            let v = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Undefined
            };
            acc = callback_fn.call(
                self,
                Value::Undefined,
                &[acc, v, Value::Int(k), call.this.clone()],
            )?;
            self.handle_scope_keep(mark, &acc);
            k += 1;
        }
        Ok(acc)
    }

    pub fn typed_array_proto_reduce_right(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.reduceRight")?;
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let mut k = ta.length - 1;
        let mut acc = None;
        if call.arguments.len() >= 2 {
            acc = Some(call.argument(1));
        } else if k >= 0 {
            acc = Some(ta.get(ta.offset + k));
            k -= 1;
        }
        let Some(mut acc) = acc else {
            return Err(Throw::Value(
                self.new_type_error("Reduce of empty array with no initial value"),
            ));
        };
        let mark = self.handle_scope_enter();
        while k >= 0 {
            let v = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Undefined
            };
            acc = callback_fn.call(
                self,
                Value::Undefined,
                &[acc, v, Value::Int(k), call.this.clone()],
            )?;
            self.handle_scope_keep(mark, &acc);
            k -= 1;
        }
        Ok(acc)
    }

    pub fn typed_array_proto_reverse(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.reverse")?;
        ensure_not_detached(self, &ta.data, true)?;
        let l = ta.length;
        let middle = l / 2;
        let mut lower = 0;
        while lower != middle {
            let upper = l - lower - 1;
            ta.swap(ta.offset + lower, ta.offset + upper);
            lower += 1;
        }
        Ok(call.this.clone())
    }

    pub fn typed_array_proto_set(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.set")?;
        let src_obj = call.arg(0).to_object(self)?;
        let target_offset = to_int_strict(call.arg(1).to_integer(self)?);
        if target_offset < 0 {
            return Err(self.range_error("offset should be >= 0"));
        }
        ensure_not_detached(self, &ta.data, true)?;
        let target_len = ta.length;
        if let Some(src) = self.typed_array(src_obj) {
            ensure_not_detached(self, &src.data, true)?;
            let src_len = src.length;
            let x = src_len + target_offset;
            if x < 0 || x > target_len {
                return Err(self.range_error("Source is too large"));
            }
            if src.default_ctor == ta.default_ctor {
                let dst_start = ((ta.offset + target_offset) * ta.elem_size) as usize;
                let s0 = (src.offset * src.elem_size) as usize;
                let s1 = ((src.offset + src_len) * src.elem_size) as usize;
                if Rc::ptr_eq(&ta.data, &src.data) {
                    let mut b = ta.data.borrow_mut();
                    let data = b.bytes_mut();
                    assert!(dst_start <= data.len(), "slice bounds out of range");
                    let n = (data.len() - dst_start).min(s1 - s0);
                    data.copy_within(s0..s0 + n, dst_start);
                } else {
                    let sb = src.data.borrow();
                    let sbytes = &sb.bytes()[s0..s1];
                    let mut db = ta.data.borrow_mut();
                    let dst = &mut db.bytes_mut()[dst_start..];
                    let n = dst.len().min(sbytes.len());
                    dst[..n].copy_from_slice(&sbytes[..n]);
                }
            } else {
                self.check_typed_array_mix_big_int(src.default_ctor, ta.default_ctor)?;
                let src_byte = (src.offset * src.elem_size) as usize;
                let dst_byte = ((ta.offset + target_offset) * ta.elem_size) as usize;
                let (src_data_len, dst_data_len) =
                    (src.data.borrow().len(), ta.data.borrow().len());
                assert!(src_byte < src_data_len, "index out of range");
                assert!(dst_byte < dst_data_len, "index out of range");
                let same = Rc::ptr_eq(&ta.data, &src.data);
                let (cur_src, cur_dst) = if same {
                    (src_byte as i64, dst_byte as i64)
                } else {
                    (0, 0)
                };
                let end_src = cur_src + src_len * src.elem_size;
                let dst_offset = ta.offset + target_offset;
                let src_offset = src.offset;
                let segs = if ta.elem_size == src.elem_size {
                    if !same || cur_dst <= cur_src || cur_dst >= end_src {
                        [(0, src_len, true), (0, 0, true)]
                    } else {
                        [(0, src_len, false), (0, 0, true)]
                    }
                } else {
                    let mut x = (cur_dst - cur_src) / (src.elem_size - ta.elem_size);
                    if x < 0 {
                        x = 0;
                    } else if x > src_len {
                        x = src_len;
                    }
                    if ta.elem_size < src.elem_size {
                        [(x, src_len, true), (0, x, false)]
                    } else {
                        [(0, x, true), (x, src_len, false)]
                    }
                };
                self.convert_copy(&src, &ta, src_offset, dst_offset, &segs)?;
            }
        } else {
            let target_len = ta.length;
            let lv = src_obj
                .get_str(self, &"length".into(), None)?
                .unwrap_or(Value::Empty);
            let src_len = to_int_strict(to_length(self, &lv)?);
            let x = src_len + target_offset;
            if x < 0 || x > target_len {
                return Err(self.range_error("Source is too large"));
            }
            let mark = self.handle_scope_enter();
            for i in 0..src_len {
                let val = nil_safe(src_obj.get_idx(self, i, None)?);
                let k = target_offset + i;
                if let Some(bits) = ta.kind.num_bits(&val) {
                    let mut b = ta.data.borrow_mut();
                    if !b.detached && k >= 0 && k < ta.length {
                        ta.kind
                            .set_raw(b.bytes_mut(), (ta.offset + k) as usize, bits);
                    }
                } else if ta.is_valid_integer_index(k) {
                    ta.set(self, ta.offset + k, &val)?;
                }
                self.handle_scope_leave(mark);
            }
        }
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_slice(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.slice")?;
        ensure_not_detached(self, &ta.data, true)?;
        let length = ta.length;
        let start = to_int_strict(rel_to_idx(call.arg(0).to_integer(self)?, length));
        let e = match call.arg(1) {
            Value::Undefined => length,
            end => end.to_integer(self)?,
        };
        let end = to_int_strict(rel_to_idx(e, length));
        let count = (end - start).max(0);
        let dst = self.typed_array_species_create(&ta, &[int_to_value(count)])?;
        if dst.default_ctor == ta.default_ctor {
            if count > 0 {
                ensure_not_detached(self, &ta.data, true)?;
                let offset = ta.offset;
                let elem_size = ta.elem_size;
                let s0 = ((offset + start) * elem_size) as usize;
                let s1 = ((offset + start + count) * elem_size) as usize;
                if Rc::ptr_eq(&dst.data, &ta.data) {
                    let mut b = ta.data.borrow_mut();
                    let data = b.bytes_mut();
                    let n = data.len().min(s1 - s0);
                    data.copy_within(s0..s0 + n, 0);
                } else {
                    let sb = ta.data.borrow();
                    let src = &sb.bytes()[s0..s1];
                    let mut db = dst.data.borrow_mut();
                    let d = db.bytes_mut();
                    let n = d.len().min(src.len());
                    d[..n].copy_from_slice(&src[..n]);
                }
            }
        } else {
            for i in 0..count {
                ensure_not_detached(self, &ta.data, true)?;
                let v = ta.get(ta.offset + start + i);
                dst.set(self, i, &v)?;
            }
        }
        Ok(Value::Object(dst.obj))
    }

    pub fn typed_array_proto_some(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.some")?;
        ensure_not_detached(self, &ta.data, true)?;
        let callback_fn = self.to_callable(call.arg(0))?;
        let this = call.argument(1);
        let mark = self.handle_scope_enter();
        for k in 0..ta.length {
            let v = if ta.is_valid_integer_index(k) {
                ta.get(ta.offset + k)
            } else {
                Value::Undefined
            };
            let r =
                callback_fn.call(self, this.clone(), &[v, int_to_value(k), call.this.clone()])?;
            if r.to_boolean() {
                return Ok(Value::Bool(true));
            }
            self.handle_scope_leave(mark);
        }
        Ok(Value::Bool(false))
    }

    pub fn typed_array_proto_sort(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.sort")?;
        ensure_not_detached(self, &ta.data, true)?;
        let mut compare_fn = None;
        if !call.arg(0).is_undefined() {
            compare_fn = Some(self.to_callable(call.arg(0))?);
        }
        sort_typed_array(self, ta, compare_fn)?;
        Ok(call.this.clone())
    }

    pub fn typed_array_proto_subarray(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.subarray")?;
        let l = ta.length;
        let begin_idx = rel_to_idx(call.arg(0).to_integer(self)?, l);
        let rel_end = match call.arg(1) {
            Value::Undefined => l,
            end => end.to_integer(self)?,
        };
        let end_idx = rel_to_idx(rel_end, l);
        let new_len = (end_idx - begin_idx).max(0);
        let r = self.typed_array_species_create(
            &ta,
            &[
                Value::Object(ta.viewed_array_buf),
                int_to_value((ta.offset + begin_idx) * ta.elem_size),
                int_to_value(new_len),
            ],
        )?;
        Ok(Value::Object(r.obj))
    }

    pub fn typed_array_proto_to_locale_string(
        &mut self,
        call: FunctionCall<'_>,
    ) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.toLocaleString")?;
        let length = ta.length;
        let mut buf = StringBuilder::new();
        let mark = self.handle_scope_enter();
        for i in 0..length {
            ensure_not_detached(self, &ta.data, true)?;
            if i > 0 {
                buf.write_rune(',' as i32);
            }
            let item = ta.get(ta.offset + i);
            self.write_item_locale_string(&item, &mut buf)?;
            self.handle_scope_leave(mark);
        }
        Ok(Value::String(buf.string()))
    }

    pub fn typed_array_proto_values(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_typed_array(call.this, "TypedArray.prototype.values")?;
        ensure_not_detached(self, &ta.data, true)?;
        Ok(self.create_array_iterator(ta.obj, IterationKind::Value))
    }

    pub fn typed_array_proto_to_string_tag(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        if let Value::Object(obj) = call.this
            && let Some(ta) = self.typed_array(*obj)
        {
            return Ok(nil_safe(ta.default_ctor.get_str(
                self,
                &"name".into(),
                None,
            )?));
        }
        Ok(Value::Undefined)
    }

    pub fn typed_array_proto_with(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_valid_typed_array(call.this)?;
        ensure_not_detached(self, &ta.data, true)?;
        let length = ta.length;
        let relative_index = call.arg(0).to_integer(self)?;
        let actual_index = if relative_index >= 0 {
            to_int_strict(relative_index)
        } else {
            to_int_strict(length + relative_index)
        };
        if !ta.is_valid_integer_index(actual_index) {
            return Err(self.range_error("Invalid typed array index"));
        }
        let numeric_value = if ta.kind.is_big_int() {
            Value::BigInt(to_big_int(self, call.arg(1))?)
        } else {
            call.arg(1).to_number(self)?
        };
        let a = self.typed_array_create(ta.default_ctor, &[int_to_value(length)])?;
        for k in 0..length {
            let from_value = if k == actual_index {
                numeric_value.clone()
            } else {
                ta.get(ta.offset + k)
            };
            a.set(self, ta.offset + k, &from_value)?;
        }
        Ok(Value::Object(a.obj))
    }

    pub fn typed_array_proto_to_reversed(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_valid_typed_array(call.this)?;
        ensure_not_detached(self, &ta.data, true)?;
        let length = ta.length;
        let a = self.typed_array_create(ta.default_ctor, &[int_to_value(length)])?;
        for k in 0..length {
            let from = length - k - 1;
            let from_value = ta.get(ta.offset + from);
            a.set(self, ta.offset + k, &from_value)?;
        }
        Ok(Value::Object(a.obj))
    }

    pub fn typed_array_proto_to_sorted(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let ta = self.this_valid_typed_array(call.this)?;
        ensure_not_detached(self, &ta.data, true)?;
        let mut compare_fn = None;
        let arg = call.arg(0);
        if !arg.is_undefined() {
            if let Value::Object(o) = arg {
                compare_fn = o.assert_callable(self);
            }
            if compare_fn.is_none() {
                return Err(Throw::Value(self.new_type_error(
                    "The comparison function must be either a function or undefined",
                )));
            }
        }
        let length = ta.length;
        let a = self.typed_array_create(ta.default_ctor, &[int_to_value(length)])?;
        if !Rc::ptr_eq(&a.data, &ta.data) {
            let sb = ta.data.borrow();
            let src = sb.bytes();
            let mut db = a.data.borrow_mut();
            let dst = db.bytes_mut();
            let n = dst.len().min(src.len());
            dst[..n].copy_from_slice(&src[..n]);
        }
        let obj = a.obj;
        sort_typed_array(self, a, compare_fn)?;
        Ok(Value::Object(obj))
    }

    pub fn new_typed_array(
        &mut self,
        _args: &[Value],
        _new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        Err(Throw::Value(self.new_type_error(
            "Abstract class TypedArray not directly constructable",
        )))
    }

    pub fn typed_array_from(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let mut map_fc = None;
        let this_value = call.argument(2);
        if !call.arg(1).is_undefined() {
            map_fc = Some(self.to_callable(call.arg(1))?);
        }
        let source = self.to_object(call.arg(0))?;
        let it = source.get_sym(self, SYM_ITERATOR, None)?;
        let using_iter = to_method(self, it)?;
        if let Some(using_iter) = using_iter {
            let values = self.iterable_to_list(&Value::Object(source), Some(using_iter))?;
            let ta = self.typed_array_create(c, &[int_to_value(values.len() as i64)])?;
            match map_fc {
                None => {
                    for (idx, val) in values.iter().enumerate() {
                        ta.set(self, idx as i64, val)?;
                    }
                }
                Some(map_fc) => {
                    let mark = self.handle_scope_enter();
                    for (idx, val) in values.iter().enumerate() {
                        let v = map_fc.call(
                            self,
                            this_value.clone(),
                            &[val.clone(), int_to_value(idx as i64)],
                        )?;
                        ta.put_idx(self, idx as i64, &v)?;
                        self.handle_scope_leave(mark);
                    }
                }
            }
            return Ok(Value::Object(ta.obj));
        }
        let lv = source
            .get_str(self, &"length".into(), None)?
            .unwrap_or(Value::Empty);
        let length = to_int_strict(to_length(self, &lv)?);
        let ta = self.typed_array_create(c, &[int_to_value(length)])?;
        let mark = self.handle_scope_enter();
        match map_fc {
            None => {
                for i in 0..length {
                    let v = nil_safe(source.get_idx(self, i, None)?);
                    ta.set(self, i, &v)?;
                    self.handle_scope_leave(mark);
                }
            }
            Some(map_fc) => {
                for i in 0..length {
                    let v = source.get_idx(self, i, None)?.unwrap_or(Value::Empty);
                    let r = map_fc.call(self, this_value.clone(), &[v, Value::Int(i)])?;
                    ta.set(self, i, &r)?;
                    self.handle_scope_leave(mark);
                }
            }
        }
        Ok(Value::Object(ta.obj))
    }

    pub fn typed_array_of(&mut self, call: FunctionCall<'_>) -> JsResult<Value> {
        let c = self.to_object(call.this)?;
        let ta = self.typed_array_create(c, &[int_to_value(call.arguments.len() as i64)])?;
        for (i, val) in call.arguments.iter().enumerate() {
            ta.set(self, i as i64, val)?;
        }
        Ok(Value::Object(ta.obj))
    }

    pub fn allocate_typed_array(
        &mut self,
        new_target: ObjRef,
        length: i64,
        ta_ctor: TypedArrayObjectCtor,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        let abp = self.get_array_buffer_prototype();
        let buf = self._new_array_buffer(Some(abp), None);
        let p = self.get_prototype_from_ctor(new_target, None, Some(proto))?;
        let ta = ta_ctor(self, buf, 0, length, p);
        if length > 0 {
            let es = self.typed_array(ta).expect("typed array").elem_size;
            let data = alloc_byte_slice(length * es)?;
            *self.array_buffer_shared(buf).borrow_mut() = ByteBuf::from_vec(data);
        }
        Ok(ta)
    }

    pub fn typed_array_species_create(
        &mut self,
        ta: &TypedArray,
        args: &[Value],
    ) -> JsResult<TypedArray> {
        let c = self.species_constructor_obj(ta.obj, ta.default_ctor)?;
        self.typed_array_create(c, args)
    }

    pub fn typed_array_create(&mut self, ctor: ObjRef, args: &[Value]) -> JsResult<TypedArray> {
        let c = self.to_constructor(&Value::Object(ctor))?;
        let o = c.construct(self, args, Some(ctor))?;
        if let Some(ta) = self.typed_array(o) {
            ensure_not_detached(self, &ta.data, true)?;
            if args.len() == 1
                && let Value::Int(l) = &args[0]
                && ta.length < *l
            {
                return Err(Throw::Value(self.new_type_error(
                    "Derived TypedArray constructor created an array which was too small",
                )));
            }
            return Ok(ta);
        }
        let s = Value::Object(o).fmt_string(self)?;
        Err(Throw::Value(
            self.new_type_error(&format!("Invalid TypedArray: {s}")),
        ))
    }

    pub fn typed_array_from_items(
        &mut self,
        ctor: ObjRef,
        items: ObjRef,
        map_fn: Option<Value>,
        this_value: Option<Value>,
        ta_ctor: TypedArrayObjectCtor,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        let mut map_fc = None;
        let mut this_value = this_value;
        if let Some(map_fn) = &map_fn {
            map_fc = Some(self.to_callable(map_fn)?);
            if this_value.is_none() {
                this_value = Some(Value::Undefined);
            }
        }
        let this_value = this_value.unwrap_or(Value::Empty);
        let it = items.get_sym(self, SYM_ITERATOR, None)?;
        let using_iter = to_method(self, it)?;
        if let Some(using_iter) = using_iter {
            let values = self.iterable_to_list(&Value::Object(items), Some(using_iter))?;
            let ta = self.allocate_typed_array(ctor, values.len() as i64, ta_ctor, proto)?;
            let ta = self.typed_array(ta).expect("typed array");
            match map_fc {
                None => {
                    for (idx, val) in values.iter().enumerate() {
                        ta.set(self, idx as i64, val)?;
                    }
                }
                Some(map_fc) => {
                    let mark = self.handle_scope_enter();
                    for (idx, val) in values.iter().enumerate() {
                        let v = map_fc.call(
                            self,
                            this_value.clone(),
                            &[val.clone(), int_to_value(idx as i64)],
                        )?;
                        ta.set(self, idx as i64, &v)?;
                        self.handle_scope_leave(mark);
                    }
                }
            }
            return Ok(ta.obj);
        }
        let lv = items
            .get_str(self, &"length".into(), None)?
            .unwrap_or(Value::Empty);
        let length = to_int_strict(to_length(self, &lv)?);
        let ta = self.allocate_typed_array(ctor, length, ta_ctor, proto)?;
        let ta = self.typed_array(ta).expect("typed array");
        let mark = self.handle_scope_enter();
        match map_fc {
            None => {
                for i in 0..length {
                    let v = nil_safe(items.get_idx(self, i, None)?);
                    ta.set(self, i, &v)?;
                    self.handle_scope_leave(mark);
                }
            }
            Some(map_fc) => {
                for i in 0..length {
                    let v = items.get_idx(self, i, None)?.unwrap_or(Value::Empty);
                    let r = map_fc.call(self, this_value.clone(), &[v, Value::Int(i)])?;
                    ta.set(self, i, &r)?;
                    self.handle_scope_leave(mark);
                }
            }
        }
        Ok(ta.obj)
    }

    pub fn _new_typed_array_from_array_buffer(
        &mut self,
        ab: ObjRef,
        args: &[Value],
        new_target: ObjRef,
        ta_ctor: TypedArrayObjectCtor,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        let p = self.get_prototype_from_ctor(new_target, None, Some(proto))?;
        let ta = ta_ctor(self, ab, 0, 0, p);
        let elem_size = self.typed_array(ta).expect("typed array").elem_size;
        let data = self.array_buffer_shared(ab);
        let mut byte_offset = 0i64;
        if args.len() > 1 && !args[1].is_empty() && !args[1].is_undefined() {
            byte_offset = self.to_index(&args[1])?;
            if byte_offset % elem_size != 0 {
                let name = new_target.get_str(self, &"name".into(), None)?;
                let name = go_fmt_value(self, name)?;
                return Err(self.range_error(&format!(
                    "Start offset of {name} should be a multiple of {elem_size}"
                )));
            }
        }
        let length;
        if args.len() > 2 && !args[2].is_empty() && !args[2].is_undefined() {
            length = self.to_index(&args[2])?;
            ensure_not_detached(self, &data, true)?;
            if byte_offset + length * elem_size > data.borrow().len() as i64 {
                return Err(self.range_error(&format!("Invalid typed array length: {length}")));
            }
        } else {
            ensure_not_detached(self, &data, true)?;
            let data_len = data.borrow().len() as i64;
            if data_len % elem_size != 0 {
                let name = new_target.get_str(self, &"name".into(), None)?;
                let name = go_fmt_value(self, name)?;
                return Err(self.range_error(&format!(
                    "Byte length of {name} should be a multiple of {elem_size}"
                )));
            }
            length = (data_len - byte_offset) / elem_size;
            if length < 0 {
                return Err(self.range_error(&format!(
                    "Start offset {byte_offset} is outside the bounds of the buffer"
                )));
            }
        }
        self.set_typed_array_bounds(ta, byte_offset / elem_size, length);
        Ok(ta)
    }

    fn ctor_name(&mut self, o: ObjRef) -> JsResult<String> {
        match o.get_str(self, &"name".into(), None)? {
            Some(v) if !v.is_empty() => v.go_string(self),
            _ => panic!("invalid memory address or nil pointer dereference"),
        }
    }

    pub fn check_typed_array_mix_big_int(&mut self, src: ObjRef, dst: ObjRef) -> JsResult<()> {
        let src_type = self.ctor_name(src)?;
        if src_type.starts_with("Big") && !self.ctor_name(dst)?.starts_with("Big") {
            return Err(Throw::TypeError(ERR_MIX_BIG_INT_TYPE.to_string()));
        }
        Ok(())
    }

    pub fn _new_typed_array_from_typed_array(
        &mut self,
        src: &TypedArray,
        new_target: ObjRef,
        ta_ctor: TypedArrayObjectCtor,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        let dst = self.allocate_typed_array(new_target, 0, ta_ctor, proto)?;
        let dst_ta = self.typed_array(dst).expect("typed array");
        ensure_not_detached(self, &src.data, true)?;
        let l = src.length;
        let data = alloc_byte_slice(to_int_strict(l * dst_ta.elem_size))?;
        *dst_ta.data.borrow_mut() = ByteBuf::from_vec(data);
        ensure_not_detached(self, &src.data, true)?;
        if src.default_ctor == dst_ta.default_ctor {
            {
                let sb = src.data.borrow();
                let s0 = (src.offset * src.elem_size) as usize;
                let s = &sb.bytes()[s0..];
                let mut db = dst_ta.data.borrow_mut();
                let d = db.bytes_mut();
                let n = d.len().min(s.len());
                d[..n].copy_from_slice(&s[..n]);
            }
            self.set_typed_array_bounds(dst, 0, src.length);
            return Ok(dst);
        }
        self.check_typed_array_mix_big_int(src.default_ctor, new_target)?;
        self.set_typed_array_bounds(dst, 0, l);
        let dst_ta = self.typed_array(dst).expect("typed array");
        for i in 0..l {
            let v = src.get(src.offset + i);
            dst_ta.set(self, i, &v)?;
        }
        Ok(dst)
    }

    pub fn _new_typed_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        ta_ctor: TypedArrayObjectCtor,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        let Some(new_target) = new_target else {
            return Err(Throw::Value(self.need_new("TypedArray")));
        };
        if let Some(Value::Object(obj)) = args.first() {
            let obj = *obj;
            if self.array_buffer_data(obj).is_some() {
                return self
                    ._new_typed_array_from_array_buffer(obj, args, new_target, ta_ctor, proto);
            }
            if let Some(src) = self.typed_array(obj) {
                return self._new_typed_array_from_typed_array(&src, new_target, ta_ctor, proto);
            }
            return self.typed_array_from_items(new_target, obj, None, None, ta_ctor, proto);
        }
        let mut l = 0;
        if let Some(arg0) = args.first()
            && !arg0.is_empty()
        {
            l = self.to_index(arg0)?;
        }
        self.allocate_typed_array(new_target, l, ta_ctor, proto)
    }

    pub fn new_uint8_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_uint8_array_object, proto)
    }

    pub fn new_uint8_clamped_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(
            args,
            new_target,
            Runtime::new_uint8_clamped_array_object,
            proto,
        )
    }

    pub fn new_int8_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_int8_array_object, proto)
    }

    pub fn new_uint16_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_uint16_array_object, proto)
    }

    pub fn new_int16_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_int16_array_object, proto)
    }

    pub fn new_uint32_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_uint32_array_object, proto)
    }

    pub fn new_int32_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_int32_array_object, proto)
    }

    pub fn new_float32_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_float32_array_object, proto)
    }

    pub fn new_float64_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_float64_array_object, proto)
    }

    pub fn new_big_int64_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(args, new_target, Runtime::new_big_int64_array_object, proto)
    }

    pub fn new_big_uint64_array(
        &mut self,
        args: &[Value],
        new_target: Option<ObjRef>,
        proto: ObjRef,
    ) -> JsResult<ObjRef> {
        self._new_typed_array(
            args,
            new_target,
            Runtime::new_big_uint64_array_object,
            proto,
        )
    }

    pub fn create_array_buffer_proto(&mut self, val: ObjRef) {
        let op = self.global.object_prototype;
        self.new_base_object_obj(val, op, CLASS_OBJECT);
        let getter = self.new_native_func(
            NativeFn::Fn(|r, c| r.array_buffer_proto_get_byte_length(c)),
            "get byteLength",
            0,
        );
        self.obj_mut(val).base._put(
            &"byteLength".into(),
            Value::Property(Box::new(ValueProperty {
                accessor: true,
                configurable: true,
                getter_func: Some(getter),
                ..ValueProperty::default()
            })),
        );
        let ab = self.get_array_buffer();
        val._put_prop(
            self,
            &"constructor".into(),
            Value::Object(ab),
            true,
            false,
            true,
        );
        let slice = self.new_native_func(
            NativeFn::Fn(|r, c| r.array_buffer_proto_slice(c)),
            "slice",
            2,
        );
        val._put_prop(
            self,
            &"slice".into(),
            Value::Object(slice),
            true,
            false,
            true,
        );
        val._put_sym(
            self,
            SYM_TO_STRING_TAG,
            value_prop(
                Value::String(ascii_string("ArrayBuffer")),
                false,
                false,
                true,
            ),
        );
    }

    pub fn create_array_buffer(&mut self, val: ObjRef) {
        let proto = self.get_array_buffer_prototype();
        let o = self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.builtin_new_array_buffer(args, nt),
            Some(proto),
            "ArrayBuffer",
            1,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.array_buffer_is_view(c)), "isView", 1);
        o._put_prop(self, &"isView".into(), Value::Object(f), true, false, true);
        self.put_species_return_this(o);
    }

    pub fn create_data_view(&mut self, val: ObjRef) {
        let proto = self.get_data_view_prototype();
        self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.new_data_view(args, nt),
            Some(proto),
            "DataView",
            1,
        );
    }

    pub fn create_typed_array(&mut self, val: ObjRef) {
        let proto = self.get_typed_array_prototype();
        let o = self.new_native_construct_only(
            Some(val),
            |r, args, nt| r.new_typed_array(args, nt),
            Some(proto),
            "TypedArray",
            0,
        );
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.typed_array_from(c)), "from", 1);
        o._put_prop(self, &"from".into(), Value::Object(f), true, false, true);
        let f = self.new_native_func(NativeFn::Fn(|r, c| r.typed_array_of(c)), "of", 0);
        o._put_prop(self, &"of".into(), Value::Object(f), true, false, true);
        self.put_species_return_this(o);
    }

    pub fn get_typed_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.typed_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.typed_array = Some(ret);
        self.create_typed_array(ret);
        ret
    }

    pub fn create_typed_array_ctor(
        &mut self,
        val: ObjRef,
        ctor: fn(&mut Runtime, &[Value], Option<ObjRef>, ObjRef) -> JsResult<ObjRef>,
        name: &str,
        bytes_per_element: i64,
    ) {
        let tap = self.get_typed_array_prototype();
        let p = self.new_base_object(Some(tap), CLASS_OBJECT);
        let o = self.new_native_func_and_construct(
            Some(val),
            Some(NativeFn::closure_with(
                vec![Value::Object(p)],
                move |r: &mut Runtime, d: &[Value], call: FunctionCall<'_>| {
                    let p = d[0].as_object().expect("prototype");
                    Ok(Value::Object(ctor(r, call.arguments, None, p)?))
                },
            )),
            Some(NativeCtor::closure_with(
                vec![Value::Object(val), Value::Object(p)],
                move |r: &mut Runtime, d: &[Value], args: &[Value], nt: Option<ObjRef>| {
                    let nt = nt.or(d[0].as_object());
                    let p = d[1].as_object().expect("prototype");
                    ctor(r, args, nt, p)
                },
            )),
            Some(p),
            name,
            int_to_value(3),
        );
        p._put_prop(
            self,
            &"constructor".into(),
            Value::Object(o),
            true,
            false,
            true,
        );
        let ta = self.get_typed_array();
        self.obj_mut(o).base.prototype = Some(ta);
        let bpe = int_to_value(bytes_per_element);
        o._put_prop(
            self,
            &"BYTES_PER_ELEMENT".into(),
            bpe.clone(),
            false,
            false,
            false,
        );
        p._put_prop(self, &"BYTES_PER_ELEMENT".into(), bpe, false, false, false);
    }

    pub fn get_typed_array_values(&mut self) -> ObjRef {
        if let Some(r) = self.global.typed_array_values {
            return r;
        }
        let ret = self.new_native_func(
            NativeFn::Fn(|r, c| r.typed_array_proto_values(c)),
            "values",
            0,
        );
        self.global.typed_array_values = Some(ret);
        ret
    }

    pub fn get_typed_array_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.typed_array_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.typed_array_prototype = Some(ret);
        self.new_templated_object(get_typed_array_proto_template(), Some(ret));
        ret
    }

    pub fn get_uint8_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint8_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint8_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_uint8_array(a, nt, p),
            "Uint8Array",
            1,
        );
        ret
    }

    pub fn get_uint8_clamped_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint8_clamped_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint8_clamped_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_uint8_clamped_array(a, nt, p),
            "Uint8ClampedArray",
            1,
        );
        ret
    }

    pub fn get_int8_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.int8_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.int8_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_int8_array(a, nt, p),
            "Int8Array",
            1,
        );
        ret
    }

    pub fn get_uint16_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint16_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint16_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_uint16_array(a, nt, p),
            "Uint16Array",
            2,
        );
        ret
    }

    pub fn get_int16_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.int16_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.int16_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_int16_array(a, nt, p),
            "Int16Array",
            2,
        );
        ret
    }

    pub fn get_uint32_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.uint32_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.uint32_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_uint32_array(a, nt, p),
            "Uint32Array",
            4,
        );
        ret
    }

    pub fn get_int32_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.int32_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.int32_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_int32_array(a, nt, p),
            "Int32Array",
            4,
        );
        ret
    }

    pub fn get_float32_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.float32_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.float32_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_float32_array(a, nt, p),
            "Float32Array",
            4,
        );
        ret
    }

    pub fn get_float64_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.float64_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.float64_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_float64_array(a, nt, p),
            "Float64Array",
            8,
        );
        ret
    }

    pub fn get_big_int64_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_int64_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_int64_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_big_int64_array(a, nt, p),
            "BigInt64Array",
            8,
        );
        ret
    }

    pub fn get_big_uint64_array(&mut self) -> ObjRef {
        if let Some(r) = self.global.big_uint64_array {
            return r;
        }
        let ret = self.reserve_object();
        self.global.big_uint64_array = Some(ret);
        self.create_typed_array_ctor(
            ret,
            |r, a, nt, p| r.new_big_uint64_array(a, nt, p),
            "BigUint64Array",
            8,
        );
        ret
    }

    pub fn get_data_view_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.data_view_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.data_view_prototype = Some(ret);
        self.new_templated_object(get_data_view_proto_template(), Some(ret));
        ret
    }

    pub fn get_data_view(&mut self) -> ObjRef {
        if let Some(r) = self.global.data_view {
            return r;
        }
        let ret = self.reserve_object();
        self.global.data_view = Some(ret);
        self.create_data_view(ret);
        ret
    }

    pub fn get_array_buffer_prototype(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_buffer_prototype {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_buffer_prototype = Some(ret);
        self.create_array_buffer_proto(ret);
        ret
    }

    pub fn get_array_buffer(&mut self) -> ObjRef {
        if let Some(r) = self.global.array_buffer {
            return r;
        }
        let ret = self.reserve_object();
        self.global.array_buffer = Some(ret);
        self.create_array_buffer(ret);
        ret
    }
}

fn accessor(
    r: &mut Runtime,
    f: fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value>,
    name: &str,
) -> Value {
    let g = r.new_native_func(NativeFn::Fn(f), name, 0);
    Value::Property(Box::new(ValueProperty {
        accessor: true,
        configurable: true,
        getter_func: Some(g),
        ..ValueProperty::default()
    }))
}

pub fn add_typed_arrays(t: &mut ObjectTemplate) {
    t.put_str("ArrayBuffer", |r| {
        value_prop(Value::Object(r.get_array_buffer()), true, false, true)
    });
    t.put_str("DataView", |r| {
        value_prop(Value::Object(r.get_data_view()), true, false, true)
    });
    t.put_str("Uint8Array", |r| {
        value_prop(Value::Object(r.get_uint8_array()), true, false, true)
    });
    t.put_str("Uint8ClampedArray", |r| {
        value_prop(
            Value::Object(r.get_uint8_clamped_array()),
            true,
            false,
            true,
        )
    });
    t.put_str("Int8Array", |r| {
        value_prop(Value::Object(r.get_int8_array()), true, false, true)
    });
    t.put_str("Uint16Array", |r| {
        value_prop(Value::Object(r.get_uint16_array()), true, false, true)
    });
    t.put_str("Int16Array", |r| {
        value_prop(Value::Object(r.get_int16_array()), true, false, true)
    });
    t.put_str("Uint32Array", |r| {
        value_prop(Value::Object(r.get_uint32_array()), true, false, true)
    });
    t.put_str("Int32Array", |r| {
        value_prop(Value::Object(r.get_int32_array()), true, false, true)
    });
    t.put_str("Float32Array", |r| {
        value_prop(Value::Object(r.get_float32_array()), true, false, true)
    });
    t.put_str("Float64Array", |r| {
        value_prop(Value::Object(r.get_float64_array()), true, false, true)
    });
    t.put_str("BigInt64Array", |r| {
        value_prop(Value::Object(r.get_big_int64_array()), true, false, true)
    });
    t.put_str("BigUint64Array", |r| {
        value_prop(Value::Object(r.get_big_uint64_array()), true, false, true)
    });
}

fn create_typed_array_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("buffer", |r| {
        accessor(r, |r, c| r.typed_array_proto_get_buffer(c), "get buffer")
    });
    t.put_str("byteLength", |r| {
        accessor(
            r,
            |r, c| r.typed_array_proto_get_byte_len(c),
            "get byteLength",
        )
    });
    t.put_str("byteOffset", |r| {
        accessor(
            r,
            |r, c| r.typed_array_proto_get_byte_offset(c),
            "get byteOffset",
        )
    });
    t.put_str("at", |r| {
        r.method_prop(|r, c| r.typed_array_proto_at(c), "at", 1)
    });
    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_typed_array()), true, false, true)
    });
    t.put_str("copyWithin", |r| {
        r.method_prop(|r, c| r.typed_array_proto_copy_within(c), "copyWithin", 2)
    });
    t.put_str("entries", |r| {
        r.method_prop(|r, c| r.typed_array_proto_entries(c), "entries", 0)
    });
    t.put_str("every", |r| {
        r.method_prop(|r, c| r.typed_array_proto_every(c), "every", 1)
    });
    t.put_str("fill", |r| {
        r.method_prop(|r, c| r.typed_array_proto_fill(c), "fill", 1)
    });
    t.put_str("filter", |r| {
        r.method_prop(|r, c| r.typed_array_proto_filter(c), "filter", 1)
    });
    t.put_str("find", |r| {
        r.method_prop(|r, c| r.typed_array_proto_find(c), "find", 1)
    });
    t.put_str("findIndex", |r| {
        r.method_prop(|r, c| r.typed_array_proto_find_index(c), "findIndex", 1)
    });
    t.put_str("findLast", |r| {
        r.method_prop(|r, c| r.typed_array_proto_find_last(c), "findLast", 1)
    });
    t.put_str("findLastIndex", |r| {
        r.method_prop(
            |r, c| r.typed_array_proto_find_last_index(c),
            "findLastIndex",
            1,
        )
    });
    t.put_str("forEach", |r| {
        r.method_prop(|r, c| r.typed_array_proto_for_each(c), "forEach", 1)
    });
    t.put_str("includes", |r| {
        r.method_prop(|r, c| r.typed_array_proto_includes(c), "includes", 1)
    });
    t.put_str("indexOf", |r| {
        r.method_prop(|r, c| r.typed_array_proto_index_of(c), "indexOf", 1)
    });
    t.put_str("join", |r| {
        r.method_prop(|r, c| r.typed_array_proto_join(c), "join", 1)
    });
    t.put_str("keys", |r| {
        r.method_prop(|r, c| r.typed_array_proto_keys(c), "keys", 0)
    });
    t.put_str("lastIndexOf", |r| {
        r.method_prop(
            |r, c| r.typed_array_proto_last_index_of(c),
            "lastIndexOf",
            1,
        )
    });
    t.put_str("length", |r| {
        accessor(r, |r, c| r.typed_array_proto_get_length(c), "get length")
    });
    t.put_str("map", |r| {
        r.method_prop(|r, c| r.typed_array_proto_map(c), "map", 1)
    });
    t.put_str("reduce", |r| {
        r.method_prop(|r, c| r.typed_array_proto_reduce(c), "reduce", 1)
    });
    t.put_str("reduceRight", |r| {
        r.method_prop(|r, c| r.typed_array_proto_reduce_right(c), "reduceRight", 1)
    });
    t.put_str("reverse", |r| {
        r.method_prop(|r, c| r.typed_array_proto_reverse(c), "reverse", 0)
    });
    t.put_str("set", |r| {
        r.method_prop(|r, c| r.typed_array_proto_set(c), "set", 1)
    });
    t.put_str("slice", |r| {
        r.method_prop(|r, c| r.typed_array_proto_slice(c), "slice", 2)
    });
    t.put_str("some", |r| {
        r.method_prop(|r, c| r.typed_array_proto_some(c), "some", 1)
    });
    t.put_str("sort", |r| {
        r.method_prop(|r, c| r.typed_array_proto_sort(c), "sort", 1)
    });
    t.put_str("subarray", |r| {
        r.method_prop(|r, c| r.typed_array_proto_subarray(c), "subarray", 2)
    });
    t.put_str("toLocaleString", |r| {
        r.method_prop(
            |r, c| r.typed_array_proto_to_locale_string(c),
            "toLocaleString",
            0,
        )
    });
    t.put_str("with", |r| {
        r.method_prop(|r, c| r.typed_array_proto_with(c), "with", 2)
    });
    t.put_str("toReversed", |r| {
        r.method_prop(|r, c| r.typed_array_proto_to_reversed(c), "toReversed", 0)
    });
    t.put_str("toSorted", |r| {
        r.method_prop(|r, c| r.typed_array_proto_to_sorted(c), "toSorted", 1)
    });
    t.put_str("toString", |r| {
        value_prop(Value::Object(r.get_array_to_string()), true, false, true)
    });
    t.put_str("values", |r| {
        value_prop(Value::Object(r.get_typed_array_values()), true, false, true)
    });

    t.put_sym(SYM_ITERATOR, |r| {
        value_prop(Value::Object(r.get_typed_array_values()), true, false, true)
    });
    t.put_sym(SYM_TO_STRING_TAG, |r| {
        accessor(
            r,
            |r, c| r.typed_array_proto_to_string_tag(c),
            "get [Symbol.toStringTag]",
        )
    });

    t
}

pub fn get_typed_array_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_typed_array_proto_template)
}

fn create_data_view_proto_template() -> ObjectTemplate {
    let mut t = ObjectTemplate::new();
    t.proto_factory = Some(|r| r.global.object_prototype);

    t.put_str("buffer", |r| {
        accessor(r, |r, c| r.data_view_proto_get_buffer(c), "get buffer")
    });
    t.put_str("byteLength", |r| {
        accessor(
            r,
            |r, c| r.data_view_proto_get_byte_len(c),
            "get byteLength",
        )
    });
    t.put_str("byteOffset", |r| {
        accessor(
            r,
            |r, c| r.data_view_proto_get_byte_offset(c),
            "get byteOffset",
        )
    });

    t.put_str("constructor", |r| {
        value_prop(Value::Object(r.get_data_view()), true, false, true)
    });

    t.put_str("getFloat32", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_float32(c), "getFloat32", 1)
    });
    t.put_str("getFloat64", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_float64(c), "getFloat64", 1)
    });
    t.put_str("getInt8", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_int8(c), "getInt8", 1)
    });
    t.put_str("getInt16", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_int16(c), "getInt16", 1)
    });
    t.put_str("getInt32", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_int32(c), "getInt32", 1)
    });
    t.put_str("getUint8", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_uint8(c), "getUint8", 1)
    });
    t.put_str("getUint16", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_uint16(c), "getUint16", 1)
    });
    t.put_str("getUint32", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_uint32(c), "getUint32", 1)
    });
    t.put_str("getBigInt64", |r| {
        r.method_prop(|r, c| r.data_view_proto_get_big_int64(c), "getBigInt64", 1)
    });
    t.put_str("getBigUint64", |r| {
        r.method_prop(
            |r, c| r.data_view_proto_get_big_uint64(c),
            "getBigUint64",
            1,
        )
    });
    t.put_str("setFloat32", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_float32(c), "setFloat32", 2)
    });
    t.put_str("setFloat64", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_float64(c), "setFloat64", 2)
    });
    t.put_str("setInt8", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_int8(c), "setInt8", 2)
    });
    t.put_str("setInt16", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_int16(c), "setInt16", 2)
    });
    t.put_str("setInt32", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_int32(c), "setInt32", 2)
    });
    t.put_str("setUint8", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_uint8(c), "setUint8", 2)
    });
    t.put_str("setUint16", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_uint16(c), "setUint16", 2)
    });
    t.put_str("setUint32", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_uint32(c), "setUint32", 2)
    });
    t.put_str("setBigInt64", |r| {
        r.method_prop(|r, c| r.data_view_proto_set_big_int64(c), "setBigInt64", 2)
    });
    t.put_str("setBigUint64", |r| {
        r.method_prop(
            |r, c| r.data_view_proto_set_big_uint64(c),
            "setBigUint64",
            2,
        )
    });

    t.put_sym(SYM_TO_STRING_TAG, |_| {
        value_prop(Value::String(ascii_string("DataView")), false, false, true)
    });

    t
}

pub fn get_data_view_proto_template() -> &'static ObjectTemplate {
    static T: OnceLock<ObjectTemplate> = OnceLock::new();
    T.get_or_init(create_data_view_proto_template)
}
