use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::{Arc, LazyLock, Once, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use crate::acl::{self, ReadMode};
use crate::api::{self, Guard, GuardError, ListQuery};
use crate::datastore::{Doc, Engine, Snapshot, decode_map};
use crate::globals;
use crate::gocompat::json::Map;
use crate::log::{self, Throttle};
use crate::memory;
use crate::query::Filters;
use crate::sync::{Condvar, Mutex};

use super::hook::HOOK_LOG;
use super::memo::OUTPUTS;
use super::natives::Aggregates;
use super::rows::{Row, RowBuilder};
use super::view::VIEWS;

pub(crate) struct RowBuffer {
    slots: Box<[OnceLock<Row>]>,
    frontier: AtomicI64,
}

impl RowBuffer {
    fn with_cap(cap: usize) -> RowBuffer {
        RowBuffer {
            slots: (0..cap).map(|_| OnceLock::new()).collect(),
            frontier: AtomicI64::new(0),
        }
    }
}

#[derive(Clone)]
pub(crate) struct RowSlice {
    buf: Arc<RowBuffer>,
    len: usize,
    cap: usize,
}

impl RowSlice {
    fn empty() -> RowSlice {
        RowSlice {
            buf: Arc::new(RowBuffer::with_cap(0)),
            len: 0,
            cap: 0,
        }
    }

    fn from_rows(rows: Vec<Row>, cap: usize) -> RowSlice {
        let cap = cap.max(rows.len());
        let buf = RowBuffer::with_cap(cap);
        let len = rows.len();
        for (i, r) in rows.into_iter().enumerate() {
            let _ = buf.slots[i].set(r);
        }
        RowSlice {
            buf: Arc::new(buf),
            len,
            cap,
        }
    }

    fn get(&self, i: usize) -> &Row {
        self.buf.slots[i].get().expect("row slot")
    }
}

pub(crate) struct QueryResult {
    pub(crate) version: String,
    pub(crate) entity: String,
    rows: RowSlice,
    pub(crate) bytes: i64,
    pub(crate) by_seq: bool,
    pub(crate) advanced: bool,
    pub(crate) instance: u64,
    pub(crate) generation: u64,
    pub(crate) lsn: u64,
    frontier: bool,
    pub(crate) agg: OnceLock<Aggregates>,
}

impl Default for QueryResult {
    fn default() -> QueryResult {
        QueryResult {
            version: String::new(),
            entity: String::new(),
            rows: RowSlice::empty(),
            bytes: 0,
            by_seq: false,
            advanced: false,
            instance: 0,
            generation: 0,
            lsn: 0,
            frontier: false,
            agg: OnceLock::new(),
        }
    }
}

impl QueryResult {
    pub(crate) fn len(&self) -> usize {
        self.rows.len
    }

    pub(crate) fn row(&self, i: usize) -> &Row {
        self.rows.get(i)
    }

    pub(crate) fn rows(&self) -> impl Iterator<Item = &Row> {
        (0..self.rows.len).map(|i| self.rows.get(i))
    }

    fn reusable(&self, seq: u32, lsn: u64) -> Option<Row> {
        if !self.by_seq || lsn == 0 {
            return None;
        }

        let n = self.rows.len;
        let (mut lo, mut hi) = (0usize, n);
        while lo < hi {
            let mid = lo + (hi - lo) / 2;
            if self.rows.get(mid).seq >= seq {
                hi = mid;
            } else {
                lo = mid + 1;
            }
        }

        if lo < n {
            let r = self.rows.get(lo);
            if r.seq == seq && r.lsn == lsn {
                return Some(r.clone());
            }
        }

        None
    }
}

struct CacheItem {
    res: Arc<QueryResult>,
    tick: u64,
    used: i64,
}

type FlightResult = Option<Result<Arc<QueryResult>, GuardError>>;

struct Flight {
    done: Mutex<FlightResult>,
    cv: Condvar,
}

impl Flight {
    fn new() -> Flight {
        Flight {
            done: Mutex::new(None),
            cv: Condvar::new(),
        }
    }

    fn finish(&self, r: Result<Arc<QueryResult>, GuardError>) {
        *self.done.lock() = Some(r);
        self.cv.notify_all();
    }
}

#[derive(Default)]
struct CacheState {
    items: HashMap<Vec<u8>, CacheItem>,
    lru: BTreeMap<u64, Vec<u8>>,
    next_tick: u64,
    flights: HashMap<Vec<u8>, Arc<Flight>>,
    refused: HashMap<Vec<u8>, Instant>,
    swept: Option<Instant>,
}

impl CacheState {
    fn tick(&mut self) -> u64 {
        self.next_tick += 1;
        self.next_tick
    }

    fn touch(&mut self, key: &[u8]) {
        let t = self.tick();
        if let Some(it) = self.items.get_mut(key) {
            self.lru.remove(&it.tick);
            it.tick = t;
            self.lru.insert(t, key.to_vec());
        }
    }

    fn remove(&mut self, key: &[u8]) -> Option<CacheItem> {
        let it = self.items.remove(key)?;
        self.lru.remove(&it.tick);
        Some(it)
    }

    fn evict(&mut self, limit: i64) {
        while limit > 0 && memory::HOOK_RESULTS.used() > limit && self.lru.len() > 1 {
            let Some((_, key)) = self.lru.pop_first() else {
                break;
            };
            if let Some(it) = self.items.remove(&key) {
                memory::HOOK_RESULTS.release(it.res.bytes);
            }
        }
    }
}

pub(crate) struct ResultCache {
    st: Mutex<CacheState>,
    pub(crate) hits: AtomicI64,
    pub(crate) misses: AtomicI64,
}

pub(crate) static RESULTS: LazyLock<ResultCache> = LazyLock::new(|| ResultCache {
    st: Mutex::new(CacheState::default()),
    hits: AtomicI64::new(0),
    misses: AtomicI64::new(0),
});

static INIT: Once = Once::new();

pub(crate) fn ensure_init() {
    INIT.call_once(|| {
        memory::on_level(|_| RESULTS.shrink());
    });
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() as i64)
}

