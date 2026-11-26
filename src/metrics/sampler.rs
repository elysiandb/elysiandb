use std::collections::{BTreeMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::{Arc, LazyLock};
use std::thread::JoinHandle;
use std::time::{Duration as StdDuration, Instant};

use crate::globals;
use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::strconv;
use crate::gocompat::time::{Duration, Time};
use crate::log;
use crate::memory;
use crate::sync::{Mutex, RwLock, Stop, Ticker, spawn};

#[cfg(not(unix))]
use super::cpu_other::{peak_resident_bytes, process_cpu_seconds};
#[cfg(unix)]
use super::cpu_unix::{peak_resident_bytes, process_cpu_seconds};

use super::counters::{
    self, COUNTERS, HEAVY_WAIT, HOOK_LATENCY, HTTP_LATENCY, NUM_COUNTERS, RouteSnapshot, TCP_AUTH,
    TCP_LATENCY, TCP_OPEN,
};
use super::histogram::{Hist, nanos};
use super::overview::IndexState;
use super::series::{NUM_SERIES, Point, SERIES_NAMES, Series, derive, round};
use super::status;

#[derive(Clone, Debug)]
pub struct Moment {
    pub wall: Time,
    pub mono: Option<Instant>,
}

impl Moment {
    pub fn now() -> Moment {
        Moment {
            wall: Time::now(),
            mono: Some(Instant::now()),
        }
    }

    pub fn from_wall(wall: Time) -> Moment {
        Moment { wall, mono: None }
    }

    pub fn add(&self, d: Duration) -> Moment {
        let mono = self.mono.and_then(|m| {
            if d.0 >= 0 {
                m.checked_add(StdDuration::from_nanos(d.0 as u64))
            } else {
                m.checked_sub(StdDuration::from_nanos(d.0.unsigned_abs()))
            }
        });

        Moment {
            wall: self.wall.add(d),
            mono,
        }
    }

    pub fn sub(&self, u: &Moment) -> Duration {
        if let (Some(t), Some(u)) = (self.mono, u.mono) {
            if t >= u {
                return Duration(i64::try_from((t - u).as_nanos()).unwrap_or(i64::MAX));
            }
            return Duration(
                i64::try_from((u - t).as_nanos())
                    .map(i64::wrapping_neg)
                    .unwrap_or(i64::MIN),
            );
        }

        self.wall.sub(&u.wall)
    }

    pub fn before(&self, u: &Moment) -> bool {
        if let (Some(t), Some(u)) = (self.mono, u.mono) {
            return t < u;
        }

        self.wall.before(&u.wall)
    }

    pub fn unix_milli(&self) -> i64 {
        self.wall.unix_milli()
    }
}

pub type Source<T> = Option<Arc<dyn Fn() -> T + Send + Sync>>;

#[derive(Clone, Default)]
pub struct Sources {
    pub data_folder: Source<String>,
    pub kv_keys: Source<(u64, u64)>,
    pub entity_counts: Source<BTreeMap<Vec<u8>, i64>>,
    pub index_backlog: Source<i64>,
    pub cache_entries: Source<i64>,
    pub recovery_sizes: Source<(i64, i64)>,
    pub admin_sessions: Source<i64>,
    pub transactions: Source<(i64, StdDuration)>,
    pub http_connections: Source<i64>,
    pub default_admin_password: Source<bool>,
    pub engine: Source<EngineSample>,
    pub hook_caches: Source<HookCacheSample>,
    pub heavy_waiting: Source<i64>,
    pub indexes: Source<Vec<IndexState>>,
    pub disk: Source<DiskSample>,
    pub backup: Source<BackupSample>,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DiskSample {
    pub read_bytes: i64,
    pub write_bytes: i64,
    pub syncs: i64,
    pub sync_nanos: i64,
    pub free_bytes: i64,
    pub refused_writes: i64,
    pub index_upkeep_bytes: i64,
}

#[derive(Clone, Debug, Default)]
pub struct BackupSample {
    pub running: i64,
    pub completed: i64,
    pub failed: i64,
    pub last_ok: bool,
    pub last_at: Option<Moment>,
    pub duration: StdDuration,
    pub bytes: i64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct EngineSample {
    pub log_since_checkpoint: i64,
    pub log_bytes: i64,
    pub live_bytes: i64,
    pub value_cache_hits: i64,
    pub value_cache_misses: i64,
    pub resident_bytes: i64,
    pub value_cache_bytes: i64,
    pub dirty_page_bytes: i64,
    pub index_count: i64,
    pub index_bytes: i64,
    pub checkpoint_ms: f64,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct HookCacheSample {
    pub query_hits: i64,
    pub query_misses: i64,
    pub output_hits: i64,
    pub output_misses: i64,
    pub fold_hits: i64,
    pub fold_passes: i64,
}

struct Resolution {
    name: &'static str,
    step: Duration,
    points: Vec<Point>,
    next: usize,
    size: usize,
    prev: Option<Arc<Snapshot>>,
}

impl Resolution {
    fn new(name: &'static str, step: Duration, capacity: usize) -> Resolution {
        Resolution {
            name,
            step,
            points: vec![Point::empty(); capacity],
            next: 0,
            size: 0,
            prev: None,
        }
    }

    fn push(&mut self, p: Point) {
        self.points[self.next] = p;
        self.next = (self.next + 1) % self.points.len();
        if self.size < self.points.len() {
            self.size += 1;
        }
    }

    fn ordered(&self) -> Vec<Point> {
        let len = self.points.len();
        let start = (self.next + len - self.size) % len;
        let mut out = Vec::with_capacity(self.size);
        for i in 0..self.size {
            out.push(self.points[(start + i) % len].clone());
        }

        out
    }
}

pub(super) struct Snapshot {
    pub(super) at: Moment,
    pub(super) counters: [u64; NUM_COUNTERS],
    pub(super) http: Hist,
    pub(super) tcp: Hist,
    pub(super) auth: Hist,
    pub(super) wait: Hist,
    pub(super) hook: Hist,
    pub(super) log_warnings: u64,
    pub(super) log_errors: u64,
    pub(super) cpu_seconds: f64,
    pub(super) procs: i64,
    pub(super) gauges: [Option<f64>; NUM_SERIES],
    pub(super) value_cache_hits: i64,
    pub(super) value_cache_misses: i64,
    pub(super) memory_refused: i64,
    pub(super) hook_query: [i64; 2],
    pub(super) hook_output: [i64; 2],
    pub(super) hook_fold: [i64; 2],
    pub(super) disk: DiskSample,
    pub(super) backup: BackupSample,
}

impl Snapshot {
    pub(super) fn gauge(&self, s: Series) -> Option<f64> {
        self.gauges[s.index()]
    }

    pub(super) fn gauge_or_zero(&self, s: Series) -> f64 {
        self.gauge(s).unwrap_or(0.0)
    }

    fn set(&mut self, s: Series, v: f64) {
        self.gauges[s.index()] = Some(v);
    }
}

#[derive(Clone)]
pub(super) struct SlowValues {
    pub(super) entity_counts: EntityCounts,
    recovery_json: f64,
    recovery_store: f64,
    open_fds: f64,
    admin_sessions: f64,
    disk_bytes: f64,
    pub(super) default_admin_pw: bool,
}

struct Sampler {
    sources: Sources,
    resolutions: [Resolution; 3],
    recent: VecDeque<Arc<Snapshot>>,
    route_marks: VecDeque<BTreeMap<Vec<u8>, RouteSnapshot>>,
    slow: SlowValues,
    ticks: i64,
    backlog_from: Option<Moment>,
}

const RECENT_SNAPSHOTS: usize = 301;
const ROUTE_MARKS: usize = 16;

impl Sampler {
    fn new() -> Sampler {
        Sampler {
            sources: Sources::default(),
            resolutions: [
                Resolution::new("15m", Duration::SECOND, 900),
                Resolution::new("1h", Duration::SECOND * 10, 1080),
                Resolution::new("24h", Duration::MINUTE, 1440),
            ],
            recent: VecDeque::new(),
            route_marks: VecDeque::new(),
            slow: SlowValues {
                entity_counts: Arc::new(BTreeMap::new()),
                recovery_json: f64::NAN,
                recovery_store: f64::NAN,
                open_fds: f64::NAN,
                admin_sessions: f64::NAN,
                disk_bytes: f64::NAN,
                default_admin_pw: false,
            },
            ticks: 0,
            backlog_from: None,
        }
    }
}

static STATE: LazyLock<RwLock<Sampler>> = LazyLock::new(|| RwLock::new(Sampler::new()));

struct Runner {
    stop: Arc<Stop>,
    handle: JoinHandle<()>,
}

static RUNNER: Mutex<Option<Runner>> = Mutex::new(None);

pub fn set_sources(s: Sources) {
    STATE.write().sources = s;
}

pub fn start() {
    let mut runner = RUNNER.lock();
    if runner.is_some() {
        return;
    }

    tick(Moment::now());

    let stop = Arc::new(Stop::new());
    let signal = stop.clone();
    let handle = spawn("metrics-sampler", move || {
        let mut ticker = Ticker::new(StdDuration::from_secs(1));
        while ticker.wait(&signal).is_some() {
            tick(Moment::now());
        }
    });

    *runner = Some(Runner { stop, handle });
}

pub fn stop() {
    let Some(runner) = RUNNER.lock().take() else {
        return;
    };

    runner.stop.stop();
    let _ = runner.handle.join();
}

pub fn reset() {
    stop();
    *STATE.write() = Sampler::new();
    status::reset_problems();
    counters::reset_counters();
}

pub fn tick(now: Moment) {
    let (sources, ticks, slow) = {
        let s = STATE.read();
        (s.sources.clone(), s.ticks, s.slow.clone())
    };

    let slow = refresh_slow(&sources, ticks, slow);
    let cur = Arc::new(take_snapshot(&now, &sources, &slow));

    {
        let mut s = STATE.write();
        s.slow = slow;
        s.ticks += 1;

        for r in &mut s.resolutions {
            let Some(prev) = r.prev.clone() else {
                r.prev = Some(cur.clone());
                continue;
            };

            if now.sub(&prev.at) >= r.step - Duration(r.step.0 / 10) {
                r.push(derive(&prev, &cur));
                r.prev = Some(cur.clone());
            }
        }

        s.recent.push_back(cur.clone());
        while s.recent.len() > RECENT_SNAPSHOTS {
            s.recent.pop_front();
        }

        if ticks % 60 == 0 {
            s.route_marks.push_back(counters::snapshot_routes());
            while s.route_marks.len() > ROUTE_MARKS {
                s.route_marks.pop_front();
            }
        }

        if cur.gauge_or_zero(Series::IndexBacklog) > 0.0 {
            if s.backlog_from.is_none() {
                s.backlog_from = Some(now.clone());
            }
        } else {
            s.backlog_from = None;
        }
    }

    let found = status::evaluate(&now);
    status::update(&now, found);
}

fn refresh_slow(src: &Sources, ticks: i64, mut slow: SlowValues) -> SlowValues {
    if ticks % 10 == 0 {
        if let Some(f) = &src.entity_counts {
            slow.entity_counts = Arc::new(f());
        }
        if let Some(f) = &src.recovery_sizes {
            let (json, store) = f();
            slow.recovery_json = json as f64;
            slow.recovery_store = store as f64;
        }
        if let Some(f) = &src.admin_sessions {
            slow.admin_sessions = f() as f64;
        }
        slow.open_fds = open_file_descriptors();
    }

    if ticks % 60 == 0 {
        if let Some(f) = &src.data_folder {
            slow.disk_bytes = folder_size(&f());
        }
        if let Some(f) = &src.default_admin_password {
            slow.default_admin_pw = f();
        }
    }

    slow
}

fn take_snapshot(now: &Moment, src: &Sources, slow: &SlowValues) -> Snapshot {
    let mut s = Snapshot {
        at: now.clone(),
        counters: [0; NUM_COUNTERS],
        http: HTTP_LATENCY.snapshot(),
        tcp: TCP_LATENCY.snapshot(),
        auth: TCP_AUTH.snapshot(),
        wait: HEAVY_WAIT.snapshot(),
        hook: HOOK_LATENCY.snapshot(),
        log_warnings: 0,
        log_errors: 0,
        cpu_seconds: 0.0,
        procs: i64::try_from(globals::procs()).unwrap_or(i64::MAX),
        gauges: [None; NUM_SERIES],
        value_cache_hits: 0,
        value_cache_misses: 0,
        memory_refused: 0,
        hook_query: [0; 2],
        hook_output: [0; 2],
        hook_fold: [0; 2],
        disk: DiskSample::default(),
        backup: BackupSample::default(),
    };

    for (i, c) in COUNTERS.iter().enumerate() {
        s.counters[i] = c.load(Ordering::SeqCst);
    }

    (s.log_warnings, s.log_errors) = log::counts();
    s.cpu_seconds = process_cpu_seconds();

    s.set(Series::AllocatedBytes, memory::allocated() as f64);
    s.set(Series::TotalBytes, peak_resident_bytes());
    s.set(Series::Threads, thread_count());

    if let Some(f) = &src.kv_keys {
        let (keys, ttl) = f();
        s.set(Series::KvKeys, keys as f64);
        s.set(Series::KvKeysTtl, ttl as f64);
    }

    let mut total: i64 = 0;
    for c in slow.entity_counts.values() {
        total = total.wrapping_add(*c);
    }
    s.set(Series::Entities, total as f64);

    if let Some(f) = &src.index_backlog {
        s.set(Series::IndexBacklog, f() as f64);
    }
    if let Some(f) = &src.cache_entries {
        s.set(Series::CacheEntries, f() as f64);
    }
    if let Some(f) = &src.http_connections {
        s.set(Series::HttpConns, f() as f64);
    }
    if let Some(f) = &src.transactions {
        let (open, oldest) = f();
        s.set(Series::OpenTransactions, open as f64);
        s.set(Series::OldestTransaction, oldest.as_secs_f64());
    }

    if let Some(f) = &src.heavy_waiting {
        s.set(Series::HeavyWaiting, f() as f64);
    }

    if let Some(f) = &src.engine {
        let eng = f();
        s.set(
            Series::LogSinceCheckpointBytes,
            eng.log_since_checkpoint as f64,
        );
        s.set(Series::LogBytes, eng.log_bytes as f64);
        s.set(Series::LiveBytes, eng.live_bytes as f64);
        s.set(Series::ResidentBytes, eng.resident_bytes as f64);
        s.set(Series::CheckpointMs, eng.checkpoint_ms);
        s.set(Series::ValueCacheBytes, eng.value_cache_bytes as f64);
        s.set(Series::PageCacheBytes, eng.resident_bytes as f64);
        s.set(Series::DirtyPageBytes, eng.dirty_page_bytes as f64);
        s.set(Series::IndexCount, eng.index_count as f64);
        s.set(Series::IndexBytes, eng.index_bytes as f64);
        s.value_cache_hits = eng.value_cache_hits;
        s.value_cache_misses = eng.value_cache_misses;
    }

    if let Some(f) = &src.hook_caches {
        let hc = f();
        s.hook_query = [hc.query_hits, hc.query_misses];
        s.hook_output = [hc.output_hits, hc.output_misses];
        s.hook_fold = [hc.fold_hits, hc.fold_passes];
    }

    if let Some(f) = &src.disk {
        s.disk = f();
        s.set(Series::DiskFreeBytes, s.disk.free_bytes as f64);
        s.set(Series::IndexUpkeepBytes, s.disk.index_upkeep_bytes as f64);
    }

    if let Some(f) = &src.backup {
        s.backup = f();
        s.set(Series::BackupRunning, s.backup.running as f64);
        if let Some(last_at) = s.backup.last_at.clone() {
            s.set(
                Series::BackupLastOk,
                if s.backup.last_ok { 1.0 } else { 0.0 },
            );
            s.set(
                Series::BackupDurationMs,
                nanos(s.backup.duration) as f64 / 1_000_000.0,
            );
            s.set(Series::BackupBytes, s.backup.bytes as f64);
            s.set(Series::BackupAgeSeconds, now.sub(&last_at).seconds());
        }
    }

    let took = counters::startup();
    if !took.is_zero() {
        s.set(Series::StartupMs, nanos(took) as f64 / 1_000_000.0);
    }

    s.set(Series::HeldBytes, memory::resident() as f64);
    s.set(Series::MemoryBudget, memory::budget() as f64);
    s.set(Series::MemoryLiveBytes, memory::live() as f64);
    s.set(Series::MemoryPressure, memory::current() as i32 as f64);
    s.set(Series::HookResultsBytes, memory::HOOK_RESULTS.used() as f64);
    s.set(Series::IndexBuildBytes, memory::BUILDS.used() as f64);
    s.set(Series::RequestReservedBytes, memory::REQUESTS.used() as f64);
    for p in memory::pools() {
        s.memory_refused = s.memory_refused.wrapping_add(p.refused());
    }

    s.set(Series::TcpConns, TCP_OPEN.load(Ordering::SeqCst) as f64);
    s.set(Series::RecoveryJsonBytes, slow.recovery_json);
    s.set(Series::RecoveryStoreBytes, slow.recovery_store);
    s.set(Series::OpenFds, slow.open_fds);
    s.set(Series::AdminSessions, slow.admin_sessions);
    s.set(Series::DiskBytes, slow.disk_bytes);

    s
}

#[cfg(target_os = "linux")]
fn thread_count() -> f64 {
    let Ok(status) = std::fs::read("/proc/self/status") else {
        return f64::NAN;
    };

    for line in status.split(|&b| b == b'\n') {
        let Some(rest) = line.strip_prefix(b"Threads:") else {
            continue;
        };
        let digits = rest.trim_ascii();
        if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
            return f64::NAN;
        }
        let mut n: f64 = 0.0;
        for &d in digits {
            n = n * 10.0 + f64::from(d - b'0');
        }
        return n;
    }

    f64::NAN
}

#[cfg(target_os = "macos")]
fn thread_count() -> f64 {
    let mut info = std::mem::MaybeUninit::<libc::proc_taskinfo>::zeroed();
    let size = std::mem::size_of::<libc::proc_taskinfo>() as libc::c_int;
    let n = unsafe {
        libc::proc_pidinfo(
            libc::getpid(),
            libc::PROC_PIDTASKINFO,
            0,
            info.as_mut_ptr().cast::<libc::c_void>(),
            size,
        )
    };
    if n != size {
        return f64::NAN;
    }

    f64::from(unsafe { info.assume_init() }.pti_threadnum)
}

#[cfg(not(any(target_os = "linux", target_os = "macos")))]
fn thread_count() -> f64 {
    f64::NAN
}

fn open_file_descriptors() -> f64 {
    for folder in ["/proc/self/fd", "/dev/fd"] {
        let Ok(entries) = std::fs::read_dir(folder) else {
            continue;
        };

        let mut n: usize = 0;
        let mut ok = true;
        for entry in entries {
            if entry.is_err() {
                ok = false;
                break;
            }
            n += 1;
        }

        if ok {
            return n as f64;
        }
    }

    f64::NAN
}

fn folder_size(folder: &str) -> f64 {
    if folder.is_empty() {
        return f64::NAN;
    }

    let root = Path::new(folder);
    let Ok(meta) = std::fs::symlink_metadata(root) else {
        return 0.0;
    };

    let mut total: i64 = 0;
    if meta.file_type().is_file() {
        total = total.wrapping_add(meta.len() as i64);
    }

    let mut stack: Vec<PathBuf> = Vec::new();
    if meta.file_type().is_dir() {
        stack.push(root.to_path_buf());
    }

    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };

        for entry in entries {
            let Ok(entry) = entry else {
                continue;
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };

            if kind.is_dir() {
                stack.push(entry.path());
            } else if kind.is_file()
                && let Ok(info) = entry.metadata()
            {
                total = total.wrapping_add(info.len() as i64);
            }
        }
    }

    total as f64
}

#[derive(Clone, Debug, Default)]
pub struct SeriesResponse {
    pub range: String,
    pub step_seconds: i64,
    pub slow_request_ms: i64,
    pub points: Vec<Point>,
}

pub fn series(range_name: &str, since: i64) -> (SeriesResponse, bool) {
    let state = STATE.read();

    for r in &state.resolutions {
        if r.name != range_name {
            continue;
        }

        let mut points = r.ordered();
        if since > 0 {
            let i = points.partition_point(|p| p.t <= since);
            points.drain(..i);
        }

        return (
            SeriesResponse {
                range: r.name.to_string(),
                step_seconds: r.step.0 / Duration::SECOND.0,
                slow_request_ms: 0,
                points,
            },
            true,
        );
    }

    (SeriesResponse::default(), false)
}

impl SeriesResponse {
    pub fn marshal_json(&self) -> Vec<u8> {
        let mut b = Vec::with_capacity(64 + self.points.len() * NUM_SERIES * 8);
        b.extend_from_slice(b"{\"range\":");
        strconv::append_quote(&mut b, self.range.as_bytes());
        b.extend_from_slice(b",\"step_seconds\":");
        strconv::append_int(&mut b, self.step_seconds, 10);
        b.extend_from_slice(b",\"slow_request_ms\":");
        strconv::append_int(&mut b, self.slow_request_ms, 10);
        b.extend_from_slice(b",\"time\":[");
        for (i, p) in self.points.iter().enumerate() {
            if i > 0 {
                b.push(b',');
            }
            strconv::append_int(&mut b, p.t, 10);
        }
        b.extend_from_slice(b"],\"series\":{");
        for (si, name) in SERIES_NAMES.iter().enumerate() {
            if si > 0 {
                b.push(b',');
            }
            strconv::append_quote(&mut b, name.as_bytes());
            b.extend_from_slice(b":[");
            for (i, p) in self.points.iter().enumerate() {
                if i > 0 {
                    b.push(b',');
                }
                append_number(&mut b, p.v[si]);
            }
            b.push(b']');
        }
        b.extend_from_slice(b"}}");

        b
    }
}

impl Encode for SeriesResponse {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        e.write_marshaled(&self.marshal_json())
    }
}

