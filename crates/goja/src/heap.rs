use crate::object::{BaseObject, ObjClass, ObjExt};
use crate::runtime::Runtime;
use crate::string::JsString;
use crate::value::Value;
use crate::vm::Stash;
use std::collections::HashMap;
use std::hash::{BuildHasherDefault, Hasher};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ObjRef(pub u32);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct SymRef(pub u32);

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct StashRef(pub u32);

#[derive(Default, Clone, Copy)]
pub struct FxHasher {
    hash: u64,
}

const FX_SEED: u64 = 0x51_7c_c1_b7_27_22_0a_95;

impl FxHasher {
    #[inline]
    fn add(&mut self, w: u64) {
        self.hash = (self.hash.rotate_left(5) ^ w).wrapping_mul(FX_SEED);
    }
}

impl Hasher for FxHasher {
    #[inline]
    fn write(&mut self, bytes: &[u8]) {
        let mut chunks = bytes.chunks_exact(8);
        for c in &mut chunks {
            self.add(u64::from_le_bytes([
                c[0], c[1], c[2], c[3], c[4], c[5], c[6], c[7],
            ]));
        }
        let rest = chunks.remainder();
        if !rest.is_empty() {
            let mut w = 0u64;
            for (i, &b) in rest.iter().enumerate() {
                w |= u64::from(b) << (8 * i);
            }
            self.add(w);
        }
    }

    #[inline]
    fn write_u8(&mut self, i: u8) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u16(&mut self, i: u16) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u32(&mut self, i: u32) {
        self.add(u64::from(i));
    }

    #[inline]
    fn write_u64(&mut self, i: u64) {
        self.add(i);
    }

    #[inline]
    fn write_usize(&mut self, i: usize) {
        self.add(i as u64);
    }

    #[inline]
    fn finish(&self) -> u64 {
        self.hash
    }
}

pub type FxBuildHasher = BuildHasherDefault<FxHasher>;
pub type FxHashMap<K, V> = HashMap<K, V, FxBuildHasher>;
pub type FxHashSet<K> = std::collections::HashSet<K, FxBuildHasher>;

pub struct Object {
    pub class: &'static ObjClass,
    pub base: BaseObject,
    pub ext: ObjExt,
    pub weak_refs: Option<Box<FxHashMap<ObjRef, Value>>>,
}

#[derive(Clone, Debug)]
pub struct SymbolData {
    pub desc: Option<JsString>,
}

pub trait Trace {
    fn trace(&self, t: &mut Tracer);
}

pub struct Tracer {
    obj_marks: Vec<u64>,
    stash_marks: Vec<u64>,
    sym_marks: Vec<u64>,
    obj_stack: Vec<u32>,
    stash_stack: Vec<u32>,
}

fn mark_bit(marks: &mut [u64], i: u32) -> bool {
    let w = (i / 64) as usize;
    let b = 1u64 << (i % 64);
    if marks[w] & b != 0 {
        return false;
    }
    marks[w] |= b;
    true
}

fn is_marked(marks: &[u64], i: u32) -> bool {
    marks[(i / 64) as usize] & (1u64 << (i % 64)) != 0
}

impl Tracer {
    pub fn obj(&mut self, o: ObjRef) {
        if (o.0 as usize) < self.obj_marks.len() * 64 && mark_bit(&mut self.obj_marks, o.0) {
            self.obj_stack.push(o.0);
        }
    }

    pub fn opt_obj(&mut self, o: Option<ObjRef>) {
        if let Some(o) = o {
            self.obj(o);
        }
    }

    pub fn stash(&mut self, s: StashRef) {
        if (s.0 as usize) < self.stash_marks.len() * 64 && mark_bit(&mut self.stash_marks, s.0) {
            self.stash_stack.push(s.0);
        }
    }

