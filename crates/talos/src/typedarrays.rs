use crate::builtin_bigint::to_big_int;
use crate::deps::big;
use crate::heap::{ObjRef, Tracer};
use crate::host::{DocValue, PodElem, PodStore, SliceElem, TypedSlice};
use crate::object::{
    BASE_CLASS, BaseObject, CLASS_OBJECT, EnumerableFlag, Flag, IterNextFunc, ObjClass, ObjData,
    ObjExt, PropIter, PropIterItem, PropertyDescriptor, UStr, idx_string,
};
use crate::runtime::{
    JsResult, Runtime, Throw, str_to_int_num, to_float32, to_int_clamp, to_int8, to_int16,
    to_int32, to_uint8, to_uint8_clamp, to_uint16, to_uint32,
};
use crate::string::ascii_string;
use crate::value::{
    Value, ValueProperty, float_to_value, go_float_to_int, int_to_value, new_big_int,
};
use std::any::Any;
use std::cell::RefCell;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ByteOrder {
    BigEndian,
    LittleEndian,
}

pub const BIG_ENDIAN: ByteOrder = ByteOrder::BigEndian;
pub const LITTLE_ENDIAN: ByteOrder = ByteOrder::LittleEndian;

pub const NATIVE_ENDIAN: ByteOrder = if cfg!(target_endian = "little") {
    ByteOrder::LittleEndian
} else {
    ByteOrder::BigEndian
};

pub const GO_NAN_BITS: u64 = 0x7FF8_0000_0000_0001;

pub fn float64_bits(f: f64) -> u64 {
    if f.is_nan() {
        return GO_NAN_BITS;
    }
    f.to_bits()
}

#[derive(Debug, Default)]
pub struct ByteBuf {
    pub data: Vec<u8>,
    pub nil: bool,
    pub detached: bool,
}

pub type SharedBytes = Rc<RefCell<ByteBuf>>;

impl ByteBuf {
    pub fn new_nil() -> ByteBuf {
        ByteBuf {
            data: Vec::new(),
            nil: true,
            detached: false,
        }
    }

    pub fn from_vec(data: Vec<u8>) -> ByteBuf {
        ByteBuf {
            data,
            nil: false,
            detached: false,
        }
    }

    pub fn shared(self) -> SharedBytes {
        Rc::new(RefCell::new(self))
    }

    pub fn len(&self) -> usize {
        if self.detached {
            return 0;
        }
        self.data.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn is_nil(&self) -> bool {
        self.nil || self.detached
    }

    pub fn bytes(&self) -> &[u8] {
        if self.detached {
            return &[];
        }
        &self.data
    }

    pub fn bytes_mut(&mut self) -> &mut [u8] {
        if self.detached {
            return &mut [];
        }
        &mut self.data
    }
}

fn rd<const N: usize>(b: &[u8], off: usize) -> [u8; N] {
    let mut a = [0u8; N];
    a.copy_from_slice(&b[off..off + N]);
    a
}

#[derive(Clone, Copy, Debug)]
pub enum Num {
    I(i64),
    F(f64),
}

impl Num {
    fn float(f: f64) -> Num {
        if f.is_nan() {
            return Num::F(f64::NAN);
        }
        Num::F(f)
    }
}

fn clamp_float(num: f64) -> u8 {
    if num.is_nan() || num < 0.0 {
        return 0;
    }
    if num > 255.0 {
        return 255;
    }
    let f = num.floor();
    let f1 = f + 0.5;
    if f1 < num {
        return (f + 1.0) as u8;
    }
    if f1 > num {
        return f as u8;
    }
    let r = f as u8;
    if r & 1 != 0 { r + 1 } else { r }
}

fn swap_n<const N: usize>(b: &mut [u8], oi: usize, oj: usize) {
    let x = rd::<N>(b, oi);
    let y = rd::<N>(b, oj);
    b[oi..oi + N].copy_from_slice(&y);
    b[oj..oj + N].copy_from_slice(&x);
}

fn wr(b: &mut [u8], off: usize, src: &[u8]) {
    if let Some(dst) = b.get_mut(off..off + src.len()) {
        dst.copy_from_slice(src);
    }
}

pub struct ArrayBufferObject {
    pub data: SharedBytes,
}

impl ObjData for ArrayBufferObject {
    fn trace(&self, _t: &mut Tracer) {}

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

pub struct DataViewObject {
    pub viewed_array_buf: ObjRef,
    pub data: SharedBytes,
    pub byte_len: i64,
    pub byte_offset: i64,
}

impl ObjData for DataViewObject {
    fn trace(&self, t: &mut Tracer) {
        t.obj(self.viewed_array_buf);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypedKind {
    Uint8,
    Uint8Clamped,
    Int8,
    Uint16,
    Int16,
    Uint32,
    Int32,
    Float32,
    Float64,
    BigInt64,
    BigUint64,
}

pub fn to_big_int64(rt: &mut Runtime, v: &Value) -> JsResult<big::Int> {
    let n = to_big_int(rt, v)?;
    let two_to_64 = big::Int::new_int(1).lsh(64);
    let two_to_63 = big::Int::new_int(1).lsh(63);
    let int64bit = n.modulo(&two_to_64);
    if int64bit.cmp(&two_to_63) >= 0 {
        return Ok(int64bit.sub(&two_to_64));
    }
    Ok(int64bit)
}

pub fn to_big_uint64(rt: &mut Runtime, v: &Value) -> JsResult<big::Int> {
    let n = to_big_int(rt, v)?;
    Ok(n.modulo(&big::Int::new_int(1).lsh(64)))
}

pub fn typed_float_less(x: f64, y: f64) -> bool {
    let x_nan = x.is_nan();
    let y_nan = y.is_nan();
    if y_nan {
        return !x_nan;
    } else if x_nan {
        return false;
    }
    if x == 0.0 && y == 0.0 {
        return x.is_sign_negative();
    }
    x < y
}

impl TypedKind {
    pub fn elem_size(self) -> usize {
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped | TypedKind::Int8 => 1,
            TypedKind::Uint16 | TypedKind::Int16 => 2,
            TypedKind::Uint32 | TypedKind::Int32 | TypedKind::Float32 => 4,
            TypedKind::Float64 | TypedKind::BigInt64 | TypedKind::BigUint64 => 8,
        }
    }

    pub fn is_big_int(self) -> bool {
        matches!(self, TypedKind::BigInt64 | TypedKind::BigUint64)
    }

    pub fn to_raw(self, rt: &mut Runtime, v: &Value) -> JsResult<u64> {
        Ok(match self {
            TypedKind::Uint8 => u64::from(to_uint8(rt, v)?),
            TypedKind::Uint8Clamped => u64::from(to_uint8_clamp(rt, v)?),
            TypedKind::Int8 => to_int8(rt, v)? as u64,
            TypedKind::Uint16 => u64::from(to_uint16(rt, v)?),
            TypedKind::Int16 => to_int16(rt, v)? as u64,
            TypedKind::Uint32 => u64::from(to_uint32(rt, v)?),
            TypedKind::Int32 => to_int32(rt, v)? as u64,
            TypedKind::Float32 => u64::from(to_float32(rt, v)?.to_bits()),
            TypedKind::Float64 => float64_bits(v.to_float(rt)?),
            TypedKind::BigInt64 => to_big_int64(rt, v)?.uint64(),
            TypedKind::BigUint64 => to_big_uint64(rt, v)?.uint64(),
        })
    }

    pub fn get(self, b: &[u8], idx: usize) -> Value {
        let off = idx * self.elem_size();
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped => int_to_value(i64::from(b[off])),
            TypedKind::Int8 => int_to_value(i64::from(b[off] as i8)),
            TypedKind::Uint16 => int_to_value(i64::from(u16::from_ne_bytes(rd(b, off)))),
            TypedKind::Int16 => int_to_value(i64::from(i16::from_ne_bytes(rd(b, off)))),
            TypedKind::Uint32 => int_to_value(i64::from(u32::from_ne_bytes(rd(b, off)))),
            TypedKind::Int32 => int_to_value(i64::from(i32::from_ne_bytes(rd(b, off)))),
            TypedKind::Float32 => float_to_value(f64::from(f32::from_ne_bytes(rd(b, off)))),
            TypedKind::Float64 => float_to_value(f64::from_ne_bytes(rd(b, off))),
            TypedKind::BigInt64 => new_big_int(big::Int::new_int(i64::from_ne_bytes(rd(b, off)))),
            TypedKind::BigUint64 => new_big_int(big::Int::from_u64(u64::from_ne_bytes(rd(b, off)))),
        }
    }

