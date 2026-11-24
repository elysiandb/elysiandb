use crate::builtin_map::MapObject;
use crate::builtin_promise::Promise;
use crate::builtin_set::SetObject;
use crate::compiler::{MASK_CONST, MASK_STRICT, MASK_TYP, MASK_VAR};
use crate::date::DateObject;
use crate::func::{FuncKind, NativeCallable, NativeCtor, NativeFn};
use crate::heap::{FxHashMap, ObjRef, Tracer};
use crate::host::dynamic::{DynamicArrayObject, DynamicObjectData};
use crate::host::goarray_reflect::GoPairArray;
use crate::host::gomap::GoMapSimple;
use crate::host::goreflect::{GoReflect, MethodCall};
use crate::host::goslice::{GoSlice, SliceData};
use crate::host::goslice_reflect::typed_slice_export;
use crate::host::{
    DocMap, DocValue, FuncRef, HandleKind, HostFn, JsHandle, MapRef, Pair, SliceHeader,
};
use crate::object::{BaseObject, FunctionCall, ObjExt};
use crate::runtime::{JsResult, RunError, Runtime, Throw};
use crate::string::{JsString, STRING_EMPTY};
use crate::string_ascii::AsciiString;
use crate::string_imported::ImportedString;
use crate::value::{
    ERR_ACCESS_BEFORE_INIT, ERR_ASSIGN_TO_CONST, Value, float_to_value, int_to_value,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

pub fn string_from_go(s: &[u8]) -> JsString {
    if s.len() <= 16 {
        if s.is_ascii() {
            return JsString::Ascii(AsciiString::new(s));
        }
        let imp = ImportedString::new(s);
        imp.unicode();
        return JsString::Imported(imp);
    }
    JsString::Imported(ImportedString::new(s))
}

pub fn string_from_go_shared(s: &Arc<str>) -> JsString {
    let b: Arc<[u8]> = Arc::from(s.clone());
    if b.len() <= 16 {
        if b.is_ascii() {
            return JsString::Ascii(AsciiString::from_arc(b));
        }
        let imp = ImportedString::from_arc(b);
        imp.unicode();
        return JsString::Imported(imp);
    }
    JsString::Imported(ImportedString::from_arc(b))
}

pub fn scalar_value(v: &DocValue) -> Option<Value> {
    match v {
        DocValue::Nil => Some(Value::Null),
        DocValue::Bool(b) => Some(Value::Bool(*b)),
        DocValue::Int64(i) => Some(int_to_value(*i)),
        DocValue::Float64(f) => Some(float_to_value(*f)),
        _ => None,
    }
}

pub fn go_string_of(s: &JsString) -> String {
    match String::from_utf8(s.go_string()) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    }
}

pub(crate) struct FuncWrapper {
    pub fref: FuncRef,
}

impl NativeCallable for FuncWrapper {
    fn call(&self, rt: &mut Runtime, call: FunctionCall<'_>) -> JsResult<Value> {
        match &self.fref {
            FuncRef::Js {
                obj, runtime, ctor, ..
            } => {
                if *runtime != rt.host.id() {
                    return Err(Throw::Value(
                        rt.new_type_error("Illegal runtime transition of an Object"),
                    ));
                }
                if *ctor {
                    let Some(c) = obj.assert_constructor(rt) else {
                        return Err(Throw::TypeError("Not a constructor".to_string()));
                    };
                    let r = c.construct(rt, call.arguments, None)?;
                    return Ok(Value::Object(r));
                }
                match obj.assert_callable(rt) {
                    Some(f) => f.call(rt, call.this.clone(), call.arguments),
                    None => Ok(Value::Undefined),
                }
            }
            FuncRef::Native(h) => (h.f)(rt, call),
            FuncRef::Method { recv, idx } => recv.call(rt, *idx, call),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        match &self.fref {
            FuncRef::Js { obj, .. } => t.obj(*obj),
            FuncRef::Native(_) => {}
            FuncRef::Method { recv, .. } => recv.trace(t),
        }
    }

    fn as_any(&self) -> Option<&dyn std::any::Any> {
        Some(self)
    }
}

pub(crate) struct CtorWrapper {
    pub fref: FuncRef,
}

impl crate::func::NativeConstructor for CtorWrapper {
    fn construct(
        &self,
        rt: &mut Runtime,
        args: &[Value],
        _new_target: Option<ObjRef>,
    ) -> JsResult<ObjRef> {
        let w = FuncWrapper {
            fref: self.fref.clone(),
        };
        let this = Value::Undefined;
        match w.call(rt, FunctionCall::new(&this, args))? {
            Value::Object(o) => Ok(o),
            _ => Err(Throw::TypeError("Not a constructor".to_string())),
        }
    }

