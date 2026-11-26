use std::any::Any;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, OnceLock, Weak};
use std::time::{Duration, Instant};

use arc_swap::{ArcSwap, ArcSwapOption};
use talos::Runtime;
use talos::host::{GoMethod, GoMethods};
use talos::object::FunctionCall;
use talos::runtime::JsResult;
use talos::value::{Value, float_to_value};

use crate::acl::{self, ReadMode};
use crate::api::{self, Guard, GuardError};
use crate::datastore::{Engine, Snapshot, StopToken, decode_map};
use crate::docpath;
use crate::engine;
use crate::gocompat::json::{self, Map};
use crate::gocompat::strings;
use crate::memory;
use crate::query::Filters;
use crate::storage;
use crate::sync::{Condvar, Mutex, RwLock, Ticker};

use super::executor::{Request, filters_key, observe_fold};
use super::foldeval::{
    Ctl, Frame, NULL_VAL, UNDEF_VAL, VKind, Val, acquire_frame, bool_val, is_string, num_val,
    release_frame,
};
use super::foldplan::{FoldDef, plan_for};
use super::hook::{HOOK_EVENT_PRE_READ, Hook};
use super::registry::{RegisteredHook, same_engine};
use super::results::{RESULTS, unix_now};

const VIEW_SHARDS: usize = 256;
const VIEW_CHUNK_ROWS: usize = 1024;
const VIEW_RECENT_DELTAS: usize = 1024;
const FOLD_CACHE_MIN_ROWS: usize = 256;
const FOLD_LANE_MIN_ROWS: usize = 1 << 14;
const MAX_FOLD_ENTRIES: usize = 1 << 16;
const MAX_VIEWS: usize = 64;
const VIEW_IDLE_AFTER: i64 = 10 * 60;
const VIEW_REFUSED_FOR: i64 = 30;
const VIEW_REFRESH_EVERY: Duration = Duration::from_millis(200);
const VIEW_PART_OVERHEAD: i64 = 160;
const VIEW_CHUNK_OVERHEAD: i64 = 80;
const VAL_BYTES: i64 = 16;
const OWNER_NONE: u32 = 0;
const OWNER_ALL: u32 = 1;
const OWNER_IRREGULAR: u32 = 2;
const OWNER_FIRST_PART: u32 = 3;

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum ViewErr {
    Refused,
    NotReady,
    Guard(GuardError),
    Other(String),
}

static FOLDS_DISABLED: AtomicBool = AtomicBool::new(false);

pub fn set_folds_enabled(enabled: bool) {
    FOLDS_DISABLED.store(!enabled, Ordering::SeqCst);
}

const GO_CLASS_TO_SIZE: [usize; 68] = [
    0, 8, 16, 24, 32, 48, 64, 80, 96, 112, 128, 144, 160, 176, 192, 208, 224, 240, 256, 288, 320,
    352, 384, 416, 448, 480, 512, 576, 640, 704, 768, 896, 1024, 1152, 1280, 1408, 1536, 1792,
    2048, 2304, 2688, 3072, 3200, 3456, 4096, 4864, 5376, 6144, 6528, 6784, 6912, 8192, 9472, 9728,
    10240, 10880, 12288, 13568, 14336, 16384, 18432, 19072, 20480, 21760, 24576, 27264, 28672,
    32768,
];

fn roundupsize_noscan(size: usize) -> usize {
    const MAX_SMALL_SIZE: usize = 32768;
    const MALLOC_HEADER_SIZE: usize = 8;
    const PAGE_SIZE: usize = 8192;
    if size <= MAX_SMALL_SIZE - MALLOC_HEADER_SIZE {
        return GO_CLASS_TO_SIZE
            .iter()
            .copied()
            .find(|&c| c >= size)
            .unwrap_or(MAX_SMALL_SIZE);
    }
    let req = size + PAGE_SIZE - 1;
    req & !(PAGE_SIZE - 1)
}

