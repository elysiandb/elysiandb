use crate::gocompat::json::{Encode, EncodeState, Error as JsonError};
use crate::gocompat::strconv::append_float;

use super::counters::{
    C_CACHE_HITS, C_CACHE_MISSES, C_DOC_QUERIES, C_DOC_READS, C_DOC_WRITES, C_FAILED_LOGINS,
    C_FLUSH_FAILURES, C_FLUSH_NANOS, C_FLUSHES, C_FORBIDDEN, C_FORCED_FLUSHES, C_HEAVY_QUERIES,
    C_HEAVY_QUEUED, C_HEAVY_REJECTED, C_HOOK_ERRORS, C_HOOK_EXECUTIONS, C_HTTP, C_HTTP2XX,
    C_HTTP4XX, C_HTTP5XX, C_KV_DELETES, C_KV_EXPIRED, C_KV_GETS, C_KV_HITS, C_KV_MISSES, C_KV_SETS,
    C_REQUESTS_ABANDONED, C_SLOW, C_TCP, C_TCP_DEL, C_TCP_GET, C_TCP_MGET, C_TCP_OTHER, C_TCP_SET,
    C_UNAUTHORIZED,
};
use super::sampler::Snapshot;

#[repr(usize)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Series {
    HttpRps,
    TcpRps,
    Http2xxRps,
    Http4xxRps,
    Http5xxRps,
    TcpGetRps,
    TcpSetRps,
    TcpMgetRps,
    TcpDelRps,
    TcpOtherRps,
    HttpP50,
    HttpP95,
    HttpP99,
    TcpP50,
    TcpP95,
    TcpP99,
    SlowRps,
    UnauthorizedRps,
    ForbiddenRps,
    FailedLoginsRps,
    HookErrorsRps,
    LogWarningsRps,
    LogErrorsRps,
    KvKeys,
    KvKeysTtl,
    KvGetsRps,
    KvSetsRps,
    KvDeletesRps,
    KvHitRatio,
    KvExpiredRps,
    DocReadsRps,
    DocWritesRps,
    DocQueriesRps,
    Entities,
    IndexBacklog,
    CacheEntries,
    CacheHitRatio,
    FlushMs,
    FlushFailuresRps,
    RecoveryJsonBytes,
    RecoveryStoreBytes,
    ForcedFlushesRps,
    DiskBytes,
    AllocatedBytes,
    TotalBytes,
    Threads,
    CpuPercent,
    OpenFds,
    HttpConns,
    TcpConns,
    AdminSessions,
    OpenTransactions,
    OldestTransaction,
    HookExecutionsRps,
    HookP95,
    CpuCores,
    UsableCpus,
    LogSinceCheckpointBytes,
    LogBytes,
    LiveBytes,
    ValueCacheHitRatio,
    ResidentBytes,
    CheckpointMs,
    TcpAuthP95,
    HeavyQueriesRps,
    HeavyQueuedRps,
    HeavyRejectedRps,
    HeavyWaiting,
    HeavyWaitP95,
    AbandonedRps,
    MemoryBudget,
    MemoryLiveBytes,
    MemoryPressure,
    MemoryRefusedRps,
    ValueCacheBytes,
    PageCacheBytes,
    DirtyPageBytes,
    HookResultsBytes,
    IndexBuildBytes,
    RequestReservedBytes,
    HookQueryHitRatio,
    HookOutputHitRatio,
    IndexCount,
    IndexBytes,
    HeldBytes,
    HookFoldHitRatio,
    DiskReadBytesRps,
    DiskWriteBytesRps,
    DiskSyncsRps,
    DiskSyncMs,
    DiskFreeBytes,
    RefusedWritesRps,
    IndexUpkeepBytes,
    BackupRunning,
    BackupLastOk,
    BackupDurationMs,
    BackupBytes,
    BackupAgeSeconds,
    StartupMs,
}

pub const NUM_SERIES: usize = 99;

const _: () = assert!(Series::StartupMs as usize + 1 == NUM_SERIES);