    fn trace(&self, t: &mut Tracer) {
        if let FuncRef::Js { obj, .. } = &self.fref {
            t.obj(*obj);
        }
    }
}

enum Slot {
    Root,
    Map(MapRef, String),
    Elem(Rc<RefCell<Vec<DocValue>>>, usize),
    Pair(Rc<RefCell<Vec<Pair>>>, usize, usize),
}

enum Frame {
    Object {
        obj: ObjRef,
        keys: Vec<crate::unistring::String>,
        idx: usize,
        target: MapRef,
    },
    Array {
        obj: ObjRef,
        fast: bool,
        len: i64,
        idx: i64,
        target: Rc<RefCell<Vec<DocValue>>>,
    },
    Sparse {
        obj: ObjRef,
        items: Vec<(u32, Value)>,
        item: usize,
        next: u32,
        len: u32,
        target: Rc<RefCell<Vec<DocValue>>>,
    },
    MapObj {
        obj: ObjRef,
        it: crate::map::OrderedMapIter,
        n: usize,
        i: usize,
        pending: Option<Value>,
        target: Rc<RefCell<Vec<Pair>>>,
    },
    SetObj {
        obj: ObjRef,
        it: crate::map::OrderedMapIter,
        n: usize,
        i: usize,
        target: Rc<RefCell<Vec<DocValue>>>,
    },
}

thread_local! {
    static EXPORTER: std::cell::Cell<Option<Exporter>> = const { std::cell::Cell::new(None) };
}

struct Exporter {
    cache: FxHashMap<ObjRef, DocValue>,
    stack: Vec<Frame>,
    root: DocValue,
}

fn store(slot: Slot, v: DocValue, root: &mut DocValue) {
    match slot {
        Slot::Root => *root = v,
        Slot::Map(m, k) => {
            let old = m.borrow_mut().insert(k, v);
            drop(old);
        }
        Slot::Elem(b, i) => {
            let old = std::mem::replace(&mut b.borrow_mut()[i], v);
            drop(old);
        }
        Slot::Pair(b, i, j) => {
            let old = std::mem::replace(&mut b.borrow_mut()[i][j], v);
            drop(old);
        }
    }
}

fn new_backing(n: usize) -> Rc<RefCell<Vec<DocValue>>> {
    let mut v = Vec::with_capacity(n);
    v.resize_with(n, DocValue::default);
    Rc::new(RefCell::new(v))
}

fn slice_of(b: &Rc<RefCell<Vec<DocValue>>>) -> DocValue {
    let len = b.borrow().len();
    DocValue::Slice(SliceHeader {
        backing: b.clone(),
        len,
    })
}

impl Exporter {
    fn value(&mut self, rt: &mut Runtime, v: &Value, slot: Slot) -> JsResult<()> {
        let dv = match v {
            Value::Object(o) => self.object(rt, *o)?,
            other => export_primitive(rt, other),
        };
        store(slot, dv, &mut self.root);
        Ok(())
    }

    fn push(&mut self, rt: &mut Runtime, o: ObjRef, f: Frame) {
        rt.handles.push(o);
        self.stack.push(f);
    }