fn next_slice_cap(new_len: usize, old_cap: usize) -> usize {
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

fn grow_cap(old_cap: usize, new_len: usize, elem: usize) -> usize {
    let newcap = next_slice_cap(new_len, old_cap);
    roundupsize_noscan(newcap * elem) / elem
}

#[derive(Clone, Debug, Default)]
struct GoVec<T> {
    v: Vec<T>,
    cap: usize,
}

impl<T: Clone> GoVec<T> {
    fn with_cap(cap: usize) -> GoVec<T> {
        GoVec {
            v: Vec::with_capacity(cap),
            cap,
        }
    }

    fn len(&self) -> usize {
        self.v.len()
    }

    fn reserve_go(&mut self, add: usize, elem: usize) {
        let need = self.v.len() + add;
        if need > self.cap {
            self.cap = grow_cap(self.cap, need, elem);
        }
    }

    fn push_go(&mut self, x: T, elem: usize) {
        self.reserve_go(1, elem);
        self.v.push(x);
    }

    fn extend_go(&mut self, xs: &[T], elem: usize) {
        self.reserve_go(xs.len(), elem);
        self.v.extend_from_slice(xs);
    }
}

struct StrBuf {
    slots: Box<[OnceLock<Arc<str>>]>,
}

pub(crate) struct Strs {
    buf: Arc<StrBuf>,
    len: usize,
}

impl Strs {
    pub(crate) fn get(&self, i: usize) -> &str {
        assert!(
            i < self.len,
            "index out of range [{i}] with length {}",
            self.len
        );
        self.buf.slots[i].get().expect("interned string")
    }
}

struct TableInner {
    ids: HashMap<Arc<str>, u32>,
    bytes: i64,
}

pub(crate) struct InternTable {
    inner: RwLock<TableInner>,
    strs: ArcSwap<Strs>,
}

impl InternTable {
    fn new() -> InternTable {
        let buf = StrBuf {
            slots: (0..16).map(|_| OnceLock::new()).collect(),
        };
        let empty: Arc<str> = Arc::from("");
        let _ = buf.slots[0].set(empty.clone());
        InternTable {
            inner: RwLock::new(TableInner {
                ids: HashMap::from([(empty, 0)]),
                bytes: 0,
            }),
            strs: ArcSwap::from_pointee(Strs {
                buf: Arc::new(buf),
                len: 1,
            }),
        }
    }

    fn lookup(&self, s: &str) -> Option<u32> {
        self.inner.read().ids.get(s).copied()
    }

    fn intern(&self, s: &str) -> u32 {
        let mut inner = self.inner.write();
        if let Some(id) = inner.ids.get(s) {
            return *id;
        }

        let cur = self.strs.load_full();
        let v: Arc<str> = Arc::from(s);
        let next = if cur.len < cur.buf.slots.len() {
            let _ = cur.buf.slots[cur.len].set(v.clone());
            Strs {
                buf: cur.buf.clone(),
                len: cur.len + 1,
            }
        } else {
            let cap = (cur.len * 2).max(16);
            let buf = StrBuf {
                slots: (0..cap).map(|_| OnceLock::new()).collect(),
            };
            for i in 0..cur.len {
                let _ = buf.slots[i].set(cur.buf.slots[i].get().expect("interned").clone());
            }
            let _ = buf.slots[cur.len].set(v.clone());
            Strs {
                buf: Arc::new(buf),
                len: cur.len + 1,
            }
        };
        let id = cur.len as u32;
        inner.ids.insert(v, id);
        self.strs.store(Arc::new(next));
        inner.bytes += s.len() as i64 + 64;

        id
    }

    fn count(&self) -> usize {
        self.strs.load().len
    }

    fn bytes(&self) -> i64 {
        self.inner.read().bytes
    }

    pub(crate) fn snapshot(&self) -> Arc<Strs> {
        self.strs.load_full()
    }
}

#[derive(Clone, Debug)]
struct ViewChunk {
    seqs: GoVec<u32>,
    cells: GoVec<Val>,
}

impl ViewChunk {
    fn cost(&self) -> i64 {
        self.seqs.cap as i64 * 4 + self.cells.cap as i64 * VAL_BYTES + VIEW_CHUNK_OVERHEAD
    }
}

#[derive(Clone, Debug)]
struct ViewDelta {
    lsn: u64,
    seq: u32,
    old: Arc<[Val]>,
    new: Arc<[Val]>,
    had_old: bool,
    has_new: bool,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct ViewPart {
    chunks: Vec<Arc<ViewChunk>>,
    pub(crate) rows: usize,
    version: u64,
    covered: u64,
    last: u32,
    recent: Vec<ViewDelta>,
    recent_cap: usize,
}

struct FoldEntry {
    ver: u64,
    last: u32,
    regs: Vec<Val>,
}

#[derive(Default)]
struct FoldItems {
    items: HashMap<Vec<u8>, Arc<FoldEntry>>,
    bytes: i64,
}

pub(crate) struct FoldCache {
    st: Mutex<FoldItems>,
}

fn fold_entry_cost(key: &[u8], regs: &[Val]) -> i64 {
    key.len() as i64 + regs.len() as i64 * VAL_BYTES + 128
}

impl FoldCache {
    fn new() -> FoldCache {
        FoldCache {
            st: Mutex::new(FoldItems::default()),
        }
    }

    fn get(&self, key: &[u8]) -> Option<Arc<FoldEntry>> {
        self.st.lock().items.get(key).cloned()
    }

    fn put(&self, key: Vec<u8>, e: FoldEntry) -> i64 {
        let mut f = self.st.lock();

        let mut delta: i64 = 0;
        if let Some(old) = f.items.get(&key) {
            delta -= fold_entry_cost(&key, &old.regs);
        }

        if f.items.len() >= MAX_FOLD_ENTRIES {
            let dropped: Vec<Vec<u8>> =
                f.items.keys().take(MAX_FOLD_ENTRIES / 8).cloned().collect();
            for k in dropped {
                if let Some(old) = f.items.remove(&k) {
                    delta -= fold_entry_cost(&k, &old.regs);
                }
            }
        }

        delta += fold_entry_cost(&key, &e.regs);
        f.items.insert(key, Arc::new(e));
        f.bytes += delta;

        delta
    }
}

type Shard = HashMap<String, Arc<ViewPart>>;

pub(crate) struct ViewState {
    generation: u64,
    lsn: u64,
    shards: Vec<Option<Arc<Shard>>>,
    table: Arc<InternTable>,
    folds: Arc<FoldCache>,
    pub(crate) irregular: i64,
    rows: i64,
}

fn shard_of(key: &str) -> usize {
    if key.is_empty() {
        return 0;
    }

    let mut h: u32 = 0x811c9dc5;
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x01000193);
    }

    (h % VIEW_SHARDS as u32) as usize
}

impl ViewState {
    pub(crate) fn part(&self, key: &str) -> Option<&Arc<ViewPart>> {
        self.shards[shard_of(key)].as_ref()?.get(key)
    }
}

#[derive(Default)]
struct ViewMut {
    owner: GoVec<u32>,
    counted: i64,
}

#[derive(Default)]
struct ViewFlight {
    flight: Option<Arc<Signal>>,
    err: Option<ViewErr>,
}

struct Signal {
    done: Mutex<bool>,
    cv: Condvar,
}

impl Signal {
    fn new() -> Signal {
        Signal {
            done: Mutex::new(false),
            cv: Condvar::new(),
        }
    }

    fn fire(&self) {
        *self.done.lock() = true;
        self.cv.notify_all();
    }

    fn wait(&self, d: Duration) -> bool {
        let mut g = self.done.lock();
        if *g {
            return true;
        }
        let (g2, _) = self.cv.wait_timeout(g, d);
        g = g2;
        *g
    }
}

pub(crate) struct View {
    key: Vec<u8>,
    entity: String,
    base: Filters,
    part_field: String,
    fields: Vec<String>,
    pinned: AtomicBool,
    used: AtomicI64,
    refused: AtomicI64,
    state: ArcSwapOption<ViewState>,
    bytes: AtomicI64,
    mu: Mutex<ViewMut>,
    flight: Mutex<ViewFlight>,
}

#[derive(Default)]
struct ViewSetState {
    engine: Weak<Engine>,
    items: BTreeMap<Vec<u8>, Arc<View>>,
}

pub(crate) struct ViewSet {
    st: Mutex<Option<ViewSetState>>,
}

pub(crate) static VIEWS: ViewSet = ViewSet {
    st: Mutex::new(None),
};

fn view_key(entity: &str, base: &Filters, part_field: &str, fields: &[String]) -> Vec<u8> {
    let mut k = Vec::new();
    k.extend_from_slice(entity.as_bytes());
    k.push(0);
    k.extend_from_slice(&filters_key(base));
    k.push(0);
    k.extend_from_slice(part_field.as_bytes());
    k.push(0);
    for (i, f) in fields.iter().enumerate() {
        if i > 0 {
            k.push(0);
        }
        k.extend_from_slice(f.as_bytes());
    }
    k
}

impl ViewSet {
    fn get(
        &self,
        e: &Arc<Engine>,
        entity: &str,
        base: &Filters,
        part_field: &str,
        fields: &[String],
    ) -> Option<Arc<View>> {
        let key = view_key(entity, base, part_field, fields);

        let mut g = self.st.lock();
        let s = g.get_or_insert_with(ViewSetState::default);

        if !same_engine(&s.engine, e) {
            for v in s.items.values() {
                v.release();
            }
            s.items = BTreeMap::new();
            s.engine = Arc::downgrade(e);
        }

        if let Some(v) = s.items.get(&key) {
            return Some(v.clone());
        }

        if s.items.len() >= MAX_VIEWS && !evict_idle(s) {
            return None;
        }

        let v = Arc::new(View {
            key: key.clone(),
            entity: entity.to_string(),
            base: base.clone(),
            part_field: part_field.to_string(),
            fields: fields.to_vec(),
            pinned: AtomicBool::new(false),
            used: AtomicI64::new(0),
            refused: AtomicI64::new(0),
            state: ArcSwapOption::empty(),
            bytes: AtomicI64::new(0),
            mu: Mutex::new(ViewMut::default()),
            flight: Mutex::new(ViewFlight::default()),
        });
        s.items.insert(key, v.clone());

        Some(v)
    }