    pub fn get_num(self, b: &[u8], idx: usize) -> Num {
        let off = idx * self.elem_size();
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped => Num::I(i64::from(b[off])),
            TypedKind::Int8 => Num::I(i64::from(b[off] as i8)),
            TypedKind::Uint16 => Num::I(i64::from(u16::from_ne_bytes(rd(b, off)))),
            TypedKind::Int16 => Num::I(i64::from(i16::from_ne_bytes(rd(b, off)))),
            TypedKind::Uint32 => Num::I(i64::from(u32::from_ne_bytes(rd(b, off)))),
            TypedKind::Int32 => Num::I(i64::from(i32::from_ne_bytes(rd(b, off)))),
            TypedKind::Float32 => Num::float(f64::from(f32::from_ne_bytes(rd(b, off)))),
            TypedKind::Float64 => Num::float(f64::from_ne_bytes(rd(b, off))),
            TypedKind::BigInt64 => Num::I(i64::from_ne_bytes(rd(b, off))),
            TypedKind::BigUint64 => Num::I(u64::from_ne_bytes(rd(b, off)) as i64),
        }
    }

    pub fn num_to_bits(self, n: Num) -> u64 {
        let (int, f) = match n {
            Num::I(i) => (i, i as f64),
            Num::F(f) if f.is_nan() || f.is_infinite() => (0, f),
            Num::F(f) => (go_float_to_int(f), f),
        };
        match self {
            TypedKind::Uint8 | TypedKind::Int8 => u64::from(int as u8),
            TypedKind::Uint16 | TypedKind::Int16 => u64::from(int as u16),
            TypedKind::Uint32 | TypedKind::Int32 => u64::from(int as u32),
            TypedKind::Uint8Clamped => u64::from(match n {
                Num::I(i) => i.clamp(0, 255) as u8,
                Num::F(_) => clamp_float(f),
            }),
            TypedKind::Float32 => u64::from((f as f32).to_bits()),
            TypedKind::Float64 => float64_bits(f),
            TypedKind::BigInt64 | TypedKind::BigUint64 => int as u64,
        }
    }

    pub fn num_bits(self, v: &Value) -> Option<u64> {
        let (int, f) = match v {
            Value::Int(i) => (*i, *i as f64),
            Value::Float(f) if f.is_nan() || f.is_infinite() => (0, *f),
            Value::Float(f) => (go_float_to_int(*f), *f),
            _ => return None,
        };
        Some(match self {
            TypedKind::Uint8 | TypedKind::Int8 => u64::from(int as u8),
            TypedKind::Uint16 | TypedKind::Int16 => u64::from(int as u16),
            TypedKind::Uint32 | TypedKind::Int32 => u64::from(int as u32),
            TypedKind::Uint8Clamped => u64::from(match v {
                Value::Int(i) => (*i).clamp(0, 255) as u8,
                _ => clamp_float(f),
            }),
            TypedKind::Float32 => u64::from((f as f32).to_bits()),
            TypedKind::Float64 => float64_bits(f),
            TypedKind::BigInt64 | TypedKind::BigUint64 => return None,
        })
    }

