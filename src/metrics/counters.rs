use std::collections::BTreeMap;
use std::error::Error as StdError;
use std::sync::atomic::{AtomicI64, AtomicU64, Ordering};
use std::sync::{Arc, LazyLock};
use std::time::Duration;

use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::sort;
use crate::gocompat::time::Time;
use crate::sync::{Mutex, RwLock};

use super::histogram::{Hist, Histogram, nanos};
use super::sampler::Moment;
use super::series::{Number, num, round};

pub const C_HTTP: usize = 0;
pub const C_HTTP2XX: usize = 1;
pub const C_HTTP3XX: usize = 2;
pub const C_HTTP4XX: usize = 3;
pub const C_HTTP5XX: usize = 4;
pub const C_UNAUTHORIZED: usize = 5;
pub const C_FORBIDDEN: usize = 6;
pub const C_SLOW: usize = 7;
pub const C_TCP: usize = 8;
pub const C_TCP_GET: usize = 9;
pub const C_TCP_SET: usize = 10;
pub const C_TCP_MGET: usize = 11;
pub const C_TCP_DEL: usize = 12;
pub const C_TCP_OTHER: usize = 13;
pub const C_TCP_CONNECTIONS: usize = 14;
pub const C_FAILED_LOGINS: usize = 15;
pub const C_HOOK_EXECUTIONS: usize = 16;
pub const C_HOOK_ERRORS: usize = 17;
pub const C_HEAVY_QUERIES: usize = 18;
pub const C_HEAVY_QUEUED: usize = 19;
pub const C_HEAVY_REJECTED: usize = 20;
pub const C_REQUESTS_ABANDONED: usize = 21;
pub const C_KV_GETS: usize = 22;
pub const C_KV_SETS: usize = 23;
pub const C_KV_DELETES: usize = 24;
pub const C_KV_HITS: usize = 25;
pub const C_KV_MISSES: usize = 26;
pub const C_KV_EXPIRED: usize = 27;
pub const C_DOC_READS: usize = 28;
pub const C_DOC_WRITES: usize = 29;
pub const C_DOC_QUERIES: usize = 30;
pub const C_CACHE_HITS: usize = 31;
pub const C_CACHE_MISSES: usize = 32;
pub const C_FLUSHES: usize = 33;
pub const C_FLUSH_NANOS: usize = 34;
pub const C_FLUSH_FAILURES: usize = 35;
pub const C_FORCED_FLUSHES: usize = 36;
pub const C_RECOVERY_FAILURES: usize = 37;
pub const NUM_COUNTERS: usize = 38;

pub const TCP_GET: &str = "GET";
pub const TCP_SET: &str = "SET";
pub const TCP_MGET: &str = "MGET";
pub const TCP_DEL: &str = "DEL";
pub const TCP_OTHER: &str = "OTHER";

pub const KIND_KV_GET: &str = "kv_get";
pub const KIND_KV_SET: &str = "kv_set";
pub const KIND_KV_DELETE: &str = "kv_delete";
pub const KIND_DOC_READ: &str = "doc_read";
pub const KIND_DOC_WRITE: &str = "doc_write";
pub const KIND_DOC_QUERY: &str = "doc_query";

pub static COUNTERS: [AtomicU64; NUM_COUNTERS] = [const { AtomicU64::new(0) }; NUM_COUNTERS];
pub static HTTP_LATENCY: Histogram = Histogram::new();
pub static TCP_LATENCY: Histogram = Histogram::new();
pub static TCP_AUTH: Histogram = Histogram::new();
pub static HEAVY_WAIT: Histogram = Histogram::new();
pub static HOOK_LATENCY: Histogram = Histogram::new();
pub static TCP_OPEN: AtomicI64 = AtomicI64::new(0);
pub static STARTED_AT: LazyLock<RwLock<Moment>> = LazyLock::new(|| RwLock::new(Moment::now()));
pub static ROUTE_STATS: RwLock<BTreeMap<Vec<u8>, Arc<RouteStat>>> = RwLock::new(BTreeMap::new());
pub static HOOK_STATS: RwLock<BTreeMap<Vec<u8>, Arc<HookStat>>> = RwLock::new(BTreeMap::new());
pub static AUTH_FAILURES: Mutex<AddressCounter> = Mutex::new(AddressCounter::new());
pub static PERSISTENCE: Mutex<PersistenceState> = Mutex::new(PersistenceState::new());
pub static RECOVERY_STATE: Mutex<TimedError> = Mutex::new(TimedError::new());
pub static HOOK_FAILURE: Mutex<TimedError> = Mutex::new(TimedError::new());
pub static REPLAYED: AtomicI64 = AtomicI64::new(0);