const RESULT_SWEEP_EVERY: Duration = Duration::from_secs(30);
const RESULT_IDLE_AFTER: i64 = 10 * 60;
const RELEASE_AFTER_SWEEP: i64 = 64 << 20;
const REFUSED_FOR: Duration = Duration::from_secs(30);

static OVERSIZED: Throttle = Throttle::new(Duration::from_secs(60));

pub(crate) type BuildFn =
    Box<dyn FnOnce(Option<Arc<QueryResult>>) -> Result<QueryResult, GuardError> + Send>;

impl ResultCache {
    fn lookup(&self, key: &[u8], version: &str) -> Option<Arc<QueryResult>> {
        let mut st = self.st.lock();
        let it = st.items.get(key)?;
        if it.res.version != version {
            return None;
        }

        let res = it.res.clone();
        st.touch(key);
        if let Some(it) = st.items.get_mut(key) {
            it.used = unix_now();
        }

        Some(res)
    }

    fn store(&self, key: &[u8], res: &Arc<QueryResult>) -> bool {
        let limit = memory::HOOK_RESULTS.effective();
        if limit > 0 && res.bytes > limit {
            return false;
        }

        let mut st = self.st.lock();
        let t = st.tick();
        if let Some(it) = st.items.get_mut(key) {
            memory::HOOK_RESULTS.release(it.res.bytes);
            it.res = res.clone();
            it.used = unix_now();
            let old = it.tick;
            it.tick = t;
            st.lru.remove(&old);
            st.lru.insert(t, key.to_vec());
        } else {
            st.items.insert(
                key.to_vec(),
                CacheItem {
                    res: res.clone(),
                    tick: t,
                    used: unix_now(),
                },
            );
            st.lru.insert(t, key.to_vec());
        }
        memory::HOOK_RESULTS.add(res.bytes);

        st.evict(memory::HOOK_RESULTS.effective());

        true
    }