    pub fn sym(&mut self, s: SymRef) {
        if s.0 >= crate::builtin_symbol::WELL_KNOWN_SYMBOLS_COUNT {
            let i = s.0 - crate::builtin_symbol::WELL_KNOWN_SYMBOLS_COUNT;
            if (i as usize) < self.sym_marks.len() * 64 {
                mark_bit(&mut self.sym_marks, i);
            }
        }
    }

    pub fn value(&mut self, v: &Value) {
        match v {
            Value::Object(o) => self.obj(*o),
            Value::Symbol(s) => self.sym(*s),
            Value::Property(p) => {
                self.value(&p.value);
                self.opt_obj(p.getter_func);
                self.opt_obj(p.setter_func);
            }
            _ => {}
        }
    }

    pub fn values(&mut self, vs: &[Value]) {
        for v in vs {
            self.value(v);
        }
    }
}

pub struct Heap {
    objects: Vec<Option<Object>>,
    free: Vec<u32>,
    stashes: Vec<Option<Stash>>,
    stash_free: Vec<u32>,
    symbols: Vec<Option<SymbolData>>,
    sym_free: Vec<u32>,
    allocs_since_gc: usize,
    threshold: usize,
    pub gc_requested: bool,
    pub stress: bool,
}

const MIN_GC_THRESHOLD: usize = 4096;

impl Default for Heap {
    fn default() -> Heap {
        Heap::new()
    }
}

impl Heap {
    pub fn new() -> Heap {
        Heap {
            objects: Vec::new(),
            free: Vec::new(),
            stashes: Vec::new(),
            stash_free: Vec::new(),
            symbols: Vec::new(),
            sym_free: Vec::new(),
            allocs_since_gc: 0,
            threshold: MIN_GC_THRESHOLD,
            gc_requested: false,
            stress: false,
        }
    }

    #[inline]
    fn count_alloc(&mut self) {
        self.allocs_since_gc += 1;
        if self.allocs_since_gc > self.threshold || self.stress {
            self.gc_requested = true;
        }
    }

    pub fn alloc(&mut self, o: Object) -> ObjRef {
        self.count_alloc();
        if !self.stress
            && let Some(i) = self.free.pop()
        {
            self.objects[i as usize] = Some(o);
            return ObjRef(i);
        }
        self.objects.push(Some(o));
        ObjRef((self.objects.len() - 1) as u32)
    }

    pub fn alloc_stash(&mut self, s: Stash) -> StashRef {
        self.count_alloc();
        if !self.stress
            && let Some(i) = self.stash_free.pop()
        {
            self.stashes[i as usize] = Some(s);
            return StashRef(i);
        }
        self.stashes.push(Some(s));
        StashRef((self.stashes.len() - 1) as u32)
    }

    pub fn alloc_symbol(&mut self, s: SymbolData) -> SymRef {
        let base = crate::builtin_symbol::WELL_KNOWN_SYMBOLS_COUNT;
        if !self.stress
            && let Some(i) = self.sym_free.pop()
        {
            self.symbols[i as usize] = Some(s);
            return SymRef(i + base);
        }
        self.symbols.push(Some(s));
        SymRef((self.symbols.len() - 1) as u32 + base)
    }

    #[inline]
    pub fn get(&self, o: ObjRef) -> &Object {
        match self.objects.get(o.0 as usize) {
            Some(Some(obj)) => obj,
            _ => panic!("stale object reference {}", o.0),
        }
    }

    #[inline]
    pub fn get_mut(&mut self, o: ObjRef) -> &mut Object {
        match self.objects.get_mut(o.0 as usize) {
            Some(Some(obj)) => obj,
            _ => panic!("stale object reference {}", o.0),
        }
    }

    pub fn is_live(&self, o: ObjRef) -> bool {
        matches!(self.objects.get(o.0 as usize), Some(Some(_)))
    }

    #[inline]
    pub fn stash(&self, s: StashRef) -> &Stash {
        match self.stashes.get(s.0 as usize) {
            Some(Some(st)) => st,
            _ => panic!("stale stash reference {}", s.0),
        }
    }