pub fn started_at() -> Moment {
    STARTED_AT.read().clone()
}

fn add(i: usize, n: u64) {
    COUNTERS[i].fetch_add(n, Ordering::SeqCst);
}

pub fn observe_http(
    method: impl AsRef<[u8]>,
    route: impl AsRef<[u8]>,
    status: i64,
    d: Duration,
    kind: &str,
) {
    add(C_HTTP, 1);

    if status >= 500 {
        add(C_HTTP5XX, 1);
    } else if status >= 400 {
        add(C_HTTP4XX, 1);
        if status == 401 {
            add(C_UNAUTHORIZED, 1);
        }
        if status == 403 {
            add(C_FORBIDDEN, 1);
        }
    } else if status >= 300 {
        add(C_HTTP3XX, 1);
    } else {
        add(C_HTTP2XX, 1);
    }

    match kind {
        KIND_KV_GET => add(C_KV_GETS, 1),
        KIND_KV_SET => add(C_KV_SETS, 1),
        KIND_KV_DELETE => add(C_KV_DELETES, 1),
        KIND_DOC_READ => add(C_DOC_READS, 1),
        KIND_DOC_WRITE => add(C_DOC_WRITES, 1),
        KIND_DOC_QUERY => add(C_DOC_QUERIES, 1),
        _ => {}
    }

    HTTP_LATENCY.observe(d);
    route_for(method.as_ref(), route.as_ref()).observe(status, d);
}

pub fn observe_tcp(command: &str, d: Duration) {
    add(C_TCP, 1);

    match command {
        TCP_GET => {
            add(C_TCP_GET, 1);
            add(C_KV_GETS, 1);
        }
        TCP_MGET => {
            add(C_TCP_MGET, 1);
            add(C_KV_GETS, 1);
        }
        TCP_SET => {
            add(C_TCP_SET, 1);
            add(C_KV_SETS, 1);
        }
        TCP_DEL => {
            add(C_TCP_DEL, 1);
            add(C_KV_DELETES, 1);
        }
        _ => add(C_TCP_OTHER, 1),
    }

    TCP_LATENCY.observe(d);
    if command == "AUTH" {
        TCP_AUTH.observe(d);
    }
}

pub fn heavy_query(wait: Duration) {
    add(C_HEAVY_QUERIES, 1);
    if wait > Duration::from_millis(1) {
        add(C_HEAVY_QUEUED, 1);
        HEAVY_WAIT.observe(wait);
    }
}

pub fn heavy_rejected() {
    add(C_HEAVY_REJECTED, 1);
}

pub fn request_abandoned() {
    add(C_REQUESTS_ABANDONED, 1);
}

pub fn slow_request() {
    add(C_SLOW, 1);
}

pub fn tcp_connection_opened() {
    add(C_TCP_CONNECTIONS, 1);
    TCP_OPEN.fetch_add(1, Ordering::SeqCst);
}

pub fn tcp_connection_closed() {
    TCP_OPEN.fetch_sub(1, Ordering::SeqCst);
}

pub fn failed_login(addr: impl AsRef<[u8]>) {
    add(C_FAILED_LOGINS, 1);
    AUTH_FAILURES.lock().add(addr.as_ref());
}

pub fn auth_failure(addr: impl AsRef<[u8]>) {
    AUTH_FAILURES.lock().add(addr.as_ref());
}

pub fn kv_hit() {
    add(C_KV_HITS, 1);
}

pub fn kv_miss() {
    add(C_KV_MISSES, 1);
}

pub fn kv_expired(n: i64) {
    if n > 0 {
        add(C_KV_EXPIRED, n as u64);
    }
}

pub fn cache_hit() {
    add(C_CACHE_HITS, 1);
}

pub fn cache_miss() {
    add(C_CACHE_MISSES, 1);
}

pub fn forced_flush() {
    add(C_FORCED_FLUSHES, 1);
}

pub fn recovery_write_failed(err: &dyn StdError) {
    add(C_RECOVERY_FAILURES, 1);
    RECOVERY_STATE.lock().set(err.to_string(), b"");
}

pub fn recovery_replayed(entries: i64) {
    REPLAYED.fetch_add(entries, Ordering::SeqCst);
}