    pub fn set_bits(self, rt: &mut Runtime, v: &Value) -> JsResult<u64> {
        if let Some(bits) = self.num_bits(v) {
            return Ok(bits);
        }
        Ok(match self {
            TypedKind::Uint8 => u64::from(to_uint8(rt, v)?),
            TypedKind::Uint8Clamped => u64::from(to_uint8_clamp(rt, v)?),
            TypedKind::Int8 => u64::from(to_int8(rt, v)? as u8),
            TypedKind::Uint16 => u64::from(to_uint16(rt, v)?),
            TypedKind::Int16 => u64::from(to_int16(rt, v)? as u16),
            TypedKind::Uint32 => u64::from(to_uint32(rt, v)?),
            TypedKind::Int32 => u64::from(to_int32(rt, v)? as u32),
            TypedKind::Float32 => u64::from(to_float32(rt, v)?.to_bits()),
            TypedKind::Float64 => float64_bits(v.to_float(rt)?),
            TypedKind::BigInt64 => to_big_int64(rt, v)?.int64() as u64,
            TypedKind::BigUint64 => to_big_uint64(rt, v)?.uint64(),
        })
    }

    pub fn set(self, rt: &mut Runtime, data: &SharedBytes, idx: usize, v: &Value) -> JsResult<()> {
        let bits = self.set_bits(rt, v)?;
        self.set_raw(data.borrow_mut().bytes_mut(), idx, bits);
        Ok(())
    }

    pub fn get_raw(self, b: &[u8], idx: usize) -> u64 {
        let off = idx * self.elem_size();
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped => u64::from(b[off]),
            TypedKind::Int8 => b[off] as i8 as u64,
            TypedKind::Uint16 => u64::from(u16::from_ne_bytes(rd(b, off))),
            TypedKind::Int16 => i16::from_ne_bytes(rd(b, off)) as u64,
            TypedKind::Uint32 | TypedKind::Float32 => u64::from(u32::from_ne_bytes(rd(b, off))),
            TypedKind::Int32 => i32::from_ne_bytes(rd(b, off)) as u64,
            TypedKind::Float64 | TypedKind::BigInt64 | TypedKind::BigUint64 => {
                u64::from_ne_bytes(rd(b, off))
            }
        }
    }

    pub fn set_raw(self, b: &mut [u8], idx: usize, raw: u64) {
        let off = idx * self.elem_size();
        match self.elem_size() {
            1 => wr(b, off, &[raw as u8]),
            2 => wr(b, off, &(raw as u16).to_ne_bytes()),
            4 => wr(b, off, &(raw as u32).to_ne_bytes()),
            _ => wr(b, off, &raw.to_ne_bytes()),
        }
    }

    pub fn less(self, b: &[u8], i: usize, j: usize) -> bool {
        let (oi, oj) = (i * self.elem_size(), j * self.elem_size());
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped => b[oi] < b[oj],
            TypedKind::Int8 => (b[oi] as i8) < (b[oj] as i8),
            TypedKind::Uint16 => u16::from_ne_bytes(rd(b, oi)) < u16::from_ne_bytes(rd(b, oj)),
            TypedKind::Int16 => i16::from_ne_bytes(rd(b, oi)) < i16::from_ne_bytes(rd(b, oj)),
            TypedKind::Uint32 => u32::from_ne_bytes(rd(b, oi)) < u32::from_ne_bytes(rd(b, oj)),
            TypedKind::Int32 => i32::from_ne_bytes(rd(b, oi)) < i32::from_ne_bytes(rd(b, oj)),
            TypedKind::Float32 => typed_float_less(
                f64::from(f32::from_ne_bytes(rd(b, oi))),
                f64::from(f32::from_ne_bytes(rd(b, oj))),
            ),
            TypedKind::Float64 => {
                typed_float_less(f64::from_ne_bytes(rd(b, oi)), f64::from_ne_bytes(rd(b, oj)))
            }
            TypedKind::BigInt64 => i64::from_ne_bytes(rd(b, oi)) < i64::from_ne_bytes(rd(b, oj)),
            TypedKind::BigUint64 => u64::from_ne_bytes(rd(b, oi)) < u64::from_ne_bytes(rd(b, oj)),
        }
    }

    pub fn swap(self, b: &mut [u8], i: usize, j: usize) {
        let n = self.elem_size();
        let (oi, oj) = (i * n, j * n);
        match n {
            1 => b.swap(oi, oj),
            2 => swap_n::<2>(b, oi, oj),
            4 => swap_n::<4>(b, oi, oj),
            _ => swap_n::<8>(b, oi, oj),
        }
    }

    pub fn type_match(self, v: &Value) -> bool {
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped => {
                matches!(v, Value::Int(i) if (0..=255).contains(i))
            }
            TypedKind::Int8 => {
                matches!(v, Value::Int(i) if (i64::from(i8::MIN)..=i64::from(i8::MAX)).contains(i))
            }
            TypedKind::Uint16 => {
                matches!(v, Value::Int(i) if (0..=i64::from(u16::MAX)).contains(i))
            }
            TypedKind::Int16 => {
                matches!(v, Value::Int(i) if (i64::from(i16::MIN)..=i64::from(i16::MAX)).contains(i))
            }
            TypedKind::Uint32 => {
                matches!(v, Value::Int(i) if (0..=i64::from(u32::MAX)).contains(i))
            }
            TypedKind::Int32 => {
                matches!(v, Value::Int(i) if (i64::from(i32::MIN)..=i64::from(i32::MAX)).contains(i))
            }
            TypedKind::Float32 | TypedKind::Float64 => {
                matches!(v, Value::Int(_) | Value::Float(_))
            }
            TypedKind::BigInt64 | TypedKind::BigUint64 => matches!(v, Value::BigInt(_)),
        }
    }

    pub fn export(self, data: &SharedBytes, offset: usize, length: usize) -> DocValue {
        let byte_off = offset * self.elem_size();
        let nil = data.borrow().is_nil();
        match self {
            TypedKind::Uint8 | TypedKind::Uint8Clamped => {
                DocValue::Bytes(export_view::<u8>(data, byte_off, length, nil))
            }
            TypedKind::Int8 => DocValue::Int8s(export_view::<i8>(data, byte_off, length, nil)),
            TypedKind::Uint16 => DocValue::Uint16s(export_view::<u16>(data, byte_off, length, nil)),
            TypedKind::Int16 => DocValue::Int16s(export_view::<i16>(data, byte_off, length, nil)),
            TypedKind::Uint32 => DocValue::Uint32s(export_view::<u32>(data, byte_off, length, nil)),
            TypedKind::Int32 => DocValue::Int32s(export_view::<i32>(data, byte_off, length, nil)),
            TypedKind::Float32 => {
                DocValue::Float32s(export_view::<f32>(data, byte_off, length, nil))
            }
            TypedKind::Float64 => {
                DocValue::Float64s(export_view::<f64>(data, byte_off, length, nil))
            }
            TypedKind::BigInt64 => {
                DocValue::Int64s(export_view::<i64>(data, byte_off, length, nil))
            }
            TypedKind::BigUint64 => {
                DocValue::Uint64s(export_view::<u64>(data, byte_off, length, nil))
            }
        }
    }
}