    pub(crate) fn reset(&self) {
        let g = self.st.lock();
        if let Some(s) = g.as_ref() {
            for v in s.items.values() {
                v.release();
            }
        }
    }

    fn list(&self) -> Vec<Arc<View>> {
        let g = self.st.lock();
        match g.as_ref() {
            Some(s) => s.items.values().cloned().collect(),
            None => Vec::new(),
        }
    }

    pub(crate) fn sync(&self, e: &Arc<Engine>, list: &[RegisteredHook]) {
        let mut want: HashSet<Vec<u8>> = HashSet::new();
        for h in list {
            if !h.hook.enabled {
                continue;
            }

            let Some(plan) = plan_for(&h.hook.script, script_function(&h.hook.event)) else {
                continue;
            };

            for def in &plan.folds {
                if !def.static_base || engine::is_core_entity_type(def.target.as_bytes()) {
                    continue;
                }

                if let Some(v) = self.get(
                    e,
                    &def.target,
                    &def.base_filters,
                    &def.part_field,
                    &def.fields,
                ) {
                    v.pinned.store(true, Ordering::SeqCst);
                    want.insert(v.key.clone());
                }
            }
        }

        for v in self.list() {
            if v.pinned.load(Ordering::SeqCst) && !want.contains(&v.key) {
                v.pinned.store(false, Ordering::SeqCst);
            }
        }
    }

    pub(crate) fn maintain(&self, e: &Arc<Engine>, stop: &StopToken) {
        let mut ticker = Ticker::new(VIEW_REFRESH_EVERY);
        loop {
            loop {
                if ticker.take(Instant::now()).is_some() {
                    break;
                }
                let wait = ticker.deadline().saturating_duration_since(Instant::now());
                if stop.wait_timeout(wait) {
                    return;
                }
            }
            if stop.is_stopped() {
                return;
            }

            let cutoff = unix_now() - VIEW_IDLE_AFTER;
            for v in self.list() {
                if !v.pinned.load(Ordering::SeqCst) && v.used.load(Ordering::SeqCst) < cutoff {
                    v.release();
                    continue;
                }

                if !v.pinned.load(Ordering::SeqCst) && v.state.load().is_none() {
                    continue;
                }

                let _ = v.try_refresh(e);
            }

            RESULTS.sweep(e);
        }
    }
}

fn evict_idle(s: &mut ViewSetState) -> bool {
    let mut oldest: Option<&Arc<View>> = None;
    for v in s.items.values() {
        if v.pinned.load(Ordering::SeqCst) {
            continue;
        }
        if oldest.is_none_or(|o| v.used.load(Ordering::SeqCst) < o.used.load(Ordering::SeqCst)) {
            oldest = Some(v);
        }
    }

    let Some(oldest) = oldest.cloned() else {
        return false;
    };

    oldest.release();
    s.items.remove(&oldest.key);

    true
}

fn script_function(event: &str) -> &'static str {
    if event == HOOK_EVENT_PRE_READ {
        return "preRead";
    }

    "postRead"
}

fn fresh(st: Option<&Arc<ViewState>>, snap: &Snapshot) -> bool {
    st.is_some_and(|st| st.generation == snap.generation() && st.lsn >= snap.version())
}

impl View {
    fn release(&self) {
        let mut m = self.mu.lock();
        self.state.store(None);
        m.owner = GoVec::default();
        m.counted = 0;
        memory::HOOK_RESULTS.release(self.bytes.swap(0, Ordering::SeqCst));
    }

    fn try_refresh(&self, e: &Arc<Engine>) -> Result<(), ViewErr> {
        let r = catch_unwind(AssertUnwindSafe(|| -> Result<(), ViewErr> {
            if unix_now() < self.refused.load(Ordering::SeqCst) {
                return Err(ViewErr::Refused);
            }

            let snap = e.snapshot(self.entity.as_bytes());
            if fresh(self.state.load_full().as_ref(), &snap) {
                return Ok(());
            }

            self.advance(e)
        }));

        match r {
            Ok(r) => r,
            Err(_) => Err(ViewErr::NotReady),
        }
    }

    fn current(
        self: &Arc<Self>,
        e: &Arc<Engine>,
        guard: Option<&Guard>,
    ) -> Result<(Arc<ViewState>, bool), (ViewErr, bool)> {
        if unix_now() < self.refused.load(Ordering::SeqCst) {
            return Err((ViewErr::Refused, false));
        }

        let snap = e.snapshot(self.entity.as_bytes());
        let st = self.state.load_full();
        if fresh(st.as_ref(), &snap) {
            return Ok((st.expect("fresh state"), false));
        }

        let cold = st.is_none();
        let timeout = api::queue_timeout();
        let start = Instant::now();
        loop {
            let f = {
                let mut fl = self.flight.lock();
                match &fl.flight {
                    Some(f) => f.clone(),
                    None => {
                        let f = Arc::new(Signal::new());
                        fl.flight = Some(f.clone());
                        let v = self.clone();
                        let eng = e.clone();
                        let done = f.clone();
                        drop(crate::sync::spawn("hook-view-refresh", move || {
                            let err = v.try_refresh(&eng).err();
                            {
                                let mut fl = v.flight.lock();
                                fl.err = err;
                                fl.flight = None;
                            }
                            done.fire();
                        }));
                        f
                    }
                }
            };

            loop {
                if f.wait(Duration::from_millis(20)) {
                    break;
                }
                if let Err(err) = api::guard::check(guard) {
                    return Err((ViewErr::Guard(err), cold));
                }
                if timeout.0 >= 0 && start.elapsed().as_nanos() as i64 >= timeout.0 {
                    return Err((ViewErr::Guard(api::guard::reject(guard)), cold));
                }
            }

            let err = self.flight.lock().err.clone();
            if let Some(err) = err {
                return Err((err, cold));
            }

            let st = self.state.load_full();
            if fresh(st.as_ref(), &snap) {
                return Ok((st.expect("fresh state"), cold));
            }
        }
    }

    fn advance(&self, e: &Arc<Engine>) -> Result<(), ViewErr> {
        let mut m = self.mu.lock();

        let r = catch_unwind(AssertUnwindSafe(|| self.advance_locked(e, &mut m)));
        match r {
            Ok(Ok(())) => Ok(()),
            Ok(Err(err)) => {
                self.state.store(None);
                Err(err)
            }
            Err(p) => {
                self.state.store(None);
                std::panic::resume_unwind(p)
            }
        }
    }