impl Series {
    pub const fn index(self) -> usize {
        self as usize
    }

    pub const fn name(self) -> &'static str {
        SERIES_NAMES[self as usize]
    }
}

pub static SERIES_NAMES: [&str; NUM_SERIES] = [
    "http_rps",
    "tcp_rps",
    "http_2xx_rps",
    "http_4xx_rps",
    "http_5xx_rps",
    "tcp_get_rps",
    "tcp_set_rps",
    "tcp_mget_rps",
    "tcp_del_rps",
    "tcp_other_rps",
    "http_p50_ms",
    "http_p95_ms",
    "http_p99_ms",
    "tcp_p50_ms",
    "tcp_p95_ms",
    "tcp_p99_ms",
    "slow_rps",
    "unauthorized_rps",
    "forbidden_rps",
    "failed_logins_rps",
    "hook_errors_rps",
    "log_warnings_rps",
    "log_errors_rps",
    "kv_keys",
    "kv_keys_ttl",
    "kv_gets_rps",
    "kv_sets_rps",
    "kv_deletes_rps",
    "kv_hit_ratio",
    "kv_expired_rps",
    "doc_reads_rps",
    "doc_writes_rps",
    "doc_queries_rps",
    "entities",
    "index_backlog",
    "cache_entries",
    "cache_hit_ratio",
    "flush_ms",
    "flush_failures_rps",
    "recovery_json_bytes",
    "recovery_store_bytes",
    "forced_flushes_rps",
    "disk_bytes",
    "allocated_bytes",
    "total_bytes",
    "threads",
    "cpu_percent",
    "open_fds",
    "http_conns",
    "tcp_conns",
    "admin_sessions",
    "open_transactions",
    "oldest_transaction_seconds",
    "hook_executions_rps",
    "hook_p95_ms",
    "cpu_cores",
    "usable_cpus",
    "log_since_checkpoint_bytes",
    "log_bytes",
    "live_bytes",
    "value_cache_hit_ratio",
    "resident_bytes",
    "checkpoint_ms",
    "tcp_auth_p95_ms",
    "heavy_queries_rps",
    "heavy_queued_rps",
    "heavy_rejected_rps",
    "heavy_waiting",
    "heavy_wait_p95_ms",
    "abandoned_rps",
    "memory_budget_bytes",
    "memory_live_bytes",
    "memory_pressure",
    "memory_refused_rps",
    "value_cache_bytes",
    "page_cache_bytes",
    "dirty_page_bytes",
    "hook_results_bytes",
    "index_build_bytes",
    "request_reserved_bytes",
    "hook_query_hit_ratio",
    "hook_output_hit_ratio",
    "index_count",
    "index_bytes",
    "held_bytes",
    "hook_fold_hit_ratio",
    "disk_read_bytes_rps",
    "disk_write_bytes_rps",
    "disk_syncs_rps",
    "disk_sync_ms",
    "disk_free_bytes",
    "refused_writes_rps",
    "index_upkeep_bytes",
    "backup_running",
    "backup_last_ok",
    "backup_duration_ms",
    "backup_bytes",
    "backup_age_seconds",
    "startup_ms",
];

pub fn series_names() -> &'static [&'static str] {
    &SERIES_NAMES
}

#[derive(Clone, Debug, PartialEq)]
pub struct Point {
    pub t: i64,
    pub v: [f64; NUM_SERIES],
}

impl Point {
    pub const fn empty() -> Point {
        Point {
            t: 0,
            v: [0.0; NUM_SERIES],
        }
    }

    pub fn get(&self, s: Series) -> f64 {
        self.v[s.index()]
    }
}

impl Default for Point {
    fn default() -> Point {
        Point::empty()
    }
}