pub fn observe_hook(
    id: impl AsRef<[u8]>,
    entity: impl AsRef<[u8]>,
    phase: impl AsRef<[u8]>,
    d: Duration,
    err: Option<&dyn StdError>,
) {
    add(C_HOOK_EXECUTIONS, 1);
    HOOK_LATENCY.observe(d);

    let id = id.as_ref();
    let hs = hook_for(id, entity.as_ref(), phase.as_ref());
    hs.executions.fetch_add(1, Ordering::SeqCst);
    hs.latency.observe(d);
    hs.total_nanos.fetch_add(nanos(d) as u64, Ordering::SeqCst);

    if let Some(err) = err {
        add(C_HOOK_ERRORS, 1);
        hs.errors.fetch_add(1, Ordering::SeqCst);
        {
            let mut last = hs.last.lock();
            last.error = err.to_string();
            last.at = Some(Time::now());
        }
        HOOK_FAILURE.lock().set(err.to_string(), id);
    }
}

pub fn observe_hook_query(
    id: impl AsRef<[u8]>,
    entity: impl AsRef<[u8]>,
    phase: impl AsRef<[u8]>,
    rows: i64,
    cache_hit: bool,
    d: Duration,
) {
    let hs = hook_for(id.as_ref(), entity.as_ref(), phase.as_ref());
    hs.queries.fetch_add(1, Ordering::SeqCst);
    hs.rows_read.fetch_add(rows as u64, Ordering::SeqCst);
    hs.query_nanos.fetch_add(nanos(d) as u64, Ordering::SeqCst);
    if cache_hit {
        hs.cache_hits.fetch_add(1, Ordering::SeqCst);
    }
}

pub struct PersistenceState {
    targets: BTreeMap<String, bool>,
    last_flush_at: Option<Time>,
    last_success_at: Option<Time>,
    last_duration: Duration,
    last_error: String,
    failing_since: Option<Time>,
}

impl PersistenceState {
    pub const fn new() -> PersistenceState {
        PersistenceState {
            targets: BTreeMap::new(),
            last_flush_at: None,
            last_success_at: None,
            last_duration: Duration::ZERO,
            last_error: String::new(),
            failing_since: None,
        }
    }
}

impl Default for PersistenceState {
    fn default() -> PersistenceState {
        PersistenceState::new()
    }
}

pub fn record_flush(target: &str, d: Duration, wrote: bool, err: Option<&dyn StdError>) {
    if wrote || err.is_some() {
        add(C_FLUSHES, 1);
        add(C_FLUSH_NANOS, nanos(d) as u64);
    }

    if err.is_some() {
        add(C_FLUSH_FAILURES, 1);
    }

    let mut p = PERSISTENCE.lock();

    let now = Time::now();
    p.last_flush_at = Some(now.clone());
    if wrote || err.is_some() {
        p.last_duration = d;
    }

    p.targets.insert(target.to_string(), err.is_none());
    if let Some(err) = err {
        p.last_error = err.to_string();
        if p.failing_since.is_none() {
            p.failing_since = Some(now);
        }
        return;
    }

    if p.targets.values().any(|ok| !ok) {
        return;
    }

    p.last_error = String::new();
    p.failing_since = None;
    p.last_success_at = Some(now);
}

#[derive(Clone, Debug, Default)]
pub struct PersistenceStatus {
    pub last_flush_at: Option<Time>,
    pub last_flush_ms: f64,
    pub last_flush_ok: bool,
    pub last_error: String,
    pub flush_failures: u64,
    pub last_success_at: Option<Time>,
}

pub(super) fn encode_time(t: &Time, e: &mut EncodeState) -> Result<(), JsonError> {
    let mut b = Vec::with_capacity(40);
    b.push(b'"');
    t.append_strict_rfc3339(&mut b)
        .map_err(|err| JsonError::custom(format!("cannot encode JSON: {err}")))?;
    b.push(b'"');
    e.write_raw(&b);
    Ok(())
}

pub(super) fn encode_optional_time(t: &Option<Time>, e: &mut EncodeState) -> Result<(), JsonError> {
    match t {
        Some(t) => encode_time(t, e),
        None => {
            e.write_null();
            Ok(())
        }
    }
}

impl Encode for PersistenceStatus {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "last_flush_at");
        encode_optional_time(&self.last_flush_at, e)?;
        e.write_field_name(&mut first, "last_flush_ms");
        self.last_flush_ms.encode(e)?;
        e.write_field_name(&mut first, "last_flush_ok");
        e.write_bool(self.last_flush_ok);
        e.write_field_name(&mut first, "last_flush_error");
        e.write_string(self.last_error.as_bytes());
        e.write_field_name(&mut first, "flush_failures");
        e.write_u64(self.flush_failures);
        e.write_field_name(&mut first, "last_success_at");
        encode_optional_time(&self.last_success_at, e)?;
        e.end_struct(first);
        Ok(())
    }
}