    fn advance_locked(&self, e: &Arc<Engine>, m: &mut ViewMut) -> Result<(), ViewErr> {
        let snap = e.snapshot(self.entity.as_bytes());
        let st = self.state.load_full();
        if fresh(st.as_ref(), &snap) {
            return Ok(());
        }

        let Some(st) = st else {
            return self.rebuild(&snap, m);
        };
        if st.generation != snap.generation() || st.table.count() as i64 > 4 * st.rows + 4096 {
            return self.rebuild(&snap, m);
        }

        let Some(mut seqs) = snap.changed_since(st.lsn) else {
            return self.rebuild(&snap, m);
        };

        seqs.sort_unstable();

        let mut b = Batch::new(self, m, &st, snap.version(), snap.generation());
        for seq in seqs {
            let Some(d) = snap.by_seq(seq) else {
                b.apply(seq, None);
                continue;
            };

            let raw = d.bytes().map_err(|err| ViewErr::Other(err.to_string()))?;

            match decode_map(&raw) {
                Ok(Some(doc)) => b.apply(seq, Some(&doc)),
                _ => b.apply(seq, None),
            }
        }

        let next = b.finish();
        self.publish(next, m)
    }

    fn rebuild(&self, snap: &Snapshot, m: &mut ViewMut) -> Result<(), ViewErr> {
        m.owner = GoVec::with_cap(snap.next_seq() as usize);
        m.counted = 0;

        let empty = ViewState {
            generation: 0,
            lsn: 0,
            shards: vec![None; VIEW_SHARDS],
            table: Arc::new(InternTable::new()),
            folds: Arc::new(FoldCache::new()),
            irregular: 0,
            rows: 0,
        };
        let mut b = Batch::new(self, m, &empty, snap.version(), snap.generation());
        b.rebuilding = true;

        let mut failed: Option<String> = None;
        snap.scan_values(|d, raw| {
            let raw = match raw {
                Ok(raw) => raw,
                Err(err) => {
                    failed = Some(err.to_string());
                    return false;
                }
            };

            match decode_map(raw) {
                Ok(Some(doc)) => {
                    b.apply(d.seq(), Some(&doc));
                    true
                }
                _ => true,
            }
        });

        if let Some(err) = failed {
            return Err(ViewErr::Other(err));
        }

        let next = b.finish();
        self.publish(next, m)
    }

    fn publish(&self, mut b: Finished, m: &mut ViewMut) -> Result<(), ViewErr> {
        let table_bytes = b.next.table.bytes();
        b.delta += table_bytes - m.counted + (m.owner.cap as i64 - b.owner_cap as i64) * 4;
        m.counted = table_bytes;

        let limit = memory::HOOK_RESULTS.effective();
        if memory::budget() > 0 && limit > 0 {
            let mut held = b.delta;
            if !b.rebuilding {
                held += self.bytes.load(Ordering::SeqCst);
            }
            if held > limit / 2 {
                self.state.store(None);
                m.owner = GoVec::default();
                m.counted = 0;
                memory::HOOK_RESULTS.release(self.bytes.swap(0, Ordering::SeqCst));
                self.refused
                    .store(unix_now() + VIEW_REFUSED_FOR, Ordering::SeqCst);
                memory::HOOK_RESULTS.refuse();
                return Err(ViewErr::Refused);
            }
        }

        if b.rebuilding {
            memory::HOOK_RESULTS.release(self.bytes.swap(0, Ordering::SeqCst));
        }

        self.bytes.fetch_add(b.delta, Ordering::SeqCst);
        memory::HOOK_RESULTS.add(b.delta);
        self.state.store(Some(Arc::new(b.next)));

        Ok(())
    }

    fn str_val(table: &InternTable, fr: &mut Frame, s: &str) -> Val {
        if let Some(id) = table.lookup(s) {
            return Val {
                k: VKind::Str,
                id,
                n: 0.0,
            };
        }

        fr.tmp.push(Arc::from(s));

        Val {
            k: VKind::Tmp,
            id: (fr.tmp.len() - 1) as u32,
            n: 0.0,
        }
    }

    fn to_val(
        rt: &mut Runtime,
        table: &InternTable,
        fr: &mut Frame,
        x: &Value,
    ) -> JsResult<Option<Val>> {
        if x.is_undefined() {
            return Ok(Some(UNDEF_VAL));
        }

        if x.is_null() {
            return Ok(Some(NULL_VAL));
        }

        Ok(match &rt.export(x)? {
            talos::host::DocValue::Bool(b) => Some(bool_val(*b)),
            talos::host::DocValue::Int64(i) => Some(num_val(*i as f64)),
            talos::host::DocValue::Float64(f) => Some(num_val(*f)),
            talos::host::DocValue::String(s) => Some(View::str_val(table, fr, s)),
            _ => None,
        })
    }

    fn add_cost(&self, delta: i64) {
        self.bytes.fetch_add(delta, Ordering::SeqCst);
        memory::HOOK_RESULTS.add(delta);
    }
}

struct Finished {
    next: ViewState,
    delta: i64,
    owner_cap: usize,
    rebuilding: bool,
}

struct Batch<'a> {
    v: &'a View,
    m: &'a mut ViewMut,
    next: ViewState,
    lsn: u64,
    width: usize,
    owned_shard: [bool; VIEW_SHARDS],
    owned_part: HashSet<usize>,
    owned_chunk: HashSet<usize>,
    delta: i64,
    owner_cap: usize,
    rebuilding: bool,
}

fn part_ptr(p: &Arc<ViewPart>) -> usize {
    Arc::as_ptr(p) as usize
}

fn chunk_ptr(c: &Arc<ViewChunk>) -> usize {
    Arc::as_ptr(c) as usize
}

impl<'a> Batch<'a> {
    fn new(
        v: &'a View,
        m: &'a mut ViewMut,
        st: &ViewState,
        lsn: u64,
        generation: u64,
    ) -> Batch<'a> {
        let next = ViewState {
            generation,
            lsn,
            shards: st.shards.clone(),
            table: st.table.clone(),
            folds: st.folds.clone(),
            irregular: st.irregular,
            rows: st.rows,
        };
        let owner_cap = m.owner.cap;