    pub(crate) fn shrink(&self) {
        let mut st = self.st.lock();
        st.evict(memory::HOOK_RESULTS.effective());
    }

    pub(crate) fn reset(&self) {
        let mut st = self.st.lock();
        for it in st.items.values() {
            memory::HOOK_RESULTS.release(it.res.bytes);
        }
        st.items = HashMap::new();
        st.refused = HashMap::new();
        st.lru = BTreeMap::new();
    }

    pub(crate) fn sweep(&self, e: &Engine) {
        let mut st = self.st.lock();
        if st.swept.is_some_and(|t| t.elapsed() < RESULT_SWEEP_EVERY) {
            return;
        }
        st.swept = Some(Instant::now());

        let cutoff = unix_now() - RESULT_IDLE_AFTER;
        let mut freed: i64 = 0;
        let mut stale_keys: Vec<Vec<u8>> = Vec::new();
        for (key, it) in st.items.iter() {
            let mut stale = it.used < cutoff;
            if !stale && it.res.by_seq && !it.res.entity.is_empty() {
                stale = it.res.instance != e.instance()
                    || e.snapshot(it.res.entity.as_bytes()).generation() != it.res.generation;
            }
            if stale {
                stale_keys.push(key.clone());
            }
        }
        for key in stale_keys {
            if let Some(it) = st.remove(&key) {
                memory::HOOK_RESULTS.release(it.res.bytes);
                freed += it.res.bytes;
            }
        }
        drop(st);

        freed += OUTPUTS.sweep(cutoff);
        if freed >= RELEASE_AFTER_SWEEP {
            memory::free_os_memory();
        }
    }

    pub(crate) fn get(
        &self,
        key: &[u8],
        version: &str,
        guard: Option<&Guard>,
        build: BuildFn,
    ) -> Result<(Arc<QueryResult>, bool), GuardError> {
        ensure_init();
        if version.is_empty() {
            let res = build(None)?;
            return Ok((Arc::new(res), false));
        }

        let mut build = Some(build);
        loop {
            if let Some(res) = self.lookup(key, version) {
                self.hits.fetch_add(1, Ordering::SeqCst);
                return Ok((res, true));
            }

            let mut st = self.st.lock();
            if let Some(until) = st.refused.get(key).copied() {
                if Instant::now() < until {
                    drop(st);
                    self.misses.fetch_add(1, Ordering::SeqCst);
                    return Err(api::guard::reject(guard));
                }
                st.refused.remove(key);
            }

            let existing = st.flights.get(key).cloned();
            match existing {
                None => {
                    let f = Arc::new(Flight::new());
                    st.flights.insert(key.to_vec(), f.clone());
                    drop(st);

                    let b = build.take().expect("single build");
                    let fk = key.to_vec();
                    let fv = version.to_string();
                    let ff = f.clone();
                    drop(crate::sync::spawn("hook-query-fill", move || {
                        RESULTS.fill(&fk, &fv, &ff, b);
                    }));

                    let r = wait_flight(&f, guard);
                    let counted = self.counted(r.as_ref().ok());
                    return r.map(|res| (res, counted));
                }
                Some(f) => {
                    drop(st);
                    let res = wait_flight(&f, guard)?;
                    if res.version == version {
                        let counted = self.counted(Some(&res));
                        return Ok((res, counted));
                    }
                }
            }
        }
    }

    fn counted(&self, res: Option<&Arc<QueryResult>>) -> bool {
        if res.is_some_and(|r| r.advanced) {
            self.hits.fetch_add(1, Ordering::SeqCst);
            return true;
        }

        self.misses.fetch_add(1, Ordering::SeqCst);

        false
    }