pub fn persistence() -> PersistenceStatus {
    let p = PERSISTENCE.lock();

    PersistenceStatus {
        last_flush_at: p.last_flush_at.clone(),
        last_flush_ms: round(nanos(p.last_duration) as f64 / 1_000_000.0),
        last_flush_ok: p.failing_since.is_none(),
        last_error: p.last_error.clone(),
        flush_failures: COUNTERS[C_FLUSH_FAILURES].load(Ordering::SeqCst),
        last_success_at: p.last_success_at.clone(),
    }
}

pub fn flush_failing_since() -> Option<Time> {
    PERSISTENCE.lock().failing_since.clone()
}

#[derive(Clone, Debug, Default)]
pub struct TimedError {
    pub at: Option<Moment>,
    pub message: String,
    pub key: Vec<u8>,
}

impl TimedError {
    pub const fn new() -> TimedError {
        TimedError {
            at: None,
            message: String::new(),
            key: Vec::new(),
        }
    }

    pub fn set(&mut self, message: String, key: &[u8]) {
        self.at = Some(Moment::now());
        self.message = message;
        self.key = key.to_vec();
    }

    pub fn get(&self) -> (Option<Moment>, Vec<u8>) {
        (self.at.clone(), self.key.clone())
    }
}

pub struct RouteStat {
    method: Vec<u8>,
    route: Vec<u8>,
    count: AtomicU64,
    errors: AtomicU64,
    latency: Histogram,
}

fn route_for(method: &[u8], route: &[u8]) -> Arc<RouteStat> {
    let mut key = Vec::with_capacity(method.len() + 1 + route.len());
    key.extend_from_slice(method);
    key.push(b' ');
    key.extend_from_slice(route);

    if let Some(r) = ROUTE_STATS.read().get(&key) {
        return r.clone();
    }

    ROUTE_STATS
        .write()
        .entry(key)
        .or_insert_with(|| {
            Arc::new(RouteStat {
                method: method.to_vec(),
                route: route.to_vec(),
                count: AtomicU64::new(0),
                errors: AtomicU64::new(0),
                latency: Histogram::new(),
            })
        })
        .clone()
}

impl RouteStat {
    fn observe(&self, status: i64, d: Duration) {
        self.count.fetch_add(1, Ordering::SeqCst);
        if status >= 500 {
            self.errors.fetch_add(1, Ordering::SeqCst);
        }
        self.latency.observe(d);
    }
}

#[derive(Clone, Debug, Default)]
pub struct RouteSnapshot {
    pub method: Vec<u8>,
    pub route: Vec<u8>,
    pub count: u64,
    pub errors: u64,
    pub hist: Hist,
}

pub fn snapshot_routes() -> BTreeMap<Vec<u8>, RouteSnapshot> {
    let mut out = BTreeMap::new();
    for (k, r) in ROUTE_STATS.read().iter() {
        out.insert(
            k.clone(),
            RouteSnapshot {
                method: r.method.clone(),
                route: r.route.clone(),
                count: r.count.load(Ordering::SeqCst),
                errors: r.errors.load(Ordering::SeqCst),
                hist: r.latency.snapshot(),
            },
        );
    }

    out
}

#[derive(Clone, Debug, Default)]
pub struct RouteSummary {
    pub method: Vec<u8>,
    pub route: Vec<u8>,
    pub count: u64,
    pub errors: u64,
    pub error_rate: f64,
    pub p95_ms: Option<Number>,
}

impl Encode for RouteSummary {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "method");
        e.write_string(&self.method);
        e.write_field_name(&mut first, "route");
        e.write_string(&self.route);
        e.write_field_name(&mut first, "count");
        e.write_u64(self.count);
        e.write_field_name(&mut first, "errors");
        e.write_u64(self.errors);
        e.write_field_name(&mut first, "error_rate");
        self.error_rate.encode(e)?;
        e.write_field_name(&mut first, "p95_ms");
        self.p95_ms.encode(e)?;
        e.end_struct(first);
        Ok(())
    }
}

fn route_name(r: &RouteSummary) -> Vec<u8> {
    let mut name = Vec::with_capacity(r.method.len() + r.route.len());
    name.extend_from_slice(&r.method);
    name.extend_from_slice(&r.route);
    name
}