        Batch {
            v,
            m,
            next,
            lsn,
            width: v.fields.len(),
            owned_shard: [false; VIEW_SHARDS],
            owned_part: HashSet::new(),
            owned_chunk: HashSet::new(),
            delta: 0,
            owner_cap,
            rebuilding: false,
        }
    }

    fn finish(self) -> Finished {
        Finished {
            next: self.next,
            delta: self.delta,
            owner_cap: self.owner_cap,
            rebuilding: self.rebuilding,
        }
    }

    fn owner_of(&self, seq: u32) -> u32 {
        if (seq as usize) < self.m.owner.len() {
            return self.m.owner.v[seq as usize];
        }

        OWNER_NONE
    }

    fn set_owner(&mut self, seq: u32, owner: u32) {
        let seq = seq as usize;
        if seq >= self.m.owner.len() {
            if owner == OWNER_NONE {
                return;
            }
            if seq < self.m.owner.cap {
                self.m.owner.v.resize(seq + 1, OWNER_NONE);
            } else {
                let cap = (seq + 1).max(2 * self.m.owner.cap);
                let mut grown = Vec::with_capacity(cap);
                grown.extend_from_slice(&self.m.owner.v);
                grown.resize(seq + 1, OWNER_NONE);
                self.m.owner = GoVec { v: grown, cap };
            }
        }

        self.m.owner.v[seq] = owner;
    }

    fn part_key(&self, owner: u32) -> String {
        if owner == OWNER_ALL {
            return String::new();
        }

        self.next
            .table
            .snapshot()
            .get((owner - OWNER_FIRST_PART) as usize)
            .to_string()
    }

    fn cells_of(&self, doc: &Map) -> Vec<Val> {
        let mut cells = vec![UNDEF_VAL; self.width];
        for (i, f) in self.v.fields.iter().enumerate() {
            let Some(raw) = doc.get(f) else {
                continue;
            };

            cells[i] = match raw {
                json::Value::Null => NULL_VAL,
                json::Value::String(s) => Val {
                    k: VKind::Str,
                    id: self.next.table.intern(s),
                    n: 0.0,
                },
                json::Value::Number(f) => num_val(*f),
                json::Value::Int(i) => num_val(*i as f64),
                json::Value::Bool(b) => bool_val(*b),
                _ => Val {
                    k: VKind::Opaque,
                    id: 0,
                    n: 0.0,
                },
            };
        }

        cells
    }

    fn classify(&self, doc: &Map) -> (u32, bool) {
        if !api::filters_match_entity(doc, &self.v.base) {
            return (OWNER_NONE, false);
        }

        if self.v.part_field.is_empty() {
            return (OWNER_ALL, true);
        }

        let raw = match doc.get(&self.v.part_field) {
            None | Some(json::Value::Null) => return (OWNER_NONE, false),
            Some(raw) => raw,
        };

        let json::Value::String(s) = raw else {
            return (OWNER_IRREGULAR, false);
        };

        let lower = strings::to_lower(s.as_bytes());
        let lower = String::from_utf8_lossy(&lower);

        (self.next.table.intern(&lower) + OWNER_FIRST_PART, true)
    }

    fn apply(&mut self, seq: u32, doc: Option<&Map>) {
        let before = self.owner_of(seq);

        let mut after = OWNER_NONE;
        let mut cells: Vec<Val> = Vec::new();
        if let Some(doc) = doc {
            let (owner, in_view) = self.classify(doc);
            after = owner;
            if in_view {
                cells = self.cells_of(doc);
            }
        }
        let cells: Arc<[Val]> = cells.into();

        if before == OWNER_IRREGULAR && after != OWNER_IRREGULAR {
            self.next.irregular -= 1;
        }
        if after == OWNER_IRREGULAR && before != OWNER_IRREGULAR {
            self.next.irregular += 1;
        }

        if before >= OWNER_ALL && before != OWNER_IRREGULAR {
            let key = self.part_key(before);
            if before == after {
                let old = self.with_part(&key, |b, p| b.update(p, seq, &cells));
                self.with_part(&key, |b, p| {
                    b.record(
                        p,
                        ViewDelta {
                            lsn: 0,
                            seq,
                            old: old.clone(),
                            new: cells.clone(),
                            had_old: true,
                            has_new: true,
                        },
                    )
                });
                self.set_owner(seq, after);
                return;
            }

            let old = self.with_part(&key, |b, p| b.remove(p, seq));
            let empty = self.with_part(&key, |b, p| {
                b.record(
                    p,
                    ViewDelta {
                        lsn: 0,
                        seq,
                        old: old.clone(),
                        new: Arc::from(Vec::new()),
                        had_old: true,
                        has_new: false,
                    },
                );
                p.rows == 0
            });
            if empty {
                self.drop_part(&key);
            }
        }

        if after >= OWNER_ALL && after != OWNER_IRREGULAR {
            let key = self.part_key(after);
            self.with_part(&key, |b, p| {
                b.insert(p, seq, &cells);
                b.record(
                    p,
                    ViewDelta {
                        lsn: 0,
                        seq,
                        old: Arc::from(Vec::new()),
                        new: cells.clone(),
                        had_old: false,
                        has_new: true,
                    },
                );
            });
        }

        self.set_owner(seq, after);
    }

    fn own_shard(&mut self, i: usize) -> &mut Shard {
        if !self.owned_shard[i] {
            self.owned_shard[i] = true;
            let cloned = match &self.next.shards[i] {
                None => Shard::new(),
                Some(s) => (**s).clone(),
            };
            self.next.shards[i] = Some(Arc::new(cloned));
        }

        Arc::get_mut(self.next.shards[i].as_mut().expect("owned shard")).expect("unique shard")
    }

    fn with_part<R>(&mut self, key: &str, f: impl FnOnce(&mut Batch<'a>, &mut ViewPart) -> R) -> R {
        let si = shard_of(key);
        let lsn = self.lsn;
        let existing = self.own_shard(si).get(key).cloned();
        let mut part: Arc<ViewPart> = match existing {
            None => {
                let p = Arc::new(ViewPart {
                    covered: lsn,
                    version: lsn,
                    ..ViewPart::default()
                });
                self.owned_part.insert(part_ptr(&p));
                self.delta += VIEW_PART_OVERHEAD + key.len() as i64;
                p
            }
            Some(p) if self.owned_part.contains(&part_ptr(&p)) => p,
            Some(p) => {
                let mut c = (*p).clone();
                let mut chunks = Vec::with_capacity(p.chunks.len() + 1);
                chunks.extend(p.chunks.iter().cloned());
                c.chunks = chunks;
                let c = Arc::new(c);
                self.owned_part.insert(part_ptr(&c));
                c
            }
        };

        self.own_shard(si).remove(key);
        let r = {
            let p = Arc::get_mut(&mut part).expect("unique part");
            f(self, p)
        };
        self.own_shard(si).insert(key.to_string(), part);

        r
    }

    fn drop_part(&mut self, key: &str) {
        let shard = self.own_shard(shard_of(key));
        if shard.remove(key).is_some() {
            self.delta -= VIEW_PART_OVERHEAD + key.len() as i64;
        }
    }

    fn own<'c>(&mut self, p: &'c mut ViewPart, i: usize) -> &'c mut ViewChunk {
        if !self.owned_chunk.contains(&chunk_ptr(&p.chunks[i])) {
            let c = &p.chunks[i];
            let room = VIEW_CHUNK_ROWS.min(2 * c.seqs.len() + 4);
            let mut seqs = GoVec::with_cap(room);
            seqs.v.extend_from_slice(&c.seqs.v);
            let mut cells = GoVec::with_cap(room * self.width);
            cells.v.extend_from_slice(&c.cells.v);
            let n = ViewChunk { seqs, cells };
            self.delta += n.cost() - c.cost();
            let n = Arc::new(n);
            self.owned_chunk.insert(chunk_ptr(&n));
            p.chunks[i] = n;
        }

        Arc::get_mut(&mut p.chunks[i]).expect("unique chunk")
    }

    fn find(p: &ViewPart, seq: u32) -> (usize, usize, bool) {
        let ci = p
            .chunks
            .partition_point(|c| c.seqs.v[c.seqs.v.len() - 1] < seq);
        if ci == p.chunks.len() {
            return (ci, 0, false);
        }

        let s = &p.chunks[ci].seqs.v;
        let ri = s.partition_point(|x| *x < seq);

        (ci, ri, ri < s.len() && s[ri] == seq)
    }

    fn insert(&mut self, p: &mut ViewPart, seq: u32, cells: &[Val]) {
        let width = self.width;
        p.rows += 1;
        self.next.rows += 1;
        if p.chunks.is_empty() || seq > p.last {
            if seq > p.last || p.rows == 1 {
                p.last = seq;
            }

            let n = p.chunks.len();
            if n > 0 && p.chunks[n - 1].seqs.len() < VIEW_CHUNK_ROWS {
                let before_delta;
                {
                    let c = self.own(p, n - 1);
                    let grown = c.seqs.cap;
                    c.seqs.push_go(seq, 4);
                    c.cells.extend_go(cells, VAL_BYTES as usize);
                    before_delta = if c.seqs.cap != grown {
                        (c.seqs.cap as i64 - grown as i64) * (4 + width as i64 * VAL_BYTES)
                    } else {
                        0
                    };
                }
                self.delta += before_delta;
                return;
            }

            let mut seqs = GoVec::with_cap(4);
            seqs.v.push(seq);
            let mut cv = GoVec::with_cap(4 * width.max(1));
            cv.v.extend_from_slice(cells);
            cv.v.resize(width, UNDEF_VAL);
            let c = ViewChunk { seqs, cells: cv };
            self.delta += c.cost();
            let c = Arc::new(c);
            self.owned_chunk.insert(chunk_ptr(&c));
            p.chunks.push(c);
            return;
        }

        let (mut ci, mut ri, _) = Batch::find(p, seq);
        if ci == p.chunks.len() {
            ci = p.chunks.len() - 1;
            ri = p.chunks[ci].seqs.len();
        }

        let (cost_before, split) = {
            let c = self.own(p, ci);
            let before = c.cost();
            c.seqs.push_go(0, 4);
            let n = c.seqs.v.len();
            c.seqs.v.copy_within(ri..n - 1, ri + 1);
            c.seqs.v[ri] = seq;
            let pad = vec![UNDEF_VAL; width];
            c.cells.extend_go(&pad, VAL_BYTES as usize);
            let end = c.cells.v.len();
            c.cells
                .v
                .copy_within(ri * width..end - width, (ri + 1) * width);
            c.cells.v[ri * width..(ri + 1) * width].copy_from_slice(&cells[..width]);

            let mut split = None;
            if c.seqs.len() > VIEW_CHUNK_ROWS {
                let mid = c.seqs.len() / 2;
                let mut rs = GoVec::with_cap(VIEW_CHUNK_ROWS);
                rs.v.extend_from_slice(&c.seqs.v[mid..]);
                let mut rc = GoVec::with_cap(VIEW_CHUNK_ROWS * width);
                rc.v.extend_from_slice(&c.cells.v[mid * width..]);
                c.seqs.v.truncate(mid);
                c.cells.v.truncate(mid * width);
                split = Some(ViewChunk {
                    seqs: rs,
                    cells: rc,
                });
            }
            (before, split)
        };

        if let Some(right) = split {
            self.delta += right.cost();
            let right = Arc::new(right);
            self.owned_chunk.insert(chunk_ptr(&right));
            p.chunks.insert(ci + 1, right);
        }
        let after = p.chunks[ci].cost();
        self.delta += after - cost_before;
    }

    fn update(&mut self, p: &mut ViewPart, seq: u32, cells: &[Val]) -> Arc<[Val]> {
        let (ci, ri, ok) = Batch::find(p, seq);
        if !ok {
            return Arc::from(Vec::new());
        }

        let width = self.width;
        let c = self.own(p, ci);
        let row = &mut c.cells.v[ri * width..(ri + 1) * width];
        let old: Arc<[Val]> = Arc::from(row.to_vec());
        row.copy_from_slice(&cells[..width]);

        old
    }

    fn remove(&mut self, p: &mut ViewPart, seq: u32) -> Arc<[Val]> {
        let (ci, ri, ok) = Batch::find(p, seq);
        if !ok {
            return Arc::from(Vec::new());
        }

        let width = self.width;
        let (old, emptied, cost) = {
            let c = self.own(p, ci);
            let old: Arc<[Val]> = Arc::from(c.cells.v[ri * width..(ri + 1) * width].to_vec());
            c.seqs.v.remove(ri);
            c.cells.v.drain(ri * width..(ri + 1) * width);
            (old, c.seqs.len() == 0, c.cost())
        };
        p.rows -= 1;
        self.next.rows -= 1;

        if emptied {
            self.delta -= cost;
            p.chunks.remove(ci);
        }

        p.last = match p.chunks.last() {
            Some(c) => c.seqs.v[c.seqs.len() - 1],
            None => 0,
        };

        old
    }

    fn record(&mut self, p: &mut ViewPart, mut d: ViewDelta) {
        p.version = self.lsn;
        if self.rebuilding {
            p.covered = self.lsn;
            return;
        }

        if p.rows < FOLD_CACHE_MIN_ROWS && p.recent.is_empty() {
            p.covered = self.lsn;
            return;
        }

        d.lsn = self.lsn;

        if p.recent.len() >= VIEW_RECENT_DELTAS || p.recent_cap == p.recent.len() {
            let mut keep: &[ViewDelta] = &p.recent;
            if keep.len() >= VIEW_RECENT_DELTAS {
                p.covered = keep[keep.len() - VIEW_RECENT_DELTAS / 2 - 1].lsn;
                keep = &keep[keep.len() - VIEW_RECENT_DELTAS / 2..];
            }
            let cap = VIEW_RECENT_DELTAS.min(2 * keep.len() + 8);
            let mut recent = Vec::with_capacity(cap);
            recent.extend_from_slice(keep);
            p.recent = recent;
            p.recent_cap = cap;
        }
        p.recent.push(d);
    }
}