    fn object(&mut self, rt: &mut Runtime, o: ObjRef) -> JsResult<DocValue> {
        if let Some(dv) = export_host(rt, o) {
            return Ok(dv);
        }
        enum Kind {
            Array(bool, i64),
            Sparse(Vec<(u32, Value)>, u32),
            Args(i64),
            Map(usize),
            Set(usize),
            Date(Option<gocompat::time::Time>),
            Promise,
            Proxy,
            Primitive(Value),
            Func(bool),
            Base,
        }
        let kind = {
            let obj = rt.obj(o);
            match &obj.ext {
                ObjExt::Array(a) => {
                    let fast = a.prop_value_count == 0
                        && a.length as usize == a.values.len()
                        && a.obj_count == a.length as usize;
                    Kind::Array(fast, i64::from(a.length))
                }
                ObjExt::SparseArray(a) => Kind::Sparse(
                    a.items.iter().map(|i| (i.idx, i.value.clone())).collect(),
                    a.length,
                ),
                ObjExt::Arguments(a) => Kind::Args(a.length as i64),
                ObjExt::Primitive(v) => Kind::Primitive(v.clone()),
                ObjExt::JsFunc(f) => Kind::Func(f.kind == FuncKind::Class),
                ObjExt::NativeFunc(_) => Kind::Func(false),
                ObjExt::Template(t) if t.f.is_some() || t.construct.is_some() => Kind::Func(false),
                ObjExt::Dyn(d) => {
                    let a = d.as_any();
                    if let Some(m) = a.downcast_ref::<MapObject>() {
                        Kind::Map(m.m.size())
                    } else if let Some(s) = a.downcast_ref::<SetObject>() {
                        Kind::Set(s.m.size())
                    } else if let Some(d) = a.downcast_ref::<DateObject>() {
                        Kind::Date(d.export())
                    } else if a.downcast_ref::<Promise>().is_some() {
                        Kind::Promise
                    } else if rt.is_proxy(o) {
                        Kind::Proxy
                    } else {
                        Kind::Base
                    }
                }
                _ if rt.is_proxy(o) => Kind::Proxy,
                _ => Kind::Base,
            }
        };
        let cached = matches!(
            kind,
            Kind::Array(..) | Kind::Sparse(..) | Kind::Args(_) | Kind::Base
        );
        if cached && let Some(v) = self.cache.get(&o) {
            return Ok(v.clone());
        }
        Ok(match kind {
            Kind::Array(fast, len) => {
                let target = new_backing(len as usize);
                let dv = slice_of(&target);
                self.cache.insert(o, dv.clone());
                self.push(
                    rt,
                    o,
                    Frame::Array {
                        obj: o,
                        fast,
                        len,
                        idx: 0,
                        target,
                    },
                );
                dv
            }
            Kind::Args(len) => {
                let target = new_backing(len as usize);
                let dv = slice_of(&target);
                self.cache.insert(o, dv.clone());
                self.push(
                    rt,
                    o,
                    Frame::Array {
                        obj: o,
                        fast: false,
                        len,
                        idx: 0,
                        target,
                    },
                );
                dv
            }
            Kind::Sparse(items, len) => {
                let target = new_backing(len as usize);
                let dv = slice_of(&target);
                self.cache.insert(o, dv.clone());
                for (_, v) in &items {
                    if let Value::Object(x) = v {
                        rt.handles.push(*x);
                    }
                }
                self.push(
                    rt,
                    o,
                    Frame::Sparse {
                        obj: o,
                        items,
                        item: 0,
                        next: 0,
                        len,
                        target,
                    },
                );
                dv
            }
            Kind::Map(n) => {
                let mut v: Vec<Pair> = Vec::with_capacity(n);
                v.resize_with(n, Default::default);
                let target = Rc::new(RefCell::new(v));
                let dv = DocValue::Pairs(crate::host::TypedSlice {
                    backing: target.clone(),
                    len: n,
                });
                self.cache.insert(o, dv.clone());
                let it = match rt.ext_dyn::<MapObject>(o) {
                    Some(m) => m.m.new_iter(),
                    None => crate::map::OrderedMapIter::new(),
                };
                self.push(
                    rt,
                    o,
                    Frame::MapObj {
                        obj: o,
                        it,
                        n,
                        i: 0,
                        pending: None,
                        target,
                    },
                );
                dv
            }
            Kind::Set(n) => {
                let target = new_backing(n);
                let dv = slice_of(&target);
                self.cache.insert(o, dv.clone());
                let it = match rt.ext_dyn::<SetObject>(o) {
                    Some(m) => m.m.new_iter(),
                    None => crate::map::OrderedMapIter::new(),
                };
                self.push(
                    rt,
                    o,
                    Frame::SetObj {
                        obj: o,
                        it,
                        n,
                        i: 0,
                        target,
                    },
                );
                dv
            }
            Kind::Date(t) => match t {
                Some(t) => DocValue::Time(t),
                None => DocValue::Nil,
            },
            Kind::Promise => js_handle(rt, o, HandleKind::Promise),
            Kind::Proxy => js_handle(rt, o, HandleKind::Proxy),
            Kind::Primitive(v) => export_primitive(rt, &v),
            Kind::Func(ctor) => export_func(rt, o, ctor),
            Kind::Base => {
                let keys = {
                    let base = &mut rt.obj_mut(o).base;
                    base.ensure_prop_order();
                    base.prop_names
                        .iter()
                        .filter(|k| {
                            !matches!(base.values.get(*k), Some(Value::Property(p)) if !p.enumerable)
                        })
                        .cloned()
                        .collect()
                };
                let target: MapRef = Rc::new(RefCell::new(DocMap::new()));
                let dv = DocValue::Map(target.clone());
                self.cache.insert(o, dv.clone());
                self.push(
                    rt,
                    o,
                    Frame::Object {
                        obj: o,
                        keys,
                        idx: 0,
                        target,
                    },
                );
                dv
            }
        })
    }