fn export_view<E: PodElem + SliceElem<Store = PodStore<E>>>(
    data: &SharedBytes,
    byte_off: usize,
    length: usize,
    nil: bool,
) -> TypedSlice<E> {
    TypedSlice {
        backing: PodStore::view(data.clone(), byte_off, length, nil),
        len: length,
    }
}

pub struct TypedArrayObject {
    pub viewed_array_buf: ObjRef,
    pub data: SharedBytes,
    pub default_ctor: ObjRef,
    pub length: i64,
    pub offset: i64,
    pub elem_size: i64,
    pub kind: TypedKind,
}

impl ObjData for TypedArrayObject {
    fn trace(&self, t: &mut Tracer) {
        t.obj(self.viewed_array_buf);
        t.obj(self.default_ctor);
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

#[derive(Clone)]
pub struct TypedArray {
    pub obj: ObjRef,
    pub viewed_array_buf: ObjRef,
    pub data: SharedBytes,
    pub default_ctor: ObjRef,
    pub length: i64,
    pub offset: i64,
    pub elem_size: i64,
    pub kind: TypedKind,
}

impl TypedArray {
    pub fn is_valid_integer_index(&self, idx: i64) -> bool {
        !self.data.borrow().detached && idx >= 0 && idx < self.length
    }

    pub fn get(&self, idx: i64) -> Value {
        self.kind.get(self.data.borrow().bytes(), idx as usize)
    }

    pub fn set(&self, rt: &mut Runtime, idx: i64, v: &Value) -> JsResult<()> {
        self.kind.set(rt, &self.data, idx as usize, v)
    }

    pub fn get_raw(&self, idx: i64) -> u64 {
        self.kind.get_raw(self.data.borrow().bytes(), idx as usize)
    }

    pub fn set_raw(&self, idx: i64, raw: u64) {
        self.kind
            .set_raw(self.data.borrow_mut().bytes_mut(), idx as usize, raw);
    }

    pub fn swap(&self, i: i64, j: i64) {
        self.kind
            .swap(self.data.borrow_mut().bytes_mut(), i as usize, j as usize);
    }

    pub fn less(&self, i: i64, j: i64) -> bool {
        self.kind
            .less(self.data.borrow().bytes(), i as usize, j as usize)
    }

    pub fn put_idx(&self, rt: &mut Runtime, idx: i64, v: &Value) -> JsResult<()> {
        TypedArrayObject::_put_idx(rt, self.obj, idx, v)
    }
}

pub fn ensure_not_detached(rt: &mut Runtime, data: &SharedBytes, throw: bool) -> JsResult<bool> {
    if data.borrow().detached {
        rt.type_error_result(throw, "ArrayBuffer is detached")?;
        return Ok(false);
    }
    Ok(true)
}

fn canonical_index(name: &UStr) -> (i64, bool) {
    match name.ascii_bytes().and_then(|b| b.first()) {
        Some(c) if c.is_ascii_digit() || matches!(c, b'-' | b'I' | b'N') => str_to_int_num(name),
        _ => (-1, false),
    }
}

fn idx_prop(v: Value) -> Value {
    Value::Property(Box::new(ValueProperty {
        value: v,
        writable: true,
        enumerable: true,
        configurable: true,
        accessor: false,
        getter_func: None,
        setter_func: None,
    }))
}

pub static TYPED_ARRAY_CLASS: ObjClass = ObjClass {
    get_own_prop_str: TypedArrayObject::get_own_prop_str,
    get_own_prop_idx: TypedArrayObject::get_own_prop_idx,
    get_str: TypedArrayObject::get_str,
    get_idx: TypedArrayObject::get_idx,
    set_own_str: TypedArrayObject::set_own_str,
    set_own_idx: TypedArrayObject::set_own_idx,
    set_foreign_str: TypedArrayObject::set_foreign_str,
    set_foreign_idx: TypedArrayObject::set_foreign_idx,
    has_own_property_str: TypedArrayObject::has_own_property_str,
    has_own_property_idx: TypedArrayObject::has_own_property_idx,
    has_property_str: TypedArrayObject::has_property_str,
    has_property_idx: TypedArrayObject::has_property_idx,
    define_own_property_str: TypedArrayObject::define_own_property_str,
    define_own_property_idx: TypedArrayObject::define_own_property_idx,
    delete_str: TypedArrayObject::delete_str,
    delete_idx: TypedArrayObject::delete_idx,
    string_keys: TypedArrayObject::string_keys,
    iterate_string_keys: TypedArrayObject::iterate_string_keys,
    ..BASE_CLASS
};

pub static ARRAY_BUFFER_CLASS: ObjClass = BASE_CLASS;

pub static DATA_VIEW_CLASS: ObjClass = BASE_CLASS;

impl TypedArrayObject {
    fn data(rt: &Runtime, o: ObjRef) -> &TypedArrayObject {
        match rt.ext_dyn::<TypedArrayObject>(o) {
            Some(t) => t,
            None => unreachable!("not a typed array"),
        }
    }

    pub fn _get_idx(rt: &Runtime, o: ObjRef, idx: i64) -> Option<Value> {
        let a = TypedArrayObject::data(rt, o);
        if 0 <= idx && idx < a.length {
            let b = a.data.borrow();
            if b.detached {
                return None;
            }
            return Some(a.kind.get(b.bytes(), (idx + a.offset) as usize));
        }
        None
    }

    fn is_valid_integer_index(rt: &Runtime, o: ObjRef, idx: i64) -> bool {
        let a = TypedArrayObject::data(rt, o);
        !a.data.borrow().detached && idx >= 0 && idx < a.length
    }

    pub fn _put_idx(rt: &mut Runtime, o: ObjRef, idx: i64, v: &Value) -> JsResult<()> {
        let a = TypedArrayObject::data(rt, o);
        let kind = a.kind;
        if let Some(bits) = kind.num_bits(v) {
            let mut b = a.data.borrow_mut();
            if !b.detached && 0 <= idx && idx < a.length {
                kind.set_raw(b.bytes_mut(), (idx + a.offset) as usize, bits);
            }
            return Ok(());
        }
        let bits = if kind.is_big_int() {
            if v.is_empty() {
                return Err(Throw::TypeError(
                    "Cannot convert %!!(MISSING)s(<nil>) to a BigInt".to_string(),
                ));
            }
            let n = Value::BigInt(to_big_int(rt, v)?);
            kind.set_bits(rt, &n)?
        } else {
            if v.is_empty() {
                panic!("invalid memory address or nil pointer dereference");
            }
            match v {
                Value::Int(_) | Value::Float(_) => kind.set_bits(rt, v)?,
                _ => {
                    let n = v.to_number(rt)?;
                    kind.set_bits(rt, &n)?
                }
            }
        };
        let a = TypedArrayObject::data(rt, o);
        let mut b = a.data.borrow_mut();
        if !b.detached && 0 <= idx && idx < a.length {
            kind.set_raw(b.bytes_mut(), (idx + a.offset) as usize, bits);
        }
        Ok(())
    }

    fn _has_idx(rt: &Runtime, o: ObjRef, idx: i64) -> bool {
        TypedArrayObject::is_valid_integer_index(rt, o, idx)
    }

    pub fn get_own_prop_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<Option<Value>> {
        let (idx, ok) = canonical_index(name);
        if ok {
            return Ok(TypedArrayObject::_get_idx(rt, o, idx).map(idx_prop));
        }
        if idx == 0 {
            return Ok(None);
        }
        BaseObject::get_own_prop_str(rt, o, name)
    }

    pub fn get_own_prop_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<Option<Value>> {
        Ok(TypedArrayObject::_get_idx(rt, o, to_int_clamp(idx)).map(idx_prop))
    }

    pub fn get_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        let (idx, ok) = canonical_index(name);
        if ok {
            return Ok(TypedArrayObject::_get_idx(rt, o, idx));
        }
        if idx == 0 {
            return Ok(None);
        }
        BaseObject::get_str(rt, o, name, receiver)
    }

    pub fn get_idx(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        _receiver: Option<&Value>,
    ) -> JsResult<Option<Value>> {
        Ok(TypedArrayObject::_get_idx(rt, o, to_int_clamp(idx)))
    }

    pub fn set_own_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        throw: bool,
    ) -> JsResult<bool> {
        let (idx, ok) = canonical_index(p);
        if ok {
            TypedArrayObject::_put_idx(rt, o, idx, &v)?;
            return Ok(true);
        }
        if idx == 0 {
            rt.vm_to_numeric(&v)?;
            return Ok(true);
        }
        BaseObject::set_own_str(rt, o, p, v, throw)
    }

