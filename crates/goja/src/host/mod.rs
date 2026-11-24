pub mod dynamic;
pub mod goarray_reflect;
pub mod gomap;
pub mod goreflect;
pub mod goslice;
pub mod goslice_reflect;
pub mod gotime;
pub mod json;

use crate::array::go_append;
use crate::deps::big;
use crate::heap::{FxHashSet, ObjRef, Tracer};
use crate::object::{Flag, FunctionCall, PropertyDescriptor};
use crate::runtime::{JsResult, Runtime};
use crate::value::Value;
use gocompat::time::Time;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

pub use dynamic::{DynamicArray, DynamicObject};
pub use goreflect::{GoKind, GoMethod, GoMethods, GoReflect};

pub type DocMap = BTreeMap<String, DocValue>;
pub type MapRef = Rc<RefCell<DocMap>>;
pub type Pair = [DocValue; 2];
pub type HostRef = Rc<dyn GoMethods>;

pub struct TypedSlice<E> {
    pub backing: Rc<RefCell<Vec<E>>>,
    pub len: usize,
}

impl<E> Clone for TypedSlice<E> {
    fn clone(&self) -> TypedSlice<E> {
        TypedSlice {
            backing: self.backing.clone(),
            len: self.len,
        }
    }
}

impl<E: Clone + Default> TypedSlice<E> {
    pub fn new(items: Vec<E>) -> TypedSlice<E> {
        let len = items.len();
        TypedSlice {
            backing: Rc::new(RefCell::new(items)),
            len,
        }
    }

    pub fn with_cap(mut items: Vec<E>, cap: usize) -> TypedSlice<E> {
        let len = items.len();
        if cap > len {
            items.resize_with(cap, E::default);
        }
        TypedSlice {
            backing: Rc::new(RefCell::new(items)),
            len,
        }
    }

    pub fn decoded(items: Vec<E>) -> TypedSlice<E> {
        let mut v = Vec::new();
        for it in items {
            go_append(&mut v, it);
        }
        let cap = v.capacity();
        TypedSlice::with_cap(v, cap)
    }

    pub fn cap(&self) -> usize {
        self.backing.borrow().len()
    }

    pub fn to_vec(&self) -> Vec<E> {
        self.backing.borrow()[..self.len].to_vec()
    }

    pub fn get(&self, i: usize) -> Option<E> {
        if i < self.len {
            return self.backing.borrow().get(i).cloned();
        }
        None
    }

    pub fn same_data(&self, other: &TypedSlice<E>) -> bool {
        Rc::ptr_eq(&self.backing, &other.backing) && self.len == other.len
    }
}