    #[inline]
    pub fn stash_mut(&mut self, s: StashRef) -> &mut Stash {
        match self.stashes.get_mut(s.0 as usize) {
            Some(Some(st)) => st,
            _ => panic!("stale stash reference {}", s.0),
        }
    }

    pub fn symbol(&self, s: SymRef) -> Option<&SymbolData> {
        let base = crate::builtin_symbol::WELL_KNOWN_SYMBOLS_COUNT;
        if s.0 < base {
            return None;
        }
        match self.symbols.get((s.0 - base) as usize) {
            Some(Some(sym)) => Some(sym),
            _ => panic!("stale symbol reference {}", s.0),
        }
    }

    pub fn live_objects(&self) -> usize {
        self.objects.len() - self.free.len()
    }

    fn new_tracer(&self) -> Tracer {
        Tracer {
            obj_marks: vec![0; self.objects.len().div_ceil(64)],
            stash_marks: vec![0; self.stashes.len().div_ceil(64)],
            sym_marks: vec![0; self.symbols.len().div_ceil(64)],
            obj_stack: Vec::new(),
            stash_stack: Vec::new(),
        }
    }

    fn propagate(&self, t: &mut Tracer) {
        loop {
            if let Some(i) = t.obj_stack.pop() {
                if let Some(Some(o)) = self.objects.get(i as usize) {
                    o.trace(t);
                }
                continue;
            }
            if let Some(i) = t.stash_stack.pop() {
                if let Some(Some(s)) = self.stashes.get(i as usize) {
                    s.trace(t);
                }
                continue;
            }
            break;
        }
    }

    fn sweep(&mut self, t: &Tracer) {
        for (i, slot) in self.objects.iter_mut().enumerate() {
            if slot.is_some() && !is_marked(&t.obj_marks, i as u32) {
                *slot = None;
                if !self.stress {
                    self.free.push(i as u32);
                }
            }
        }
        for (i, slot) in self.stashes.iter_mut().enumerate() {
            if slot.is_some() && !is_marked(&t.stash_marks, i as u32) {
                *slot = None;
                if !self.stress {
                    self.stash_free.push(i as u32);
                }
            }
        }
        for (i, slot) in self.symbols.iter_mut().enumerate() {
            if slot.is_some() && !is_marked(&t.sym_marks, i as u32) {
                *slot = None;
                if !self.stress {
                    self.sym_free.push(i as u32);
                }
            }
        }
        let live = self.live_objects();
        self.threshold = live.max(MIN_GC_THRESHOLD);
        self.allocs_since_gc = 0;
        self.gc_requested = false;
    }
}

impl Object {
    fn trace(&self, t: &mut Tracer) {
        self.base.trace(t);
        self.ext.trace(t);
        if let Some(w) = &self.weak_refs {
            for v in w.values() {
                t.value(v);
            }
        }
    }
}

impl Runtime {
    pub fn collect_garbage(&mut self) {
        let mut t = self.heap.new_tracer();
        self.trace_roots(&mut t);
        self.heap.propagate(&mut t);
        self.heap.sweep(&t);
    }

    pub fn set_gc_stress(&mut self, stress: bool) {
        self.heap.stress = stress;
    }

    #[inline]
    pub fn root(&mut self, v: &Value) {
        if self.in_native
            && let Value::Object(o) = v
        {
            self.handles.push(*o);
        }
    }

    #[inline]
    pub fn root_obj(&mut self, o: ObjRef) {
        if self.in_native {
            self.handles.push(o);
        }
    }

    pub fn handle_scope_enter(&self) -> usize {
        self.handles.len()
    }

    pub fn handle_scope_leave(&mut self, mark: usize) {
        self.handles.truncate(mark);
    }

    pub fn handle_scope_keep(&mut self, mark: usize, v: &Value) {
        self.handles.truncate(mark);
        if let Value::Object(o) = v {
            self.handles.push(*o);
        }
    }
}