    pub fn set_own_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        _throw: bool,
    ) -> JsResult<bool> {
        TypedArrayObject::_put_idx(rt, o, to_int_clamp(p), &v)?;
        Ok(true)
    }

    pub fn set_foreign_str(
        rt: &mut Runtime,
        o: ObjRef,
        p: &UStr,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let prop = TypedArrayObject::get_own_prop_str(rt, o, p)?;
        BaseObject::_set_foreign_str(rt, o, p, prop.as_ref(), v, receiver, throw)
    }

    pub fn set_foreign_idx(
        rt: &mut Runtime,
        o: ObjRef,
        p: i64,
        v: Value,
        receiver: &Value,
        throw: bool,
    ) -> JsResult<(bool, bool)> {
        let present = TypedArrayObject::has_own_property_idx(rt, o, p)?;
        let prop = present.then_some(Value::Bool(true));
        BaseObject::_set_foreign_idx(rt, o, p, prop.as_ref(), v, receiver, throw)
    }

    pub fn has_own_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let (idx, ok) = canonical_index(name);
        if ok {
            return Ok(TypedArrayObject::_has_idx(rt, o, idx));
        }
        if idx == 0 {
            return Ok(false);
        }
        BaseObject::has_own_property_str(rt, o, name)
    }

    pub fn has_own_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        Ok(TypedArrayObject::_has_idx(rt, o, to_int_clamp(idx)))
    }

    pub fn has_property_str(rt: &mut Runtime, o: ObjRef, name: &UStr) -> JsResult<bool> {
        let (idx, ok) = canonical_index(name);
        if ok {
            return Ok(TypedArrayObject::_has_idx(rt, o, idx));
        }
        if idx == 0 {
            return Ok(false);
        }
        BaseObject::has_property_str(rt, o, name)
    }

    pub fn has_property_idx(rt: &mut Runtime, o: ObjRef, idx: i64) -> JsResult<bool> {
        TypedArrayObject::has_own_property_idx(rt, o, idx)
    }

    fn _define_idx_property(
        rt: &mut Runtime,
        o: ObjRef,
        idx: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        if desc.configurable == Flag::False
            || desc.enumerable == Flag::False
            || desc.is_accessor()
            || desc.writable == Flag::False
        {
            rt.type_error_result(throw, &format!("Cannot redefine property: {idx}"))?;
            return Ok(false);
        }
        let existing = TypedArrayObject::get_own_prop_idx(rt, o, idx)?;
        let name = idx_string(idx);
        let ok = BaseObject::_define_own_property(rt, o, &name, existing.as_ref(), &desc, throw)?
            .is_some();
        if ok {
            if !TypedArrayObject::is_valid_integer_index(rt, o, idx) {
                rt.type_error_result(throw, "Invalid typed array index")?;
                return Ok(false);
            }
            let v = desc.value.unwrap_or(Value::Empty);
            TypedArrayObject::_put_idx(rt, o, idx, &v)?;
            return Ok(true);
        }
        Ok(ok)
    }

    pub fn define_own_property_str(
        rt: &mut Runtime,
        o: ObjRef,
        name: &UStr,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        let (idx, ok) = canonical_index(name);
        if ok {
            return TypedArrayObject::_define_idx_property(rt, o, idx, desc, throw);
        }
        if idx == 0 {
            let data = TypedArrayObject::data(rt, o).data.clone();
            ensure_not_detached(rt, &data, throw)?;
            rt.type_error_result(throw, "Invalid typed array index")?;
            return Ok(false);
        }
        BaseObject::define_own_property_str(rt, o, name, desc, throw)
    }

    pub fn define_own_property_idx(
        rt: &mut Runtime,
        o: ObjRef,
        name: i64,
        desc: PropertyDescriptor,
        throw: bool,
    ) -> JsResult<bool> {
        TypedArrayObject::_define_idx_property(rt, o, to_int_clamp(name), desc, throw)
    }

    pub fn delete_str(rt: &mut Runtime, o: ObjRef, name: &UStr, throw: bool) -> JsResult<bool> {
        let (idx, ok) = canonical_index(name);
        if ok {
            if TypedArrayObject::is_valid_integer_index(rt, o, idx) {
                let s = Value::Object(o).go_string(rt)?;
                rt.type_error_result(throw, &format!("Cannot delete property '{idx}' of {s}"))?;
                return Ok(false);
            }
            return Ok(true);
        }
        if idx == 0 {
            return Ok(true);
        }
        BaseObject::delete_str(rt, o, name, throw)
    }

    pub fn delete_idx(rt: &mut Runtime, o: ObjRef, idx: i64, throw: bool) -> JsResult<bool> {
        let a = TypedArrayObject::data(rt, o);
        if !a.data.borrow().detached && idx >= 0 && idx < a.length {
            let s = Value::Object(o).go_string(rt)?;
            rt.type_error_result(throw, &format!("Cannot delete property '{idx}' of {s}"))?;
            return Ok(false);
        }
        Ok(true)
    }

    pub fn string_keys(
        rt: &mut Runtime,
        o: ObjRef,
        all: bool,
        mut accum: Vec<Value>,
    ) -> JsResult<Vec<Value>> {
        let length = TypedArrayObject::data(rt, o).length;
        accum.reserve(length.max(0) as usize);
        for i in 0..length {
            accum.push(Value::String(ascii_string(&i.to_string())));
        }
        BaseObject::string_keys(rt, o, all, accum)
    }

    pub fn iterate_string_keys(_rt: &mut Runtime, o: ObjRef) -> JsResult<IterNextFunc> {
        Ok(Box::new(TypedArrayPropIter {
            a: o,
            idx: 0,
            base: None,
        }))
    }

    pub fn export_to_bytes(rt: &Runtime, o: ObjRef) -> Vec<u8> {
        let a = TypedArrayObject::data(rt, o);
        let es = a.elem_size as usize;
        let start = a.offset as usize * es;
        let end = (a.offset + a.length) as usize * es;
        a.data.borrow().bytes()[start..end].to_vec()
    }

    pub fn export(rt: &Runtime, o: ObjRef) -> DocValue {
        let a = TypedArrayObject::data(rt, o);
        a.kind.export(&a.data, a.offset as usize, a.length as usize)
    }
}