fn append_val_key(b: &mut Vec<u8>, fr: &Frame, x: Val) {
    b.push(x.k as u8);
    match x.k {
        VKind::Num | VKind::Bool => {
            b.extend_from_slice(&x.n.to_bits().to_le_bytes());
        }
        VKind::Str | VKind::Tmp => {
            let s = fr.str(x);
            b.extend_from_slice(&(s.len() as u32).to_le_bytes());
            b.extend_from_slice(s.as_bytes());
        }
        _ => {}
    }
}

impl FoldDef {
    fn run_row(&self, fr: &mut Frame, row: &[Val], base: usize) -> bool {
        fr.tmp.truncate(base);
        let r = self.body.run(fr, row);

        r != Ctl::Bail && !fr.bail
    }

    fn inert(&self, fr: &mut Frame, row: &[Val], present: bool, base: usize) -> (bool, bool) {
        if !present {
            return (true, true);
        }

        for r in fr.regs.iter_mut() {
            *r = num_val(0.0);
        }
        fr.touched = false;
        if !self.run_row(fr, row, base) {
            return (false, false);
        }

        (!fr.touched, true)
    }

    fn full_pass(
        &self,
        fr: &mut Frame,
        part: &ViewPart,
        base: usize,
        width: usize,
        check: &dyn Fn() -> Result<(), GuardError>,
    ) -> Result<bool, GuardError> {
        for c in &part.chunks {
            check()?;

            for i in 0..c.seqs.len() {
                if !self.run_row(fr, &c.cells.v[i * width..(i + 1) * width], base) {
                    return Ok(false);
                }
            }
        }

        Ok(true)
    }