    fn fill(&self, key: &[u8], version: &str, f: &Flight, build: BuildFn) {
        let prev = {
            let st = self.st.lock();
            st.items.get(key).map(|it| it.res.clone())
        };

        let mut out: Result<Arc<QueryResult>, GuardError> = match build(prev) {
            Ok(mut res) => {
                res.version = version.to_string();
                Ok(Arc::new(res))
            }
            Err(e) => Err(e),
        };

        if let Ok(res) = &out
            && !self.store(key, res)
            && memory::budget() > 0
        {
            memory::HOOK_RESULTS.refuse();
            out = Err(GuardError::Busy);
        }

        let mut st = self.st.lock();
        st.flights.remove(key);
        if matches!(out, Err(GuardError::Busy)) {
            st.refused
                .insert(key.to_vec(), Instant::now() + REFUSED_FOR);
            let entity = key.split(|b| *b == 0).next().unwrap_or_default().to_vec();
            let (suppressed, ok) = OVERSIZED.allow(&entity);
            if ok {
                let share = memory::HOOK_RESULTS.effective() >> 20;
                HOOK_LOG.warn(
                    "hook query result larger than its share of the memory budget, refused",
                    &log::with_suppressed(
                        &[
                            ("entity", log::Value::Str(&entity)),
                            ("share_mb", log::Value::Int(share)),
                        ],
                        suppressed,
                    ),
                );
            }
        }
        drop(st);
        f.finish(out);
    }
}

fn wait_flight(f: &Flight, guard: Option<&Guard>) -> Result<Arc<QueryResult>, GuardError> {
    let timeout = api::queue_timeout();
    let start = Instant::now();
    let mut done = f.done.lock();
    loop {
        if let Some(r) = done.as_ref() {
            return r.clone();
        }

        let (g, timed_out) = f.cv.wait_timeout(done, Duration::from_millis(20));
        done = g;
        if done.is_some() {
            continue;
        }
        if !timed_out {
            continue;
        }

        drop(done);
        api::guard::check(guard)?;
        if timeout.0 >= 0 && start.elapsed().as_nanos() as i64 >= timeout.0 {
            return Err(api::guard::reject(guard));
        }
        done = f.done.lock();
    }
}

fn list_for_hook(
    target: &str,
    filters: &Filters,
    guard: Option<&Guard>,
) -> Result<Vec<Map>, GuardError> {
    let (docs, err) = api::list_entities_guarded(
        target.as_bytes(),
        0,
        0,
        b"",
        true,
        Some(filters),
        b"",
        b"",
        guard,
    );
    match err {
        Some(e) => Err(e),
        None => Ok(docs),
    }
}

pub(crate) fn build_result(
    target: &str,
    filters: &Filters,
    owner: &str,
    bypass_acl: bool,
    guard: Option<&Guard>,
    prev: Option<Arc<QueryResult>>,
) -> Result<QueryResult, GuardError> {
    let mode = if bypass_acl {
        ReadMode::ReadAll
    } else {
        acl::read_mode_for(owner.as_bytes(), target.as_bytes())
    };

    let mut res = QueryResult {
        by_seq: true,
        entity: target.to_string(),
        ..QueryResult::default()
    };
    if mode == ReadMode::ReadNone {
        return Ok(res);
    }

    let mut b = RowBuilder::new();
    let keep = |doc: &Map| mode != ReadMode::ReadOwned || acl::owned_by(doc, owner.as_bytes());

    let includes = api::merge_includes(b"", &api::extract_auto_includes(filters));
    if !includes.is_empty() {
        let docs = list_for_hook(target, filters, guard)?;

        res.by_seq = false;
        let mut rows = Vec::new();
        for doc in &docs {
            if keep(doc) {
                rows.push(b.build(doc, 0, 0, 0));
            }
        }
        let cap = rows.capacity();
        res.rows = RowSlice::from_rows(rows, cap);
        res.bytes = b.bytes;

        return Ok(res);
    }

    let Some(snap) = api::snapshot(target.as_bytes()) else {
        return Ok(res);
    };
    let prev = prev.filter(|p| p.instance == snap.instance());

    if let Some(p) = &prev
        && let Some(next) = advance(p, &snap, filters, &keep, &mut b)
    {
        return Ok(next);
    }

    let planned = api::plan(ListQuery {
        entity: target.as_bytes(),
        filters: Some(filters),
        guard,
        snapshot: Some(snap.clone()),
        ..ListQuery::default()
    });
    if let Some(err) = planned.err {
        return Err(err);
    }
    res.instance = snap.instance();
    res.generation = snap.generation();
    res.lsn = snap.version();
    res.frontier = true;

    let (rows, built) = build_rows(&planned.docs, prev.as_deref(), &keep)?;

    b.bytes += built;
    for i in 1..rows.len() {
        if rows[i].seq <= rows[i - 1].seq {
            res.by_seq = false;
            break;
        }
    }

    let cap = rows.len();
    res.bytes = b.bytes + cap as i64 * 48;
    res.rows = RowSlice::from_rows(rows, cap);
    res.rows
        .buf
        .frontier
        .store(res.rows.len as i64, Ordering::SeqCst);

    Ok(res)
}