pub struct TypedArrayPropIter {
    a: ObjRef,
    idx: i64,
    base: Option<IterNextFunc>,
}

impl PropIter for TypedArrayPropIter {
    fn next(&mut self, rt: &mut Runtime) -> JsResult<Option<PropIterItem>> {
        if self.base.is_none() {
            if self.idx < TypedArrayObject::data(rt, self.a).length {
                let name = Value::String(ascii_string(&self.idx.to_string()));
                let prop = TypedArrayObject::_get_idx(rt, self.a, self.idx);
                self.idx += 1;
                return Ok(Some(PropIterItem {
                    name,
                    value: prop,
                    enumerable: EnumerableFlag::Unknown,
                }));
            }
            self.base = Some(BaseObject::iterate_string_keys(rt, self.a)?);
        }
        match &mut self.base {
            Some(b) => b.next(rt),
            None => Ok(None),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        t.obj(self.a);
        if let Some(b) = &self.base {
            b.trace(t);
        }
    }
}

pub type TypedArrayObjectCtor = fn(&mut Runtime, ObjRef, i64, i64, Option<ObjRef>) -> ObjRef;

impl Runtime {
    pub fn typed_array(&self, o: ObjRef) -> Option<TypedArray> {
        let a = self.ext_dyn::<TypedArrayObject>(o)?;
        Some(TypedArray {
            obj: o,
            viewed_array_buf: a.viewed_array_buf,
            data: a.data.clone(),
            default_ctor: a.default_ctor,
            length: a.length,
            offset: a.offset,
            elem_size: a.elem_size,
            kind: a.kind,
        })
    }