pub fn top_routes(
    current: &BTreeMap<Vec<u8>, RouteSnapshot>,
    previous: &BTreeMap<Vec<u8>, RouteSnapshot>,
    limit: usize,
) -> Vec<RouteSummary> {
    let zero = RouteSnapshot::default();
    let mut out = Vec::new();
    for (key, cur) in current {
        let prev = previous.get(key).unwrap_or(&zero);
        let count = cur.count - prev.count.min(cur.count);
        if count == 0 {
            continue;
        }

        let errors = cur.errors - prev.errors.min(cur.errors);
        out.push(RouteSummary {
            method: cur.method.clone(),
            route: cur.route.clone(),
            count,
            errors,
            error_rate: round(errors as f64 / count as f64),
            p95_ms: num((cur.hist - prev.hist).quantile(0.95)),
        });
    }

    sort::slice(&mut out, |a, b| {
        if a.count != b.count {
            return a.count > b.count;
        }
        route_name(a) < route_name(b)
    });

    out.truncate(limit);

    out
}

#[derive(Default)]
struct HookError {
    error: String,
    at: Option<Time>,
}

pub struct HookStat {
    id: Vec<u8>,
    entity: Vec<u8>,
    phase: Vec<u8>,
    executions: AtomicU64,
    queries: AtomicU64,
    rows_read: AtomicU64,
    cache_hits: AtomicU64,
    errors: AtomicU64,
    total_nanos: AtomicU64,
    query_nanos: AtomicU64,
    latency: Histogram,
    last: Mutex<HookError>,
}

fn hook_for(id: &[u8], entity: &[u8], phase: &[u8]) -> Arc<HookStat> {
    if let Some(h) = HOOK_STATS.read().get(id) {
        return h.clone();
    }

    HOOK_STATS
        .write()
        .entry(id.to_vec())
        .or_insert_with(|| {
            Arc::new(HookStat {
                id: id.to_vec(),
                entity: entity.to_vec(),
                phase: phase.to_vec(),
                executions: AtomicU64::new(0),
                queries: AtomicU64::new(0),
                rows_read: AtomicU64::new(0),
                cache_hits: AtomicU64::new(0),
                errors: AtomicU64::new(0),
                total_nanos: AtomicU64::new(0),
                query_nanos: AtomicU64::new(0),
                latency: Histogram::new(),
                last: Mutex::new(HookError::default()),
            })
        })
        .clone()
}

#[derive(Clone, Debug, Default)]
pub struct HookSummary {
    pub hook_id: Vec<u8>,
    pub entity: Vec<u8>,
    pub phase: Vec<u8>,
    pub executions: u64,
    pub queries: u64,
    pub rows_read: u64,
    pub cache_hits: u64,
    pub errors: u64,
    pub avg_ms: Option<Number>,
    pub avg_query_ms: Option<Number>,
    pub p95_ms: Option<Number>,
    pub last_error: String,
    pub last_error_at: Option<Time>,
}

impl Encode for HookSummary {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut first = true;
        e.write_field_name(&mut first, "hook_id");
        e.write_string(&self.hook_id);
        e.write_field_name(&mut first, "entity");
        e.write_string(&self.entity);
        e.write_field_name(&mut first, "phase");
        e.write_string(&self.phase);
        e.write_field_name(&mut first, "executions");
        e.write_u64(self.executions);
        e.write_field_name(&mut first, "queries");
        e.write_u64(self.queries);
        e.write_field_name(&mut first, "rows_read");
        e.write_u64(self.rows_read);
        e.write_field_name(&mut first, "query_cache_hits");
        e.write_u64(self.cache_hits);
        e.write_field_name(&mut first, "errors");
        e.write_u64(self.errors);
        e.write_field_name(&mut first, "avg_ms");
        self.avg_ms.encode(e)?;
        if let Some(n) = &self.avg_query_ms {
            e.write_field_name(&mut first, "avg_query_ms");
            n.encode(e)?;
        }
        e.write_field_name(&mut first, "p95_ms");
        self.p95_ms.encode(e)?;
        e.write_field_name(&mut first, "last_error");
        e.write_string(self.last_error.as_bytes());
        e.write_field_name(&mut first, "last_error_at");
        encode_optional_time(&self.last_error_at, e)?;
        e.end_struct(first);
        Ok(())
    }
}