fn append_number(b: &mut Vec<u8>, v: f64) {
    if v.is_nan() || v.is_infinite() {
        b.extend_from_slice(b"null");
        return;
    }

    strconv::append_float(b, round(v), b'f', -1, 64);
}

fn window(recent: &VecDeque<Arc<Snapshot>>, d: Duration) -> Option<(Arc<Snapshot>, Arc<Snapshot>)> {
    if recent.len() < 2 {
        return None;
    }

    let cur = recent.back()?.clone();
    let target = cur.at.add(-d);
    for snap in recent {
        if !snap.at.before(&target) {
            if Arc::ptr_eq(snap, &cur) {
                return None;
            }
            return Some((snap.clone(), cur));
        }
    }

    Some((recent[0].clone(), cur))
}

pub(super) struct Evaluation {
    pub(super) slow: SlowValues,
    pub(super) backlog_from: Option<Moment>,
    pub(super) window: Option<(Arc<Snapshot>, Arc<Snapshot>)>,
}

pub(super) fn evaluation(d: Duration) -> Evaluation {
    let s = STATE.read();

    Evaluation {
        slow: s.slow.clone(),
        backlog_from: s.backlog_from.clone(),
        window: window(&s.recent, d),
    }
}

pub(super) struct OverviewState {
    pub(super) slow: SlowValues,
    pub(super) traffic: Option<(Arc<Snapshot>, Arc<Snapshot>)>,
    pub(super) cpu: Option<(Arc<Snapshot>, Arc<Snapshot>)>,
    pub(super) latest: Option<Arc<Snapshot>>,
    pub(super) ten_seconds: Vec<Point>,
    pub(super) minutes: Vec<Point>,
}

pub(super) fn overview_state() -> OverviewState {
    let s = STATE.read();

    OverviewState {
        slow: s.slow.clone(),
        traffic: window(&s.recent, Duration::MINUTE),
        cpu: window(&s.recent, Duration::SECOND * 10),
        latest: s.recent.back().cloned(),
        ten_seconds: s.resolutions[1].ordered(),
        minutes: s.resolutions[2].ordered(),
    }
}

pub(super) struct DetailsState {
    pub(super) baseline: Option<BTreeMap<Vec<u8>, RouteSnapshot>>,
    pub(super) counts: EntityCounts,
    pub(super) indexes: Source<Vec<IndexState>>,
}

pub(super) fn details_state() -> DetailsState {
    let s = STATE.read();

    DetailsState {
        baseline: s.route_marks.front().cloned(),
        counts: s.slow.entity_counts.clone(),
        indexes: s.sources.indexes.clone(),
    }
}

pub(super) type EntityCounts = Arc<BTreeMap<Vec<u8>, i64>>;

pub(super) fn latest_and_counts() -> (Option<Arc<Snapshot>>, EntityCounts) {
    let s = STATE.read();

    (s.recent.back().cloned(), s.slow.entity_counts.clone())
}