    fn step(&mut self, rt: &mut Runtime) -> JsResult<bool> {
        let Some(top) = self.stack.last_mut() else {
            return Ok(false);
        };
        match top {
            Frame::Object {
                obj,
                keys,
                idx,
                target,
            } => {
                if *idx >= keys.len() {
                    self.stack.pop();
                    return Ok(true);
                }
                let name = keys[*idx].clone();
                *idx += 1;
                let (o, target) = (*obj, target.clone());
                let name_str = name.string();
                match o.get_str(rt, &name, None)? {
                    Some(v) => self.value(rt, &v, Slot::Map(target, name_str))?,
                    None => store(Slot::Map(target, name_str), DocValue::Nil, &mut self.root),
                }
            }
            Frame::Array {
                obj,
                fast,
                len,
                idx,
                target,
            } => {
                if *idx >= *len {
                    self.stack.pop();
                    return Ok(true);
                }
                let i = *idx;
                *idx += 1;
                let (o, fast, target) = (*obj, *fast, target.clone());
                let v = if fast {
                    match &rt.obj(o).ext {
                        ObjExt::Array(a) => a.values.get(i as usize).cloned(),
                        _ => None,
                    }
                    .filter(|v| !v.is_empty())
                } else {
                    o.get_idx(rt, i, None)?
                };
                if let Some(v) = v {
                    self.value(rt, &v, Slot::Elem(target, i as usize))?;
                }
            }
            Frame::Sparse {
                obj,
                items,
                item,
                next,
                len,
                target,
            } => {
                let o = *obj;
                let target = target.clone();
                let gap_end = if *item < items.len() {
                    items[*item].0
                } else {
                    *len
                };
                if *next < gap_end {
                    let i = *next;
                    *next += 1;
                    if let Some(proto) = rt.obj(o).base.prototype
                        && let Some(v) = proto.get_idx(rt, i64::from(i), None)?
                    {
                        self.value(rt, &v, Slot::Elem(target, i as usize))?;
                    }
                    return Ok(true);
                }
                if *item >= items.len() {
                    self.stack.pop();
                    return Ok(true);
                }
                let (idx, v) = items[*item].clone();
                *item += 1;
                *next = idx + 1;
                let v = match v {
                    Value::Property(p) => Some(p.get(rt, &Value::Object(o))?),
                    Value::Empty => None,
                    v => Some(v),
                };
                if let Some(v) = v {
                    self.value(rt, &v, Slot::Elem(target, idx as usize))?;
                }
            }
            Frame::MapObj {
                obj,
                it,
                n,
                i,
                pending,
                target,
            } => {
                let target = target.clone();
                if let Some(v) = pending.take() {
                    let at = *i;
                    *i += 1;
                    self.value(rt, &v, Slot::Pair(target, at, 1))?;
                    return Ok(true);
                }
                if *i >= *n {
                    self.stack.pop();
                    return Ok(true);
                }
                let (o, at) = (*obj, *i);
                let mut cur = *it;
                let entry = match rt.ext_dyn_mut::<MapObject>(o) {
                    Some(m) => m.m.iter_next(&mut cur),
                    None => None,
                };
                *it = cur;
                match entry {
                    None => {
                        self.stack.pop();
                    }
                    Some((k, v)) => {
                        if let Value::Object(x) = &v {
                            rt.handles.push(*x);
                        }
                        *pending = Some(v);
                        self.value(rt, &k, Slot::Pair(target, at, 0))?;
                    }
                }
            }
            Frame::SetObj {
                obj,
                it,
                n,
                i,
                target,
            } => {
                if *i >= *n {
                    self.stack.pop();
                    return Ok(true);
                }
                let (o, at) = (*obj, *i);
                *i += 1;
                let target = target.clone();
                let mut cur = *it;
                let entry = match rt.ext_dyn_mut::<SetObject>(o) {
                    Some(m) => m.m.iter_next(&mut cur),
                    None => None,
                };
                *it = cur;
                match entry {
                    None => {
                        self.stack.pop();
                    }
                    Some((k, _)) => self.value(rt, &k, Slot::Elem(target, at))?,
                }
            }
        }
        Ok(true)
    }
}

fn js_handle(rt: &mut Runtime, o: ObjRef, kind: HandleKind) -> DocValue {
    rt.host.keep(o);
    DocValue::Object(JsHandle {
        obj: o,
        runtime: rt.host.id(),
        kind,
    })
}

fn native_wrapper(rt: &Runtime, o: ObjRef) -> Option<FuncRef> {
    let ObjExt::NativeFunc(nf) = &rt.obj(o).ext else {
        return None;
    };
    let Some(NativeFn::Dyn(d)) = &nf.f else {
        return None;
    };
    let a = d.as_any()?;
    if let Some(w) = a.downcast_ref::<FuncWrapper>() {
        return Some(w.fref.clone());
    }
    if let Some(m) = a.downcast_ref::<MethodCall>() {
        return Some(FuncRef::Method {
            recv: m.recv.clone(),
            idx: m.idx,
        });
    }
    None
}

fn export_func(rt: &mut Runtime, o: ObjRef, ctor: bool) -> DocValue {
    if let Some(f) = native_wrapper(rt, o) {
        if let FuncRef::Method { recv, .. } = &f {
            rt.host.keep_host(recv);
        }
        return DocValue::Func(f);
    }
    let name = match BaseObject::get_own_prop_str(rt, o, &"name".into()) {
        Ok(Some(Value::Property(p))) => match &p.value {
            Value::String(s) => go_string_of(s),
            _ => String::new(),
        },
        Ok(Some(Value::String(s))) => go_string_of(&s),
        _ => String::new(),
    };
    rt.host.keep(o);
    DocValue::Func(FuncRef::Js {
        obj: o,
        runtime: rt.host.id(),
        name,
        ctor,
    })
}

fn export_host(rt: &Runtime, o: ObjRef) -> Option<DocValue> {
    let ObjExt::Dyn(d) = &rt.obj(o).ext else {
        return None;
    };
    let a = d.as_any();
    if let Some(m) = a.downcast_ref::<GoMapSimple>() {
        return Some(DocValue::Map(m.data.clone()));
    }
    if a.is::<GoSlice>() {
        return Some(GoSlice::export(rt, o));
    }
    if let Some(r) = a.downcast_ref::<GoReflect>() {
        return Some(r.value.export_value(&r.value));
    }
    if let Some(p) = a.downcast_ref::<GoPairArray>() {
        return Some(p.export());
    }
    if let Some(dv) = typed_slice_export(a) {
        return Some(dv);
    }
    if let Some(d) = a.downcast_ref::<DynamicObjectData>() {
        return Some(d.d.clone().export());
    }
    if let Some(d) = a.downcast_ref::<DynamicArrayObject>() {
        return Some(d.a.clone().export());
    }
    None
}

pub fn export_primitive(rt: &Runtime, v: &Value) -> DocValue {
    match v {
        Value::Bool(b) => DocValue::Bool(*b),
        Value::Int(i) => DocValue::Int64(*i),
        Value::Float(f) => DocValue::Float64(*f),
        Value::String(s) => DocValue::String(go_string_of(s).into()),
        Value::BigInt(b) => DocValue::BigInt(b.clone()),
        Value::Symbol(s) => DocValue::String(match rt.symbol_desc(*s) {
            Some(d) => go_string_of(&d).into(),
            None => Arc::from(""),
        }),
        _ => DocValue::Nil,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportType {
    None,
    Nil,
    Bool,
    Int64,
    Float64,
    String,
    BigInt,
    Map,
    Slice,
    SlicePtr,
    Time,
    Pairs,
    TypedSlice,
    Func,
    Ctor,
    Promise,
    Proxy,
    Host,
    Dynamic,
}

impl Runtime {
    pub fn export_type(&self, v: &Value) -> ExportType {
        match v {
            Value::Undefined | Value::Empty => ExportType::None,
            Value::Null => ExportType::Nil,
            Value::Bool(_) => ExportType::Bool,
            Value::Int(_) => ExportType::Int64,
            Value::Float(_) => ExportType::Float64,
            Value::String(_) | Value::Symbol(_) => ExportType::String,
            Value::BigInt(_) => ExportType::BigInt,
            Value::Object(o) => self.export_type_object(*o),
            _ => ExportType::None,
        }
    }

    fn export_type_object(&self, o: ObjRef) -> ExportType {
        match &self.obj(o).ext {
            ObjExt::Array(_) | ObjExt::SparseArray(_) | ObjExt::Arguments(_) => ExportType::Slice,
            ObjExt::Primitive(v) => self.export_type(v),
            ObjExt::JsFunc(f) if f.kind == FuncKind::Class => ExportType::Ctor,
            ObjExt::JsFunc(_) | ObjExt::NativeFunc(_) => ExportType::Func,
            ObjExt::Template(t) if t.f.is_some() || t.construct.is_some() => ExportType::Func,
            ObjExt::Dyn(d) => {
                let a = d.as_any();
                if a.is::<GoMapSimple>() {
                    ExportType::Map
                } else if let Some(s) = a.downcast_ref::<GoSlice>() {
                    if s.orig_is_ptr() {
                        ExportType::SlicePtr
                    } else {
                        ExportType::Slice
                    }
                } else if let Some(r) = a.downcast_ref::<GoReflect>() {
                    if r.value.as_any().is::<crate::host::gotime::GoTime>() {
                        ExportType::Time
                    } else {
                        ExportType::Host
                    }
                } else if a.is::<GoPairArray>() {
                    ExportType::Pairs
                } else if let Some(t) = typed_slice_export(a) {
                    match &t {
                        DocValue::Pairs(_) => ExportType::Pairs,
                        _ => ExportType::TypedSlice,
                    }
                } else if a.is::<DynamicObjectData>() || a.is::<DynamicArrayObject>() {
                    ExportType::Dynamic
                } else if a.is::<MapObject>() {
                    ExportType::Pairs
                } else if a.is::<SetObject>() {
                    ExportType::Slice
                } else if a.is::<DateObject>() {
                    ExportType::Time
                } else if a.is::<Promise>() {
                    ExportType::Promise
                } else if self.is_proxy(o) {
                    ExportType::Proxy
                } else {
                    ExportType::Map
                }
            }
            _ if self.is_proxy(o) => ExportType::Proxy,
            _ => ExportType::Map,
        }
    }

    pub fn export(&mut self, v: &Value) -> JsResult<DocValue> {
        let Value::Object(o) = v else {
            return Ok(export_primitive(self, v));
        };
        if let Some(dv) = export_host(self, *o) {
            return Ok(dv);
        }
        let mark = self.handle_scope_enter();
        let mut ex = EXPORTER.take().unwrap_or_else(|| Exporter {
            cache: FxHashMap::default(),
            stack: Vec::new(),
            root: DocValue::Nil,
        });
        let r = (|| -> JsResult<()> {
            ex.value(self, v, Slot::Root)?;
            while ex.step(self)? {}
            Ok(())
        })();
        self.handle_scope_leave(mark);
        let root = std::mem::take(&mut ex.root);
        ex.cache.clear();
        ex.stack.clear();
        EXPORTER.set(Some(ex));
        r?;
        Ok(root)
    }

    pub fn to_value(&mut self, v: &DocValue) -> JsResult<Value> {
        Ok(match v {
            DocValue::Nil => Value::Null,
            DocValue::Bool(b) => Value::Bool(*b),
            DocValue::Int64(i) => int_to_value(*i),
            DocValue::Float64(f) => float_to_value(*f),
            DocValue::String(s) => Value::String(string_from_go_shared(s)),
            DocValue::Map(m) => Value::Object(self.new_go_map_simple(m.clone())),
            DocValue::Slice(h) => Value::Object(self.new_go_slice(SliceData::Own(h.clone()))),
            DocValue::SlicePtr(p) => Value::Object(self.new_go_slice(SliceData::Ptr(p.clone()))),
            DocValue::Time(t) => Value::Object(GoReflect::new_time(self, t.clone())),
            DocValue::BigInt(b) => Value::BigInt(Arc::new((**b).clone())),
            DocValue::Func(f) => Value::Object(self.new_func_ref(f)),
            DocValue::Host(h) => Value::Object(self.new_go_reflect(h.clone())),
            DocValue::Object(h) => {
                if h.runtime != self.host.id() {
                    let what = match h.kind {
                        HandleKind::Proxy => "a Proxy",
                        HandleKind::Promise => "a Promise",
                        HandleKind::ArrayBuffer => "an ArrayBuffer",
                    };
                    return Err(Throw::Value(
                        self.new_type_error(&format!("Illegal runtime transition of {what}")),
                    ));
                }
                Value::Object(h.obj)
            }
            DocValue::PairRef(p) => Value::Object(self.new_go_pair_array(p.clone())),
            other => Value::Object(self.new_go_typed_slice(other)),
        })
    }

    pub fn new_func_ref(&mut self, f: &FuncRef) -> ObjRef {
        let name = match f {
            FuncRef::Js { name, .. } => name.clone(),
            FuncRef::Native(h) => h.name.clone(),
            FuncRef::Method { recv, idx } => recv.methods()[*idx].name.to_string(),
        };
        let length = match f {
            FuncRef::Method { recv, idx } => recv.methods()[*idx].num_in,
            _ => 0,
        };
        let call = NativeFn::Dyn(Rc::new(FuncWrapper { fref: f.clone() }));
        if let FuncRef::Js { ctor: true, .. } = f {
            let ctor = NativeCtor::Dyn(Rc::new(CtorWrapper { fref: f.clone() }));
            let v = self.new_native_func_and_construct(
                None,
                Some(call),
                Some(ctor),
                None,
                &name,
                int_to_value(0),
            );
            let proto = self.new_object();
            proto._put_prop(
                self,
                &"constructor".into(),
                Value::Object(v),
                true,
                false,
                true,
            );
            v._put_prop(
                self,
                &"prototype".into(),
                Value::Object(proto),
                true,
                false,
                false,
            );
            return v;
        }
        self.new_native_func(call, &name, length)
    }

    pub fn new_host_func(&mut self, f: Rc<HostFn>) -> ObjRef {
        self.new_func_ref(&FuncRef::Native(f))
    }

    pub fn set_value(&mut self, name: &str, v: &DocValue) -> JsResult<()> {
        let v = self.to_value(v)?;
        self.set_var(name, v)
    }

    pub fn get_var(&mut self, name: &str) -> JsResult<Option<Value>> {
        let n = crate::unistring::String::from(name);
        let stash = self.global.stash;
        let found = {
            let s = self.heap.stash(stash);
            s.names
                .as_ref()
                .and_then(|names| names.get(&n).copied())
                .map(|idx| (idx, s.values[(idx & !MASK_TYP) as usize].clone()))
        };
        if let Some((idx, v)) = found {
            if v.is_empty() {
                if idx & MASK_VAR == 0 {
                    return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
                }
                return Ok(Some(Value::Undefined));
            }
            return Ok(Some(v));
        }
        let g = self.global_object;
        g.get_str(self, &n, None)
    }

    pub fn set_var(&mut self, name: &str, v: Value) -> JsResult<()> {
        let n = crate::unistring::String::from(name);
        let stash = self.global.stash;
        let idx = self
            .heap
            .stash(stash)
            .names
            .as_ref()
            .and_then(|names| names.get(&n).copied());
        let Some(idx) = idx else {
            let g = self.global_object;
            return g.set_own_str(self, &n, v, true).map(|_| ());
        };
        let i = (idx & !MASK_TYP) as usize;
        if idx & MASK_VAR == 0 {
            if idx & MASK_CONST != 0 {
                if idx & MASK_STRICT != 0 {
                    return Err(Throw::TypeError(ERR_ASSIGN_TO_CONST.to_string()));
                }
                return Ok(());
            }
            if self.heap.stash(stash).values[i].is_empty() {
                return Err(Throw::ReferenceError(ERR_ACCESS_BEFORE_INIT.to_string()));
            }
        }
        self.heap.stash_mut(stash).values[i] = v;
        Ok(())
    }

    pub fn new_go_error(&mut self, msg: &str) -> Value {
        let e = self.get_error();
        self.new_error(e, msg)
    }

    pub fn call_function(
        &mut self,
        f: &Value,
        this: Value,
        args: &[Value],
    ) -> Result<Value, RunError> {
        let Some(call) = crate::runtime::assert_callable(self, f) else {
            return Err(RunError::Exception(Box::new(
                crate::runtime::Exception::new(Value::String(STRING_EMPTY.clone())),
            )));
        };
        let mut ret = Value::Undefined;
        let res = self.vm_try_result(|rt| {
            ret = call.call(rt, this, args)?;
            Ok(())
        });
        let mut out = match res {
            Ok(None) => Ok(ret),
            Ok(Some(mut ex)) => {
                ex.resolve_value_string(self);
                Err(RunError::Exception(ex))
            }
            Err(t) => {
                let e = self.throw_to_run_error(t);
                if self.vm.call_stack.is_empty() {
                    self.leave_abrupt();
                }
                return Err(e);
            }
        };
        if self.vm.call_stack.is_empty() {
            if let Err(t) = self.leave() {
                let uncatchable = t.is_uncatchable();
                out = Err(self.throw_to_run_error(t));
                if uncatchable {
                    self.leave_abrupt();
                }
            }
        } else {
            self.vm.clear_stack();
        }
        out
    }
}