const RATE_SERIES: [(Series, usize); 29] = [
    (Series::HttpRps, C_HTTP),
    (Series::TcpRps, C_TCP),
    (Series::Http2xxRps, C_HTTP2XX),
    (Series::Http4xxRps, C_HTTP4XX),
    (Series::Http5xxRps, C_HTTP5XX),
    (Series::TcpGetRps, C_TCP_GET),
    (Series::TcpSetRps, C_TCP_SET),
    (Series::TcpMgetRps, C_TCP_MGET),
    (Series::TcpDelRps, C_TCP_DEL),
    (Series::TcpOtherRps, C_TCP_OTHER),
    (Series::SlowRps, C_SLOW),
    (Series::UnauthorizedRps, C_UNAUTHORIZED),
    (Series::ForbiddenRps, C_FORBIDDEN),
    (Series::FailedLoginsRps, C_FAILED_LOGINS),
    (Series::HookErrorsRps, C_HOOK_ERRORS),
    (Series::HeavyQueriesRps, C_HEAVY_QUERIES),
    (Series::HeavyQueuedRps, C_HEAVY_QUEUED),
    (Series::HeavyRejectedRps, C_HEAVY_REJECTED),
    (Series::AbandonedRps, C_REQUESTS_ABANDONED),
    (Series::KvGetsRps, C_KV_GETS),
    (Series::KvSetsRps, C_KV_SETS),
    (Series::KvDeletesRps, C_KV_DELETES),
    (Series::KvExpiredRps, C_KV_EXPIRED),
    (Series::DocReadsRps, C_DOC_READS),
    (Series::DocWritesRps, C_DOC_WRITES),
    (Series::DocQueriesRps, C_DOC_QUERIES),
    (Series::FlushFailuresRps, C_FLUSH_FAILURES),
    (Series::ForcedFlushesRps, C_FORCED_FLUSHES),
    (Series::HookExecutionsRps, C_HOOK_EXECUTIONS),
];