pub fn hook_summaries() -> Vec<HookSummary> {
    let hooks: Vec<Arc<HookStat>> = HOOK_STATS.read().values().cloned().collect();

    let mut out = Vec::with_capacity(hooks.len());
    for h in hooks {
        let executions = h.executions.load(Ordering::SeqCst);
        let mut summary = HookSummary {
            hook_id: h.id.clone(),
            entity: h.entity.clone(),
            phase: h.phase.clone(),
            executions,
            queries: h.queries.load(Ordering::SeqCst),
            rows_read: h.rows_read.load(Ordering::SeqCst),
            cache_hits: h.cache_hits.load(Ordering::SeqCst),
            errors: h.errors.load(Ordering::SeqCst),
            p95_ms: num(h.latency.snapshot().quantile(0.95)),
            ..HookSummary::default()
        };

        if executions > 0 {
            summary.avg_ms =
                num(h.total_nanos.load(Ordering::SeqCst) as f64 / executions as f64 / 1_000_000.0);
            if summary.queries > 0 {
                summary.avg_query_ms = num(h.query_nanos.load(Ordering::SeqCst) as f64
                    / executions as f64
                    / 1_000_000.0);
            }
        }

        {
            let last = h.last.lock();
            summary.last_error = last.error.clone();
            summary.last_error_at = last.at.clone();
        }

        out.push(summary);
    }

    sort::slice(&mut out, |a, b| a.executions > b.executions);

    out
}

pub const ADDRESS_COUNTER_MAX: usize = 10_000;

pub struct AddressCounter {
    current: BTreeMap<Vec<u8>, i64>,
    previous: BTreeMap<Vec<u8>, i64>,
    minute: i64,
}

impl AddressCounter {
    pub const fn new() -> AddressCounter {
        AddressCounter {
            current: BTreeMap::new(),
            previous: BTreeMap::new(),
            minute: 0,
        }
    }

    fn rotate(&mut self, now: &Time) {
        let minute = now.unix() / 60;
        if minute == self.minute {
            return;
        }

        if minute == self.minute.wrapping_add(1) {
            self.previous = std::mem::take(&mut self.current);
        } else {
            self.previous = BTreeMap::new();
        }

        self.current = BTreeMap::new();
        self.minute = minute;
    }

    pub fn add(&mut self, addr: &[u8]) {
        self.rotate(&Time::now());
        if !self.current.contains_key(addr) && self.current.len() >= ADDRESS_COUNTER_MAX {
            return;
        }

        *self.current.entry(addr.to_vec()).or_insert(0) += 1;
    }

    pub fn worst(&mut self) -> (Vec<u8>, i64) {
        self.rotate(&Time::now());

        let (mut worst_addr, mut worst_count): (&[u8], i64) = (b"", 0);
        for m in [&self.current, &self.previous] {
            for (addr, &count) in m {
                if count > worst_count || (count == worst_count && addr.as_slice() < worst_addr) {
                    worst_addr = addr;
                    worst_count = count;
                }
            }
        }

        (worst_addr.to_vec(), worst_count)
    }
}

impl Default for AddressCounter {
    fn default() -> AddressCounter {
        AddressCounter::new()
    }
}

pub static STARTUP_NANOS: AtomicI64 = AtomicI64::new(0);

pub fn set_startup(d: Duration) {
    STARTUP_NANOS.store(nanos(d), Ordering::SeqCst);
}

pub fn startup() -> Duration {
    Duration::from_nanos(STARTUP_NANOS.load(Ordering::SeqCst).max(0) as u64)
}

fn zero(h: &Histogram) {
    for c in &h.counts {
        c.store(0, Ordering::SeqCst);
    }
    h.sum.store(0, Ordering::SeqCst);
}

pub(super) fn reset_counters() {
    for c in &COUNTERS {
        c.store(0, Ordering::SeqCst);
    }
    zero(&HTTP_LATENCY);
    zero(&TCP_LATENCY);
    zero(&TCP_AUTH);
    zero(&HEAVY_WAIT);
    zero(&HOOK_LATENCY);
    TCP_OPEN.store(0, Ordering::SeqCst);
    *ROUTE_STATS.write() = BTreeMap::new();
    *HOOK_STATS.write() = BTreeMap::new();
    *AUTH_FAILURES.lock() = AddressCounter::new();
    *PERSISTENCE.lock() = PersistenceState::new();
    *RECOVERY_STATE.lock() = TimedError::new();
    *HOOK_FAILURE.lock() = TimedError::new();
    REPLAYED.store(0, Ordering::SeqCst);
    *STARTED_AT.write() = Moment::now();
}