    pub fn is_typed_array(&self, o: ObjRef) -> bool {
        self.ext_dyn::<TypedArrayObject>(o).is_some()
    }

    pub fn array_buffer_data(&self, o: ObjRef) -> Option<SharedBytes> {
        self.ext_dyn::<ArrayBufferObject>(o).map(|b| b.data.clone())
    }

    pub fn data_view(&self, o: ObjRef) -> Option<(ObjRef, SharedBytes, i64, i64)> {
        self.ext_dyn::<DataViewObject>(o).map(|d| {
            (
                d.viewed_array_buf,
                d.data.clone(),
                d.byte_len,
                d.byte_offset,
            )
        })
    }

    pub fn set_typed_array_bounds(&mut self, o: ObjRef, offset: i64, length: i64) {
        if let Some(a) = self.ext_dyn_mut::<TypedArrayObject>(o) {
            a.offset = offset;
            a.length = length;
        }
    }

    pub fn new_array_buffer(&mut self, data: Vec<u8>) -> ObjRef {
        let proto = self.get_array_buffer_prototype();
        let buf = self._new_array_buffer(Some(proto), None);
        *self.array_buffer_shared(buf).borrow_mut() = ByteBuf::from_vec(data);
        buf
    }

    pub fn array_buffer_shared(&self, o: ObjRef) -> SharedBytes {
        match self.ext_dyn::<ArrayBufferObject>(o) {
            Some(b) => b.data.clone(),
            None => unreachable!("not an ArrayBuffer"),
        }
    }

    pub fn array_buffer_bytes(&self, o: ObjRef) -> Option<Vec<u8>> {
        let data = self.ext_dyn::<ArrayBufferObject>(o)?.data.clone();
        let b = data.borrow();
        if b.is_nil() {
            return None;
        }
        Some(b.data.clone())
    }

    pub fn array_buffer_detach(&mut self, o: ObjRef) -> bool {
        let Some(b) = self.ext_dyn::<ArrayBufferObject>(o) else {
            return false;
        };
        let mut d = b.data.borrow_mut();
        if d.detached {
            return false;
        }
        d.detached = true;
        true
    }

    pub fn array_buffer_detached(&self, o: ObjRef) -> bool {
        self.ext_dyn::<ArrayBufferObject>(o)
            .is_some_and(|b| b.data.borrow().detached)
    }

    pub fn _new_array_buffer(&mut self, proto: Option<ObjRef>, o: Option<ObjRef>) -> ObjRef {
        let base = BaseObject::new(CLASS_OBJECT, proto);
        let ext = ObjExt::Dyn(Box::new(ArrayBufferObject {
            data: ByteBuf::new_nil().shared(),
        }));
        match o {
            Some(o) => {
                self.set_object_impl(o, &ARRAY_BUFFER_CLASS, base, ext);
                o
            }
            None => self.alloc_object(&ARRAY_BUFFER_CLASS, base, ext),
        }
    }

    pub fn new_data_view_object(
        &mut self,
        proto: Option<ObjRef>,
        buffer: ObjRef,
        byte_offset: i64,
        byte_len: i64,
    ) -> ObjRef {
        let data = self.array_buffer_shared(buffer);
        self.alloc_object(
            &DATA_VIEW_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(DataViewObject {
                viewed_array_buf: buffer,
                data,
                byte_len,
                byte_offset,
            })),
        )
    }

    #[allow(clippy::too_many_arguments)]
    pub fn _new_typed_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        elem_size: i64,
        def_ctor: ObjRef,
        kind: TypedKind,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let data = self.array_buffer_shared(buf);
        self.alloc_object(
            &TYPED_ARRAY_CLASS,
            BaseObject::new(CLASS_OBJECT, proto),
            ObjExt::Dyn(Box::new(TypedArrayObject {
                viewed_array_buf: buf,
                data,
                default_ctor: def_ctor,
                length,
                offset,
                elem_size,
                kind,
            })),
        )
    }

    pub fn new_uint8_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_uint8_array();
        self._new_typed_array_object(buf, offset, length, 1, c, TypedKind::Uint8, proto)
    }

    pub fn new_uint8_clamped_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_uint8_clamped_array();
        self._new_typed_array_object(buf, offset, length, 1, c, TypedKind::Uint8Clamped, proto)
    }

    pub fn new_int8_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_int8_array();
        self._new_typed_array_object(buf, offset, length, 1, c, TypedKind::Int8, proto)
    }

    pub fn new_uint16_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_uint16_array();
        self._new_typed_array_object(buf, offset, length, 2, c, TypedKind::Uint16, proto)
    }

    pub fn new_int16_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_int16_array();
        self._new_typed_array_object(buf, offset, length, 2, c, TypedKind::Int16, proto)
    }

    pub fn new_uint32_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_uint32_array();
        self._new_typed_array_object(buf, offset, length, 4, c, TypedKind::Uint32, proto)
    }

    pub fn new_int32_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_int32_array();
        self._new_typed_array_object(buf, offset, length, 4, c, TypedKind::Int32, proto)
    }

    pub fn new_float32_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_float32_array();
        self._new_typed_array_object(buf, offset, length, 4, c, TypedKind::Float32, proto)
    }

    pub fn new_float64_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_float64_array();
        self._new_typed_array_object(buf, offset, length, 8, c, TypedKind::Float64, proto)
    }

    pub fn new_big_int64_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_big_int64_array();
        self._new_typed_array_object(buf, offset, length, 8, c, TypedKind::BigInt64, proto)
    }

    pub fn new_big_uint64_array_object(
        &mut self,
        buf: ObjRef,
        offset: i64,
        length: i64,
        proto: Option<ObjRef>,
    ) -> ObjRef {
        let c = self.get_big_uint64_array();
        self._new_typed_array_object(buf, offset, length, 8, c, TypedKind::BigUint64, proto)
    }

    pub fn export_to_bytes(&mut self, v: &Value) -> JsResult<Option<Vec<u8>>> {
        let Value::Object(o) = v else {
            return Ok(None);
        };
        let o = *o;
        if self.is_typed_array(o) {
            return Ok(Some(TypedArrayObject::export_to_bytes(self, o)));
        }
        if let Some((_, data, byte_len, byte_offset)) = self.data_view(o) {
            let b = data.borrow();
            let (start, end) = (byte_offset as usize, (byte_offset + byte_len) as usize);
            return Ok(Some(b.bytes()[start..end].to_vec()));
        }
        if let Some(data) = self.array_buffer_data(o) {
            return Ok(Some(data.borrow().bytes().to_vec()));
        }
        Ok(None)
    }
}