    fn incremental(
        &self,
        fr: &mut Frame,
        e: &FoldEntry,
        part: &ViewPart,
        base: usize,
    ) -> Option<(Vec<Val>, u32)> {
        if e.ver < part.covered {
            return None;
        }

        let i = part.recent.partition_point(|d| d.lsn <= e.ver);
        let mut regs = e.regs.clone();
        let mut last = e.last;
        for d in &part.recent[i..] {
            if !d.had_old && d.seq > last {
                fr.regs.copy_from_slice(&regs);
                fr.touched = false;
                if !self.run_row(fr, &d.new, base) {
                    return None;
                }
                regs.copy_from_slice(&fr.regs);
                last = d.seq;
                continue;
            }

            if !self.accumulating {
                return None;
            }

            let (old_inert, ok1) = self.inert(fr, &d.old, d.had_old, base);
            let (new_inert, ok2) = self.inert(fr, &d.new, d.has_new, base);
            if !ok1 || !ok2 || !old_inert || !new_inert {
                return None;
            }
        }

        Some((regs, last))
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate(
        &self,
        rt: &mut Runtime,
        v: &View,
        st: &ViewState,
        part_key: &str,
        regs_in: &[Value],
        free_in: &[Value],
        lane: &dyn Fn() -> Result<api::LaneSlot, GuardError>,
        check: &dyn Fn() -> Result<(), GuardError>,
    ) -> JsResult<Result<FoldOut, GuardError>> {
        let strs = st.table.snapshot();
        let mut fr = acquire_frame(
            self.regs.len(),
            self.locals,
            self.lits.len(),
            self.frees.len(),
            strs,
        );
        let out = self.evaluate_in(rt, &mut fr, v, st, part_key, regs_in, free_in, lane, check);
        release_frame(fr);
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn evaluate_in(
        &self,
        rt: &mut Runtime,
        fr: &mut Frame,
        v: &View,
        st: &ViewState,
        part_key: &str,
        regs_in: &[Value],
        free_in: &[Value],
        lane: &dyn Fn() -> Result<api::LaneSlot, GuardError>,
        check: &dyn Fn() -> Result<(), GuardError>,
    ) -> JsResult<Result<FoldOut, GuardError>> {
        for (i, s) in self.lits.iter().enumerate() {
            fr.lits[i] = View::str_val(&st.table, fr, s);
        }

        for (i, x) in regs_in.iter().enumerate() {
            let Some(c) = View::to_val(rt, &st.table, fr, x)? else {
                return Ok(Ok(None));
            };
            fr.regs[i] = c;
        }

        for (i, x) in free_in.iter().enumerate() {
            let Some(c) = View::to_val(rt, &st.table, fr, x)? else {
                return Ok(Ok(None));
            };
            fr.free[i] = c;
        }

        let base = fr.tmp.len();
        let width = self.fields.len();
        let part = st.part(part_key);

        let output = |fr: &Frame| -> (Vec<Val>, Vec<String>) {
            let out = fr.regs.clone();
            let names = out
                .iter()
                .map(|r| {
                    if is_string(*r) {
                        fr.str(*r).to_string()
                    } else {
                        String::new()
                    }
                })
                .collect();
            (out, names)
        };

        let Some(part) = part.filter(|p| p.rows > 0) else {
            return Ok(Ok(Some(output(fr))));
        };

        if part.rows < FOLD_CACHE_MIN_ROWS {
            FOLD_COUNTERS.cached.fetch_add(1, Ordering::SeqCst);
            match self.full_pass(fr, part, base, width, check) {
                Err(e) => return Ok(Err(e)),
                Ok(false) => return Ok(Ok(None)),
                Ok(true) => {}
            }
            fr.tmp.truncate(base);
            return Ok(Ok(Some(output(fr))));
        }

        let mut key_buf: Vec<u8> = Vec::with_capacity(96);
        key_buf.extend_from_slice(&self.id);
        key_buf.extend_from_slice(part_key.as_bytes());
        key_buf.push(0);
        for r in fr.regs.iter() {
            append_val_key(&mut key_buf, fr, *r);
        }
        for f in fr.free.iter() {
            append_val_key(&mut key_buf, fr, *f);
        }
        let key = key_buf;

        let initial = fr.regs.clone();
        let e = st.folds.get(&key);
        if let Some(e) = &e
            && e.ver == part.version
        {
            FOLD_COUNTERS.cached.fetch_add(1, Ordering::SeqCst);
            fr.regs.copy_from_slice(&e.regs);
            return Ok(Ok(Some(output(fr))));
        }

        if let Some(e) = &e
            && e.ver < part.version
        {
            if let Some((regs, last)) = self.incremental(fr, e, part, base) {
                FOLD_COUNTERS.incremental.fetch_add(1, Ordering::SeqCst);
                fr.regs.copy_from_slice(&regs);
                fr.tmp.truncate(base);
                if cacheable_regs(&regs) {
                    let delta = st.folds.put(
                        key,
                        FoldEntry {
                            ver: part.version,
                            last,
                            regs,
                        },
                    );
                    v.add_cost(delta);
                }
                return Ok(Ok(Some(output(fr))));
            }
            fr.bail = false;
        }

        let mut _slot = None;
        if part.rows >= FOLD_LANE_MIN_ROWS {
            match lane() {
                Ok(s) => _slot = Some(s),
                Err(e) => return Ok(Err(e)),
            }
        }

        FOLD_COUNTERS.full.fetch_add(1, Ordering::SeqCst);
        fr.regs.copy_from_slice(&initial);
        match self.full_pass(fr, part, base, width, check) {
            Err(e) => return Ok(Err(e)),
            Ok(false) => return Ok(Ok(None)),
            Ok(true) => {}
        }
        fr.tmp.truncate(base);

        if cacheable_regs(&fr.regs) && e.as_ref().is_none_or(|e| e.ver < part.version) {
            let delta = st.folds.put(
                key,
                FoldEntry {
                    ver: part.version,
                    last: part.last,
                    regs: fr.regs.clone(),
                },
            );
            v.add_cost(delta);
        }

        Ok(Ok(Some(output(fr))))
    }
}

fn cacheable_regs(regs: &[Val]) -> bool {
    regs.iter()
        .all(|r| r.k != VKind::Tmp && r.k != VKind::Arr && r.k != VKind::Opaque)
}

fn partition_value(raw: &[u8]) -> Option<String> {
    if raw.contains(&b'*') || raw.contains(&b'\\') {
        return None;
    }

    if docpath::parse_date(raw).is_some() {
        return None;
    }

    Some(String::from_utf8_lossy(&strings::to_lower(raw)).into_owned())
}

pub(crate) struct ViewToken {
    pub(crate) target: String,
    pub(crate) filters: Filters,
    pub(crate) core: bool,
    pub(crate) key: Vec<u8>,
}

impl GoMethods for ViewToken {
    fn methods(&self) -> &'static [GoMethod] {
        &[]
    }

    fn call(&self, _rt: &mut Runtime, _idx: usize, _call: FunctionCall<'_>) -> JsResult<Value> {
        Ok(Value::Undefined)
    }

    fn same(&self, other: &dyn GoMethods) -> bool {
        other
            .as_any()
            .downcast_ref::<ViewToken>()
            .is_some_and(|o| std::ptr::eq(o, self))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) type PinnedViews = HashMap<usize, (Arc<View>, Arc<ViewState>)>;

pub(crate) fn fold_output(vals: &[Val], names: &[String]) -> Vec<Value> {
    vals.iter()
        .zip(names)
        .map(|(v, name)| match v.k {
            VKind::Num => float_to_value(v.n),
            VKind::Bool => Value::Bool(v.n != 0.0),
            VKind::Str | VKind::Tmp => {
                Value::String(talos::export::string_from_go(name.as_bytes()))
            }
            VKind::Null => Value::Null,
            _ => Value::Undefined,
        })
        .collect()
}

type FoldOut = Option<(Vec<Val>, Vec<String>)>;

impl Request {
    fn pinned_view(
        &self,
        v: &Arc<View>,
        e: &Arc<Engine>,
    ) -> Result<(Arc<ViewState>, bool), (ViewErr, bool)> {
        let id = Arc::as_ptr(v) as usize;
        let pinned = self.inner.st.lock().pins.get(&id).map(|p| p.1.clone());
        if let Some(st) = pinned {
            return Ok((st, false));
        }

        let (st, cold) = v.current(e, self.guard())?;

        let mut s = self.inner.st.lock();
        let st = match s.pins.get(&id) {
            Some(cur) => cur.1.clone(),
            None => {
                s.pins.insert(id, (v.clone(), st.clone()));
                st
            }
        };

        Ok((st, cold))
    }

    pub(crate) fn fold(
        &self,
        rt: &mut Runtime,
        h: &Hook,
        def: &FoldDef,
        tok: &ViewToken,
        regs: &[Value],
        free: &[Value],
    ) -> JsResult<Result<FoldOut, GuardError>> {
        let out = self.fold_on_view(rt, h, def, tok, regs, free)?;
        if matches!(out, Ok(None)) {
            FOLD_COUNTERS.fallbacks.fetch_add(1, Ordering::SeqCst);
        }

        Ok(out)
    }

    fn fold_on_view(
        &self,
        rt: &mut Runtime,
        h: &Hook,
        def: &FoldDef,
        tok: &ViewToken,
        regs: &[Value],
        free: &[Value],
    ) -> JsResult<Result<FoldOut, GuardError>> {
        if FOLDS_DISABLED.load(Ordering::SeqCst) || tok.core {
            return Ok(Ok(None));
        }

        let Some(e) = storage::engine() else {
            return Ok(Ok(None));
        };

        if !h.bypass_acl
            && acl::read_mode_for(self.username().as_bytes(), tok.target.as_bytes())
                != ReadMode::ReadAll
        {
            return Ok(Ok(None));
        }

        let mut part_key = String::new();
        let mut base = tok.filters.clone();
        if !def.part_field.is_empty() {
            let Some(cond) = tok.filters.get(def.part_field.as_bytes()) else {
                return Ok(Ok(None));
            };
            if cond.len() != 1 {
                return Ok(Ok(None));
            }

            let eq = cond.get(b"eq".as_slice()).cloned().unwrap_or_default();
            let Some(p) = partition_value(&eq) else {
                return Ok(Ok(None));
            };
            part_key = p;

            base = tok
                .filters
                .iter()
                .filter(|(f, _)| f.as_slice() != def.part_field.as_bytes())
                .map(|(f, ops)| (f.clone(), ops.clone()))
                .collect();
        }

        if !api::extract_auto_includes(&base).is_empty() {
            return Ok(Ok(None));
        }

        let Some(v) = VIEWS.get(&e, &tok.target, &base, &def.part_field, &def.fields) else {
            return Ok(Ok(None));
        };
        v.used.store(unix_now(), Ordering::SeqCst);

        let start = Instant::now();
        let (st, cold) = match self.pinned_view(&v, &e) {
            Ok(r) => r,
            Err((ViewErr::Refused, _)) | Err((ViewErr::NotReady, _)) => return Ok(Ok(None)),
            Err((ViewErr::Guard(g), _)) => return Ok(Err(g)),
            Err(_) => return Ok(Ok(None)),
        };

        if st.irregular > 0 {
            return Ok(Ok(None));
        }

        let patient: Option<Guard> = self.inner_guard().map(|g| Guard::until(Some(g), None));
        let lane = || api::acquire_hook_lane(patient.as_ref());
        let guard = self.guard();
        let check = || api::guard::check(guard);

        let out = match def.evaluate(rt, &v, &st, &part_key, regs, free, &lane, &check)? {
            Err(e) => return Ok(Err(e)),
            Ok(None) => return Ok(Ok(None)),
            Ok(Some(o)) => o,
        };

        if cold {
            RESULTS.misses.fetch_add(1, Ordering::SeqCst);
        } else {
            RESULTS.hits.fetch_add(1, Ordering::SeqCst);
        }

        let rows = st.part(&part_key).map_or(0, |p| p.rows);
        observe_fold(h, rows, !cold, start);

        Ok(Ok(Some(out)))
    }
}

pub fn views_ready() -> bool {
    for v in VIEWS.list() {
        if v.pinned.load(Ordering::SeqCst)
            && v.state.load().is_none()
            && unix_now() >= v.refused.load(Ordering::SeqCst)
        {
            return false;
        }
    }

    true
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FoldStats {
    pub cached: i64,
    pub incremental: i64,
    pub full_passes: i64,
    pub fallbacks: i64,
    pub views: usize,
    pub bytes: i64,
}

struct FoldCounters {
    cached: AtomicI64,
    incremental: AtomicI64,
    full: AtomicI64,
    fallbacks: AtomicI64,
}

static FOLD_COUNTERS: FoldCounters = FoldCounters {
    cached: AtomicI64::new(0),
    incremental: AtomicI64::new(0),
    full: AtomicI64::new(0),
    fallbacks: AtomicI64::new(0),
};

pub fn folds() -> FoldStats {
    let list = VIEWS.list();
    let bytes = list.iter().map(|v| v.bytes.load(Ordering::SeqCst)).sum();

    FoldStats {
        cached: FOLD_COUNTERS.cached.load(Ordering::SeqCst),
        incremental: FOLD_COUNTERS.incremental.load(Ordering::SeqCst),
        full_passes: FOLD_COUNTERS.full.load(Ordering::SeqCst),
        fallbacks: FOLD_COUNTERS.fallbacks.load(Ordering::SeqCst),
        views: list.len(),
        bytes,
    }
}