pub(super) fn derive(prev: &Snapshot, cur: &Snapshot) -> Point {
    let mut p = Point {
        t: cur.at.unix_milli(),
        v: [f64::NAN; NUM_SERIES],
    };

    let elapsed = cur.at.sub(&prev.at).seconds();
    if elapsed <= 0.0 {
        return p;
    }

    let delta = |c: usize| -> f64 {
        if cur.counters[c] < prev.counters[c] {
            return 0.0;
        }
        (cur.counters[c] - prev.counters[c]) as f64
    };

    for (s, c) in RATE_SERIES {
        p.v[s.index()] = delta(c) / elapsed;
    }

    if cur.log_warnings >= prev.log_warnings {
        p.v[Series::LogWarningsRps.index()] =
            (cur.log_warnings - prev.log_warnings) as f64 / elapsed;
    }
    if cur.log_errors >= prev.log_errors {
        p.v[Series::LogErrorsRps.index()] = (cur.log_errors - prev.log_errors) as f64 / elapsed;
    }

    let http_hist = cur.http - prev.http;
    p.v[Series::HttpP50.index()] = http_hist.quantile(0.50);
    p.v[Series::HttpP95.index()] = http_hist.quantile(0.95);
    p.v[Series::HttpP99.index()] = http_hist.quantile(0.99);

    let tcp_hist = cur.tcp - prev.tcp;
    p.v[Series::TcpP50.index()] = tcp_hist.quantile(0.50);
    p.v[Series::TcpP95.index()] = tcp_hist.quantile(0.95);
    p.v[Series::TcpP99.index()] = tcp_hist.quantile(0.99);

    p.v[Series::HookP95.index()] = (cur.hook - prev.hook).quantile(0.95);
    p.v[Series::TcpAuthP95.index()] = (cur.auth - prev.auth).quantile(0.95);
    p.v[Series::HeavyWaitP95.index()] = (cur.wait - prev.wait).quantile(0.95);

    let (hits, misses) = (delta(C_KV_HITS), delta(C_KV_MISSES));
    if hits + misses > 0.0 {
        p.v[Series::KvHitRatio.index()] = hits / (hits + misses);
    }

    let (hits, misses) = (delta(C_CACHE_HITS), delta(C_CACHE_MISSES));
    if hits + misses > 0.0 {
        p.v[Series::CacheHitRatio.index()] = hits / (hits + misses);
    }

    let flushes = delta(C_FLUSHES);
    if flushes > 0.0 {
        p.v[Series::FlushMs.index()] = delta(C_FLUSH_NANOS) / flushes / 1e6;
    }

    if cur.cpu_seconds >= 0.0
        && prev.cpu_seconds >= 0.0
        && cur.cpu_seconds >= prev.cpu_seconds
        && cur.procs > 0
    {
        let cores = (cur.cpu_seconds - prev.cpu_seconds) / elapsed;
        p.v[Series::CpuPercent.index()] = cores / cur.procs as f64 * 100.0;
        p.v[Series::CpuCores.index()] = cores;
    }

    p.v[Series::UsableCpus.index()] = cur.procs as f64;

    let hits = cur.value_cache_hits.wrapping_sub(prev.value_cache_hits);
    let misses = cur.value_cache_misses.wrapping_sub(prev.value_cache_misses);
    if cur.value_cache_hits >= prev.value_cache_hits
        && cur.value_cache_misses >= prev.value_cache_misses
        && hits.wrapping_add(misses) > 0
    {
        p.v[Series::ValueCacheHitRatio.index()] = hits as f64 / hits.wrapping_add(misses) as f64;
    }

    if cur.memory_refused >= prev.memory_refused {
        p.v[Series::MemoryRefusedRps.index()] =
            cur.memory_refused.wrapping_sub(prev.memory_refused) as f64 / elapsed;
    }

    if cur.disk.read_bytes >= prev.disk.read_bytes && cur.disk.write_bytes >= prev.disk.write_bytes
    {
        p.v[Series::DiskReadBytesRps.index()] =
            cur.disk.read_bytes.wrapping_sub(prev.disk.read_bytes) as f64 / elapsed;
        p.v[Series::DiskWriteBytesRps.index()] =
            cur.disk.write_bytes.wrapping_sub(prev.disk.write_bytes) as f64 / elapsed;
    }

    let done = cur.disk.syncs.wrapping_sub(prev.disk.syncs);
    if done >= 0 {
        p.v[Series::DiskSyncsRps.index()] = done as f64 / elapsed;
        if done > 0 {
            p.v[Series::DiskSyncMs.index()] =
                cur.disk.sync_nanos.wrapping_sub(prev.disk.sync_nanos) as f64 / done as f64 / 1e6;
        }
    }

    if cur.disk.refused_writes >= prev.disk.refused_writes {
        p.v[Series::RefusedWritesRps.index()] =
            cur.disk
                .refused_writes
                .wrapping_sub(prev.disk.refused_writes) as f64
                / elapsed;
    }

    p.v[Series::HookQueryHitRatio.index()] = ratio(cur.hook_query, prev.hook_query);
    p.v[Series::HookOutputHitRatio.index()] = ratio(cur.hook_output, prev.hook_output);
    p.v[Series::HookFoldHitRatio.index()] = ratio(cur.hook_fold, prev.hook_fold);

    for (i, g) in cur.gauges.iter().enumerate() {
        if let Some(v) = g {
            p.v[i] = *v;
        }
    }

    p
}

fn ratio(cur: [i64; 2], prev: [i64; 2]) -> f64 {
    let hits = cur[0].wrapping_sub(prev[0]);
    let misses = cur[1].wrapping_sub(prev[1]);
    if hits < 0 || misses < 0 || hits.wrapping_add(misses) == 0 {
        return f64::NAN;
    }

    hits as f64 / hits.wrapping_add(misses) as f64
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Number(pub f64);

impl Encode for Number {
    fn encode(&self, e: &mut EncodeState) -> Result<(), JsonError> {
        let mut b = Vec::with_capacity(24);
        append_float(&mut b, self.0, b'f', -1, 64);
        e.write_marshaled(&b)
    }
}

pub fn num(f: f64) -> Option<Number> {
    if f.is_nan() || f.is_infinite() {
        return None;
    }

    Some(Number(round(f)))
}

pub fn round(f: f64) -> f64 {
    if f.is_nan() || f.is_infinite() {
        return f;
    }

    (f * 1000.0).round() / 1000.0
}