pub type SliceHeader = TypedSlice<DocValue>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandleKind {
    Proxy,
    Promise,
    ArrayBuffer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct JsHandle {
    pub obj: ObjRef,
    pub runtime: u64,
    pub kind: HandleKind,
}

pub type HostFnImpl = dyn Fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value>;

pub struct HostFn {
    pub name: String,
    pub f: Box<HostFnImpl>,
}

impl HostFn {
    pub fn new(
        name: &str,
        f: impl Fn(&mut Runtime, FunctionCall<'_>) -> JsResult<Value> + 'static,
    ) -> Rc<HostFn> {
        Rc::new(HostFn {
            name: name.to_string(),
            f: Box::new(f),
        })
    }
}

#[derive(Clone)]
pub enum FuncRef {
    Js {
        obj: ObjRef,
        runtime: u64,
        name: String,
        ctor: bool,
    },
    Native(Rc<HostFn>),
    Method {
        recv: HostRef,
        idx: usize,
    },
}

#[derive(Clone, Default)]
pub enum DocValue {
    #[default]
    Nil,
    Bool(bool),
    Int64(i64),
    Float64(f64),
    String(Arc<str>),
    Map(MapRef),
    Slice(SliceHeader),
    SlicePtr(Rc<RefCell<SliceHeader>>),
    Time(Time),
    Pairs(TypedSlice<Pair>),
    PairRef(goarray_reflect::PairCell),
    Bytes(TypedSlice<u8>),
    Int8s(TypedSlice<i8>),
    Uint16s(TypedSlice<u16>),
    Int16s(TypedSlice<i16>),
    Uint32s(TypedSlice<u32>),
    Int32s(TypedSlice<i32>),
    Float32s(TypedSlice<f32>),
    Float64s(TypedSlice<f64>),
    Int64s(TypedSlice<i64>),
    Uint64s(TypedSlice<u64>),
    BigInt(Arc<big::Int>),
    Func(FuncRef),
    Host(HostRef),
    Object(JsHandle),
}

impl DocValue {
    pub fn new_map(m: DocMap) -> DocValue {
        DocValue::Map(Rc::new(RefCell::new(m)))
    }

    pub fn new_slice(items: Vec<DocValue>) -> DocValue {
        DocValue::Slice(SliceHeader::new(items))
    }

    pub fn decoded_slice(items: Vec<DocValue>) -> DocValue {
        DocValue::Slice(SliceHeader::decoded(items))
    }

    pub fn string(s: &str) -> DocValue {
        DocValue::String(Arc::from(s))
    }

    pub fn is_nil(&self) -> bool {
        matches!(self, DocValue::Nil)
    }

    pub fn as_map(&self) -> Option<&MapRef> {
        match self {
            DocValue::Map(m) => Some(m),
            _ => None,
        }
    }

    pub fn as_host<T: 'static>(&self) -> Option<&T> {
        match self {
            DocValue::Host(h) => h.as_any().downcast_ref::<T>(),
            _ => None,
        }
    }

    pub fn release_graph(&self) {
        let mut seen: FxHashSet<usize> = FxHashSet::default();
        let mut stack: Vec<DocValue> = vec![self.clone()];
        while let Some(v) = stack.pop() {
            match &v {
                DocValue::Map(m) => {
                    if seen.insert(Rc::as_ptr(m) as *const () as usize) {
                        let taken = std::mem::take(&mut *m.borrow_mut());
                        stack.extend(taken.into_values());
                    }
                }
                DocValue::Slice(h) => {
                    if seen.insert(Rc::as_ptr(&h.backing) as *const () as usize) {
                        let taken = std::mem::take(&mut *h.backing.borrow_mut());
                        stack.extend(taken);
                    }
                }
                DocValue::SlicePtr(p) => {
                    if seen.insert(Rc::as_ptr(p) as *const () as usize) {
                        let h = p.borrow().clone();
                        stack.push(DocValue::Slice(h));
                    }
                }
                DocValue::Pairs(h) => {
                    if seen.insert(Rc::as_ptr(&h.backing) as *const () as usize) {
                        let taken = std::mem::take(&mut *h.backing.borrow_mut());
                        for [a, b] in taken {
                            stack.push(a);
                            stack.push(b);
                        }
                    }
                }
                _ => {}
            }
        }
    }

    pub fn deep_copy(&self) -> DocValue {
        let mut out = DocValue::Nil;
        let mut stack: Vec<(DocValue, CopySlot)> = vec![(self.clone(), CopySlot::Root)];
        while let Some((v, slot)) = stack.pop() {
            let copied = match &v {
                DocValue::Map(m) => {
                    let target: MapRef = Rc::new(RefCell::new(DocMap::new()));
                    for (k, item) in m.borrow().iter() {
                        target.borrow_mut().insert(k.clone(), DocValue::Nil);
                        stack.push((item.clone(), CopySlot::Map(target.clone(), k.clone())));
                    }
                    DocValue::Map(target)
                }
                DocValue::Slice(h) => {
                    let items = h.to_vec();
                    let n = items.len();
                    let target = SliceHeader::new(vec![DocValue::Nil; n]);
                    for (i, item) in items.into_iter().enumerate() {
                        stack.push((item, CopySlot::Slice(target.backing.clone(), i)));
                    }
                    DocValue::Slice(target)
                }
                _ => v,
            };
            match slot {
                CopySlot::Root => out = copied,
                CopySlot::Map(m, k) => {
                    m.borrow_mut().insert(k, copied);
                }
                CopySlot::Slice(b, i) => b.borrow_mut()[i] = copied,
            }
        }
        out
    }
}

enum CopySlot {
    Root,
    Map(MapRef, String),
    Slice(Rc<RefCell<Vec<DocValue>>>, usize),
}

fn is_container(v: &DocValue) -> bool {
    matches!(
        v,
        DocValue::Map(_) | DocValue::Slice(_) | DocValue::SlicePtr(_) | DocValue::Pairs(_)
    )
}

fn has_container_children(v: &mut DocValue) -> bool {
    match v {
        DocValue::Map(m) => Rc::get_mut(m).is_some_and(|c| c.get_mut().values().any(is_container)),
        DocValue::Slice(h) => {
            Rc::get_mut(&mut h.backing).is_some_and(|c| c.get_mut().iter().any(is_container))
        }
        DocValue::SlicePtr(p) => Rc::get_mut(p).is_some(),
        DocValue::Pairs(h) => Rc::get_mut(&mut h.backing).is_some_and(|c| {
            c.get_mut()
                .iter()
                .any(|p| is_container(&p[0]) || is_container(&p[1]))
        }),
        _ => false,
    }
}

impl Drop for DocValue {
    fn drop(&mut self) {
        if !has_container_children(self) {
            return;
        }
        let mut stack = DROP_STACK.take();
        take_children(self, &mut stack);
        while let Some(mut v) = stack.pop() {
            take_children(&mut v, &mut stack);
        }
        DROP_STACK.set(stack);
    }
}

thread_local! {
    static DROP_STACK: std::cell::Cell<Vec<DocValue>> = const { std::cell::Cell::new(Vec::new()) };
}

fn take_children(v: &mut DocValue, stack: &mut Vec<DocValue>) {
    match v {
        DocValue::Map(m) => {
            if let Some(cell) = Rc::get_mut(m) {
                let map = std::mem::take(cell.get_mut());
                stack.extend(map.into_values());
            }
        }
        DocValue::Slice(h) => {
            if let Some(cell) = Rc::get_mut(&mut h.backing) {
                stack.append(cell.get_mut());
            }
        }
        DocValue::SlicePtr(p) => {
            if let Some(cell) = Rc::get_mut(p) {
                let h = std::mem::take(&mut cell.get_mut().backing);
                stack.push(DocValue::Slice(TypedSlice { backing: h, len: 0 }));
            }
        }
        DocValue::Pairs(h) => {
            if let Some(cell) = Rc::get_mut(&mut h.backing) {
                for [a, b] in cell.get_mut().drain(..) {
                    stack.push(a);
                    stack.push(b);
                }
            }
        }
        _ => {}
    }
}

impl<E> Default for TypedSlice<E> {
    fn default() -> TypedSlice<E> {
        TypedSlice {
            backing: Rc::new(RefCell::new(Vec::new())),
            len: 0,
        }
    }
}

static NEXT_RUNTIME_ID: AtomicU64 = AtomicU64::new(1);

pub struct HostState {
    id: u64,
    objs: Vec<ObjRef>,
    obj_set: FxHashSet<ObjRef>,
    hosts: Vec<HostRef>,
    host_set: FxHashSet<usize>,
}

impl Default for HostState {
    fn default() -> HostState {
        HostState {
            id: NEXT_RUNTIME_ID.fetch_add(1, Ordering::Relaxed),
            objs: Vec::new(),
            obj_set: FxHashSet::default(),
            hosts: Vec::new(),
            host_set: FxHashSet::default(),
        }
    }
}

impl HostState {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn keep(&mut self, o: ObjRef) {
        if self.obj_set.insert(o) {
            self.objs.push(o);
        }
    }

    pub fn keep_host(&mut self, h: &HostRef) {
        let p = Rc::as_ptr(h) as *const () as usize;
        if self.host_set.insert(p) {
            self.hosts.push(h.clone());
        }
    }

    pub fn trace(&self, t: &mut Tracer) {
        for o in &self.objs {
            t.obj(*o);
        }
        for h in &self.hosts {
            h.trace(t);
        }
    }
}

pub fn key_str(name: &crate::unistring::String) -> std::borrow::Cow<'_, str> {
    if let Some(b) = name.ascii_bytes()
        && let Ok(s) = std::str::from_utf8(b)
    {
        return std::borrow::Cow::Borrowed(s);
    }
    std::borrow::Cow::Owned(name.string())
}

pub fn check_host_object_property_descr(
    rt: &mut Runtime,
    name: &dyn std::fmt::Display,
    descr: &PropertyDescriptor,
    throw: bool,
) -> JsResult<bool> {
    if descr.getter.is_some() || descr.setter.is_some() {
        rt.type_error_result(throw, "Host objects do not support accessor properties")?;
        return Ok(false);
    }
    if descr.writable == Flag::False {
        rt.type_error_result(
            throw,
            &format!("Host object field {name} cannot be made read-only"),
        )?;
        return Ok(false);
    }
    if descr.configurable == Flag::True {
        rt.type_error_result(
            throw,
            &format!("Host object field {name} cannot be made configurable"),
        )?;
        return Ok(false);
    }
    Ok(true)
}