pub struct DataViewIdx {
    pub idx: usize,
    pub bo: ByteOrder,
}

impl Runtime {
    pub fn data_view_get_idx_and_byte_order(
        &mut self,
        o: ObjRef,
        get_idx: i64,
        little_endian_val: Option<&Value>,
        size: i64,
    ) -> JsResult<DataViewIdx> {
        let Some((_, data, byte_len, byte_offset)) = self.data_view(o) else {
            unreachable!("not a DataView");
        };
        ensure_not_detached(self, &data, true)?;
        if get_idx + size > byte_len {
            let e = self.get_range_error();
            return Err(Throw::Value(
                self.new_error(e, &format!("Index {get_idx} is out of bounds")),
            ));
        }
        let idx = get_idx + byte_offset;
        let bo = match little_endian_val {
            Some(v) => {
                if v.to_boolean() {
                    LITTLE_ENDIAN
                } else {
                    BIG_ENDIAN
                }
            }
            None => NATIVE_ENDIAN,
        };
        Ok(DataViewIdx {
            idx: idx as usize,
            bo,
        })
    }
}

impl ByteBuf {
    fn get_n<const N: usize>(&self, idx: usize, bo: ByteOrder) -> [u8; N] {
        let mut b = rd::<N>(&self.data, idx);
        if bo != NATIVE_ENDIAN {
            b.reverse();
        }
        b
    }

    fn set_n<const N: usize>(&mut self, idx: usize, mut b: [u8; N], bo: ByteOrder) {
        if bo != NATIVE_ENDIAN {
            b.reverse();
        }
        self.data[idx..idx + N].copy_from_slice(&b);
    }

    pub fn get_float32(&self, idx: usize, bo: ByteOrder) -> f32 {
        f32::from_bits(self.get_uint32(idx, bo))
    }

    pub fn set_float32(&mut self, idx: usize, val: f32, bo: ByteOrder) {
        self.set_uint32(idx, val.to_bits(), bo);
    }

    pub fn get_float64(&self, idx: usize, bo: ByteOrder) -> f64 {
        f64::from_bits(self.get_uint64(idx, bo))
    }

    pub fn set_float64(&mut self, idx: usize, val: f64, bo: ByteOrder) {
        self.set_uint64(idx, float64_bits(val), bo);
    }

    pub fn get_uint64(&self, idx: usize, bo: ByteOrder) -> u64 {
        u64::from_ne_bytes(self.get_n::<8>(idx, bo))
    }

    pub fn set_uint64(&mut self, idx: usize, val: u64, bo: ByteOrder) {
        self.set_n::<8>(idx, val.to_ne_bytes(), bo);
    }

    pub fn get_uint32(&self, idx: usize, bo: ByteOrder) -> u32 {
        u32::from_ne_bytes(self.get_n::<4>(idx, bo))
    }

    pub fn set_uint32(&mut self, idx: usize, val: u32, bo: ByteOrder) {
        self.set_n::<4>(idx, val.to_ne_bytes(), bo);
    }

    pub fn get_big_int64(&self, idx: usize, bo: ByteOrder) -> big::Int {
        big::Int::new_int(i64::from_ne_bytes(self.get_n::<8>(idx, bo)))
    }

    pub fn get_big_uint64(&self, idx: usize, bo: ByteOrder) -> big::Int {
        big::Int::from_u64(u64::from_ne_bytes(self.get_n::<8>(idx, bo)))
    }

    pub fn set_big_int64(&mut self, idx: usize, val: &big::Int, bo: ByteOrder) {
        self.set_n::<8>(idx, val.int64().to_ne_bytes(), bo);
    }

    pub fn set_big_uint64(&mut self, idx: usize, val: &big::Int, bo: ByteOrder) {
        self.set_n::<8>(idx, val.uint64().to_ne_bytes(), bo);
    }

    pub fn get_uint16(&self, idx: usize, bo: ByteOrder) -> u16 {
        u16::from_ne_bytes(self.get_n::<2>(idx, bo))
    }

    pub fn set_uint16(&mut self, idx: usize, val: u16, bo: ByteOrder) {
        self.set_n::<2>(idx, val.to_ne_bytes(), bo);
    }

    pub fn get_uint8(&self, idx: usize) -> u8 {
        self.data[idx]
    }

    pub fn set_uint8(&mut self, idx: usize, val: u8) {
        self.data[idx] = val;
    }

    pub fn get_int32(&self, idx: usize, bo: ByteOrder) -> i32 {
        self.get_uint32(idx, bo) as i32
    }

    pub fn set_int32(&mut self, idx: usize, val: i32, bo: ByteOrder) {
        self.set_uint32(idx, val as u32, bo);
    }

    pub fn get_int16(&self, idx: usize, bo: ByteOrder) -> i16 {
        self.get_uint16(idx, bo) as i16
    }

    pub fn set_int16(&mut self, idx: usize, val: i16, bo: ByteOrder) {
        self.set_uint16(idx, val as u16, bo);
    }

    pub fn get_int8(&self, idx: usize) -> i8 {
        self.data[idx] as i8
    }

    pub fn set_int8(&mut self, idx: usize, val: i8) {
        self.set_uint8(idx, val as u8);
    }

    pub fn detach(&mut self) {
        self.detached = true;
    }
}