const ROW_SLICE_BYTES: i64 = 72;

fn advance(
    r: &QueryResult,
    snap: &Snapshot,
    filters: &Filters,
    keep: &dyn Fn(&Map) -> bool,
    b: &mut RowBuilder,
) -> Option<QueryResult> {
    if !r.by_seq || !r.frontier || r.generation != snap.generation() || r.lsn > snap.version() {
        return None;
    }

    let mut seqs = snap.changed_since(r.lsn)?;
    if seqs.len() > r.rows.len / 4 + 64 {
        return None;
    }
    seqs.sort_unstable();

    let mut updates: HashMap<u32, Option<Row>> = HashMap::with_capacity(seqs.len());
    for &seq in &seqs {
        updates.insert(seq, None);
        let Some(d) = snap.by_seq(seq) else {
            continue;
        };

        let raw = d.bytes().ok()?;

        let doc = match decode_map(&raw) {
            Ok(Some(doc)) => doc,
            _ => continue,
        };
        if !api::filters_match_entity(&doc, filters) || !keep(&doc) {
            continue;
        }

        let built = b.build(&doc, seq, d.lsn(), raw.len());
        updates.insert(seq, Some(built));
    }

    let mut next = QueryResult {
        by_seq: true,
        advanced: true,
        entity: r.entity.clone(),
        instance: r.instance,
        generation: snap.generation(),
        lsn: snap.version(),
        bytes: r.bytes + b.bytes,
        frontier: true,
        ..QueryResult::default()
    };
    let n = r.rows.len;
    let last = if n > 0 { r.rows.get(n - 1).seq } else { 0 };

    let mut added: Vec<Row> = Vec::with_capacity(seqs.len());
    let mut append_only = true;
    for seq in &seqs {
        if *seq <= last {
            append_only = false;
            break;
        }
        if let Some(Some(u)) = updates.get(seq) {
            added.push(u.clone());
        }
    }

    if append_only {
        let total = n + added.len();
        if r.rows.cap >= total
            && r.rows
                .buf
                .frontier
                .compare_exchange(n as i64, total as i64, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
        {
            for (i, row) in added.into_iter().enumerate() {
                let _ = r.rows.buf.slots[n + i].set(row);
            }
            next.rows = RowSlice {
                buf: r.rows.buf.clone(),
                len: total,
                cap: total,
            };
        } else {
            let cap = total * 5 / 4 + 1024;
            let mut rows: Vec<Row> = Vec::with_capacity(total);
            rows.extend(r.rows().cloned());
            rows.extend(added);
            next.rows = RowSlice::from_rows(rows, cap);
            next.rows.buf.frontier.store(total as i64, Ordering::SeqCst);
            next.bytes += (cap as i64 - r.rows.cap as i64) * ROW_SLICE_BYTES;
        }

        return Some(next);
    }

    let cap = n + seqs.len();
    let mut rows: Vec<Row> = Vec::with_capacity(cap);
    let mut i = 0usize;
    for seq in &seqs {
        while i < n && r.rows.get(i).seq < *seq {
            rows.push(r.rows.get(i).clone());
            i += 1;
        }
        if i < n && r.rows.get(i).seq == *seq {
            next.bytes -= r.rows.get(i).size as i64;
            i += 1;
        }
        if let Some(Some(u)) = updates.get(seq) {
            rows.push(u.clone());
        }
    }
    while i < n {
        rows.push(r.rows.get(i).clone());
        i += 1;
    }
    let len = rows.len();
    next.rows = RowSlice::from_rows(rows, cap);
    next.rows.buf.frontier.store(len as i64, Ordering::SeqCst);

    Some(next)
}

const PARALLEL_BUILD_ROWS: usize = 8192;
const BUILD_CHECK_EVERY: usize = 1024;

fn build_rows(
    docs: &[Doc],
    prev: Option<&QueryResult>,
    keep: &(dyn Fn(&Map) -> bool + Sync),
) -> Result<(Vec<Row>, i64), GuardError> {
    let mut workers = 1usize;
    if docs.len() >= PARALLEL_BUILD_ROWS {
        workers = 4.min(globals::procs() / 2).max(1);
    }

    let total = AtomicI64::new(0);
    let refused = AtomicBool::new(false);
    let per = docs.len().div_ceil(workers);

    let work = |w: usize, lo: usize, hi: usize| -> (Vec<Row>, i64) {
        let mut b = RowBuilder::new();
        let mut size: i64 = 0;
        let mut out: Vec<Row> = Vec::with_capacity(hi - lo);
        let mut i = lo;
        while i < hi && !refused.load(Ordering::SeqCst) {
            let d = &docs[i];
            let (seq, lsn) = (d.seq(), d.lsn());
            if let Some(r) = prev.and_then(|p| p.reusable(seq, lsn)) {
                b.bytes += r.size as i64;
                out.push(r);
            } else if let Ok(raw) = d.bytes()
                && let Ok(Some(doc)) = decode_map(&raw)
                && keep(&doc)
            {
                out.push(b.build(&doc, seq, lsn, raw.len()));
            }

            if (i - lo) % BUILD_CHECK_EVERY == BUILD_CHECK_EVERY - 1 {
                let grown = total.fetch_add(b.bytes - size, Ordering::SeqCst)
                    + (b.bytes - size)
                    + docs.len() as i64 * ROW_SLICE_BYTES;
                size = b.bytes;
                let limit = memory::HOOK_RESULTS.effective();
                if memory::budget() > 0 && limit > 0 && grown > limit {
                    refused.store(true, Ordering::SeqCst);
                }
            }
            i += 1;
        }
        let _ = w;
        (out, b.bytes)
    };

    let parts: Vec<(Vec<Row>, i64)> = if workers == 1 {
        vec![work(0, 0, docs.len())]
    } else {
        std::thread::scope(|s| {
            let handles: Vec<_> = (0..workers)
                .filter_map(|w| {
                    let (lo, hi) = (w * per, ((w + 1) * per).min(docs.len()));
                    if lo >= hi {
                        return None;
                    }
                    let work = &work;
                    Some(s.spawn(move || work(w, lo, hi)))
                })
                .collect();
            handles
                .into_iter()
                .map(|h| match h.join() {
                    Ok(p) => p,
                    Err(p) => std::panic::resume_unwind(p),
                })
                .collect()
        })
    };

    if refused.load(Ordering::SeqCst) {
        memory::HOOK_RESULTS.refuse();
        return Err(GuardError::Busy);
    }

    let n: usize = parts.iter().map(|p| p.0.len()).sum();
    let mut rows: Vec<Row> = Vec::with_capacity(n);
    let mut built: i64 = 0;
    for (p, size) in parts {
        rows.extend(p);
        built += size;
    }

    Ok((rows, built))
}

pub fn reset_query_cache() {
    RESULTS.reset();
    VIEWS.reset();
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QueryCacheStats {
    pub entries: usize,
    pub bytes: i64,
    pub hits: i64,
    pub misses: i64,
}

pub fn query_cache() -> QueryCacheStats {
    let entries = RESULTS.st.lock().items.len();

    QueryCacheStats {
        entries,
        bytes: memory::HOOK_RESULTS.used(),
        hits: RESULTS.hits.load(Ordering::SeqCst),
        misses: RESULTS.misses.load(Ordering::SeqCst),
    }
}
