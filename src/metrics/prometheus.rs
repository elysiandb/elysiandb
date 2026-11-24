use std::collections::BTreeSet;
use std::sync::atomic::Ordering;

use crate::gocompat::strconv;
use crate::log;

use super::counters::{
    C_CACHE_HITS, C_CACHE_MISSES, C_DOC_QUERIES, C_DOC_READS, C_DOC_WRITES, C_FAILED_LOGINS,
    C_FLUSH_FAILURES, C_FLUSHES, C_FORBIDDEN, C_FORCED_FLUSHES, C_HEAVY_QUERIES, C_HEAVY_QUEUED,
    C_HEAVY_REJECTED, C_HOOK_ERRORS, C_HOOK_EXECUTIONS, C_HTTP2XX, C_HTTP3XX, C_HTTP4XX, C_HTTP5XX,
    C_KV_DELETES, C_KV_EXPIRED, C_KV_GETS, C_KV_HITS, C_KV_MISSES, C_KV_SETS, C_RECOVERY_FAILURES,
    C_REQUESTS_ABANDONED, C_SLOW, C_TCP_CONNECTIONS, C_TCP_DEL, C_TCP_GET, C_TCP_MGET, C_TCP_OTHER,
    C_TCP_SET, C_UNAUTHORIZED, COUNTERS, HOOK_LATENCY, HTTP_LATENCY, TCP_AUTH, TCP_LATENCY,
    started_at, startup,
};
use super::histogram::{BUCKET_BOUNDS_MS, Histogram, NUM_BUCKETS};
use super::sampler::{self, Moment, Snapshot};
use super::series::Series;

struct PromCounter {
    name: &'static str,
    help: &'static str,
    counter: usize,
    labels: &'static str,
}

const fn counter(
    name: &'static str,
    help: &'static str,
    counter: usize,
    labels: &'static str,
) -> PromCounter {
    PromCounter {
        name,
        help,
        counter,
        labels,
    }
}

const PROM_COUNTERS: [PromCounter; 35] = [
    counter(
        "elysiandb_http_requests_total",
        "HTTP requests by status class.",
        C_HTTP2XX,
        "status_class=\"2xx\"",
    ),
    counter(
        "elysiandb_http_requests_total",
        "",
        C_HTTP3XX,
        "status_class=\"3xx\"",
    ),
    counter(
        "elysiandb_http_requests_total",
        "",
        C_HTTP4XX,
        "status_class=\"4xx\"",
    ),
    counter(
        "elysiandb_http_requests_total",
        "",
        C_HTTP5XX,
        "status_class=\"5xx\"",
    ),
    counter(
        "elysiandb_tcp_commands_total",
        "TCP commands by command.",
        C_TCP_GET,
        "command=\"GET\"",
    ),
    counter(
        "elysiandb_tcp_commands_total",
        "",
        C_TCP_SET,
        "command=\"SET\"",
    ),
    counter(
        "elysiandb_tcp_commands_total",
        "",
        C_TCP_MGET,
        "command=\"MGET\"",
    ),
    counter(
        "elysiandb_tcp_commands_total",
        "",
        C_TCP_DEL,
        "command=\"DEL\"",
    ),
    counter(
        "elysiandb_tcp_commands_total",
        "",
        C_TCP_OTHER,
        "command=\"OTHER\"",
    ),
    counter(
        "elysiandb_tcp_connections_total",
        "TCP connections accepted.",
        C_TCP_CONNECTIONS,
        "",
    ),
    counter(
        "elysiandb_slow_requests_total",
        "HTTP requests and TCP commands slower than log.slowRequestMs.",
        C_SLOW,
        "",
    ),
    counter(
        "elysiandb_unauthorized_requests_total",
        "HTTP requests answered 401.",
        C_UNAUTHORIZED,
        "",
    ),
    counter(
        "elysiandb_forbidden_requests_total",
        "HTTP requests answered 403.",
        C_FORBIDDEN,
        "",
    ),
    counter(
        "elysiandb_failed_logins_total",
        "Failed logins.",
        C_FAILED_LOGINS,
        "",
    ),
    counter(
        "elysiandb_hook_executions_total",
        "Hook script executions.",
        C_HOOK_EXECUTIONS,
        "",
    ),
    counter(
        "elysiandb_hook_errors_total",
        "Hook script errors.",
        C_HOOK_ERRORS,
        "",
    ),
    counter(
        "elysiandb_heavy_queries_total",
        "Requests served by the heavy lane.",
        C_HEAVY_QUERIES,
        "",
    ),
    counter(
        "elysiandb_heavy_queued_total",
        "Heavy requests that waited for a slot.",
        C_HEAVY_QUEUED,
        "",
    ),
    counter(
        "elysiandb_heavy_rejected_total",
        "Heavy requests rejected because the queue was full or too slow.",
        C_HEAVY_REJECTED,
        "",
    ),
    counter(
        "elysiandb_requests_abandoned_total",
        "Requests stopped because the client closed the connection.",
        C_REQUESTS_ABANDONED,
        "",
    ),
    counter(
        "elysiandb_kv_operations_total",
        "KV operations by kind.",
        C_KV_GETS,
        "op=\"get\"",
    ),
    counter("elysiandb_kv_operations_total", "", C_KV_SETS, "op=\"set\""),
    counter(
        "elysiandb_kv_operations_total",
        "",
        C_KV_DELETES,
        "op=\"delete\"",
    ),
    counter(
        "elysiandb_kv_lookups_total",
        "KV reads by result.",
        C_KV_HITS,
        "result=\"hit\"",
    ),
    counter(
        "elysiandb_kv_lookups_total",
        "",
        C_KV_MISSES,
        "result=\"miss\"",
    ),
    counter(
        "elysiandb_kv_expired_keys_total",
        "Keys removed by expiration.",
        C_KV_EXPIRED,
        "",
    ),
    counter(
        "elysiandb_document_operations_total",
        "REST API document operations by kind.",
        C_DOC_READS,
        "op=\"read\"",
    ),
    counter(
        "elysiandb_document_operations_total",
        "",
        C_DOC_WRITES,
        "op=\"write\"",
    ),
    counter(
        "elysiandb_document_operations_total",
        "",
        C_DOC_QUERIES,
        "op=\"query\"",
    ),
    counter(
        "elysiandb_api_cache_lookups_total",
        "API cache lookups by result.",
        C_CACHE_HITS,
        "result=\"hit\"",
    ),
    counter(
        "elysiandb_api_cache_lookups_total",
        "",
        C_CACHE_MISSES,
        "result=\"miss\"",
    ),
    counter(
        "elysiandb_flushes_total",
        "Flushes to disk that wrote data.",
        C_FLUSHES,
        "",
    ),
    counter(
        "elysiandb_flush_failures_total",
        "Failed flushes to disk.",
        C_FLUSH_FAILURES,
        "",
    ),
    counter(
        "elysiandb_forced_flushes_total",
        "Flushes forced by the recovery log size limit.",
        C_FORCED_FLUSHES,
        "",
    ),
    counter(
        "elysiandb_recovery_write_failures_total",
        "Failed recovery log writes.",
        C_RECOVERY_FAILURES,
        "",
    ),
];

const PROM_GAUGES: [(&str, &str, Series); 23] = [
    ("elysiandb_kv_keys", "Keys in the KV store.", Series::KvKeys),
    (
        "elysiandb_kv_keys_with_ttl",
        "Keys with a TTL.",
        Series::KvKeysTtl,
    ),
    (
        "elysiandb_index_backlog",
        "Fields waiting for a lazy index rebuild.",
        Series::IndexBacklog,
    ),
    (
        "elysiandb_heavy_waiting",
        "Heavy requests waiting for a slot.",
        Series::HeavyWaiting,
    ),
    (
        "elysiandb_api_cache_entries",
        "API cache entries.",
        Series::CacheEntries,
    ),
    (
        "elysiandb_recovery_log_bytes_json",
        "Size of the JSON recovery log.",
        Series::RecoveryJsonBytes,
    ),
    (
        "elysiandb_recovery_log_bytes_store",
        "Size of the store recovery log.",
        Series::RecoveryStoreBytes,
    ),
    (
        "elysiandb_data_folder_bytes",
        "Size of the data folder.",
        Series::DiskBytes,
    ),
    (
        "elysiandb_allocated_bytes",
        "Bytes allocated by the process.",
        Series::AllocatedBytes,
    ),
    (
        "elysiandb_memory_bytes",
        "Peak memory of the process.",
        Series::TotalBytes,
    ),
    (
        "elysiandb_threads",
        "Threads of the process.",
        Series::Threads,
    ),
    (
        "elysiandb_open_fds",
        "Open file descriptors.",
        Series::OpenFds,
    ),
    (
        "elysiandb_http_connections",
        "Open HTTP connections.",
        Series::HttpConns,
    ),
    (
        "elysiandb_tcp_connections",
        "Open TCP connections.",
        Series::TcpConns,
    ),
    (
        "elysiandb_admin_sessions",
        "Active Admin UI sessions.",
        Series::AdminSessions,
    ),
    (
        "elysiandb_open_transactions",
        "Open transactions.",
        Series::OpenTransactions,
    ),
    (
        "elysiandb_oldest_transaction_seconds",
        "Age of the oldest open transaction.",
        Series::OldestTransaction,
    ),
    (
        "elysiandb_disk_free_bytes",
        "Free space on the disk of the data folder.",
        Series::DiskFreeBytes,
    ),
    (
        "elysiandb_index_upkeep_bytes",
        "Index changes of large batches waiting to be applied.",
        Series::IndexUpkeepBytes,
    ),
    (
        "elysiandb_backup_running",
        "Backups running now.",
        Series::BackupRunning,
    ),
    (
        "elysiandb_backup_last_ok",
        "Whether the last backup succeeded, absent before the first one.",
        Series::BackupLastOk,
    ),
    (
        "elysiandb_backup_last_bytes",
        "Size of the last backup.",
        Series::BackupBytes,
    ),
    (
        "elysiandb_backup_age_seconds",
        "Seconds since the last backup ended.",
        Series::BackupAgeSeconds,
    ),
];

pub fn prometheus() -> Vec<u8> {
    let mut b = Vec::with_capacity(16 * 1024);

    let mut described = BTreeSet::new();
    for c in &PROM_COUNTERS {
        if described.insert(c.name) {
            write_header(&mut b, c.name, c.help, "counter");
        }
        write_sample(
            &mut b,
            c.name,
            c.labels.as_bytes(),
            COUNTERS[c.counter].load(Ordering::SeqCst) as f64,
        );
    }

    let (warnings, errors_count) = log::counts();
    write_header(
        &mut b,
        "elysiandb_log_lines_total",
        "Warning and error log lines.",
        "counter",
    );
    write_sample(
        &mut b,
        "elysiandb_log_lines_total",
        b"level=\"warn\"",
        warnings as f64,
    );
    write_sample(
        &mut b,
        "elysiandb_log_lines_total",
        b"level=\"error\"",
        errors_count as f64,
    );

    let (latest, counts) = sampler::latest_and_counts();

    if let Some(latest) = &latest {
        for (name, help, series) in PROM_GAUGES {
            let Some(v) = latest.gauge(series) else {
                continue;
            };
            if v.is_nan() {
                continue;
            }
            write_header(&mut b, name, help, "gauge");
            write_sample(&mut b, name, b"", v);
        }

        write_disk_counters(&mut b, latest);
    }

    write_header(
        &mut b,
        "elysiandb_entities",
        "Documents per entity type.",
        "gauge",
    );
    for (name, count) in counts.iter() {
        let mut labels = b"entity_type=\"".to_vec();
        labels.extend_from_slice(&escape_label(name));
        labels.push(b'"');
        write_sample(&mut b, "elysiandb_entities", &labels, *count as f64);
    }

    write_header(
        &mut b,
        "elysiandb_uptime_seconds",
        "Seconds since the server started.",
        "gauge",
    );
    write_sample(
        &mut b,
        "elysiandb_uptime_seconds",
        b"",
        Moment::now().sub(&started_at()).seconds(),
    );

    write_histogram(
        &mut b,
        "elysiandb_http_request_duration_seconds",
        "HTTP request duration.",
        &HTTP_LATENCY,
    );
    write_histogram(
        &mut b,
        "elysiandb_tcp_command_duration_seconds",
        "TCP command duration.",
        &TCP_LATENCY,
    );
    write_histogram(
        &mut b,
        "elysiandb_tcp_auth_duration_seconds",
        "TCP AUTH duration, which includes a bcrypt comparison.",
        &TCP_AUTH,
    );
    write_histogram(
        &mut b,
        "elysiandb_hook_duration_seconds",
        "Hook script duration.",
        &HOOK_LATENCY,
    );

    b
}

fn write_disk_counters(b: &mut Vec<u8>, latest: &Snapshot) {
    write_header(
        b,
        "elysiandb_disk_read_bytes_total",
        "Bytes read from the data folder.",
        "counter",
    );
    write_sample(
        b,
        "elysiandb_disk_read_bytes_total",
        b"",
        latest.disk.read_bytes as f64,
    );
    write_header(
        b,
        "elysiandb_disk_write_bytes_total",
        "Bytes written to the data folder.",
        "counter",
    );
    write_sample(
        b,
        "elysiandb_disk_write_bytes_total",
        b"",
        latest.disk.write_bytes as f64,
    );
    write_header(
        b,
        "elysiandb_disk_syncs_total",
        "Synchronizations of files and folders to the disk.",
        "counter",
    );
    write_sample(
        b,
        "elysiandb_disk_syncs_total",
        b"",
        latest.disk.syncs as f64,
    );
    write_header(
        b,
        "elysiandb_disk_sync_seconds_total",
        "Time spent in synchronizations.",
        "counter",
    );
    write_sample(
        b,
        "elysiandb_disk_sync_seconds_total",
        b"",
        latest.disk.sync_nanos as f64 / 1e9,
    );
    write_header(
        b,
        "elysiandb_refused_writes_total",
        "Writes refused because the disk is full or failing.",
        "counter",
    );
    write_sample(
        b,
        "elysiandb_refused_writes_total",
        b"",
        latest.disk.refused_writes as f64,
    );
    write_header(
        b,
        "elysiandb_backups_total",
        "Backups by result.",
        "counter",
    );
    write_sample(
        b,
        "elysiandb_backups_total",
        b"result=\"ok\"",
        latest.backup.completed as f64,
    );
    write_sample(
        b,
        "elysiandb_backups_total",
        b"result=\"failed\"",
        latest.backup.failed as f64,
    );
    write_header(
        b,
        "elysiandb_backup_last_duration_seconds",
        "Duration of the last backup.",
        "gauge",
    );
    write_sample(
        b,
        "elysiandb_backup_last_duration_seconds",
        b"",
        latest.backup.duration.as_secs_f64(),
    );
    write_header(
        b,
        "elysiandb_startup_seconds",
        "Time from the start of the process until it could serve.",
        "gauge",
    );
    write_sample(b, "elysiandb_startup_seconds", b"", startup().as_secs_f64());
}

fn write_header(b: &mut Vec<u8>, name: &str, help: &str, kind: &str) {
    if !help.is_empty() {
        b.extend_from_slice(b"# HELP ");
        b.extend_from_slice(name.as_bytes());
        b.push(b' ');
        b.extend_from_slice(help.as_bytes());
        b.push(b'\n');
    }
    b.extend_from_slice(b"# TYPE ");
    b.extend_from_slice(name.as_bytes());
    b.push(b' ');
    b.extend_from_slice(kind.as_bytes());
    b.push(b'\n');
}

fn write_sample(b: &mut Vec<u8>, name: &str, labels: &[u8], v: f64) {
    b.extend_from_slice(name.as_bytes());
    if !labels.is_empty() {
        b.push(b'{');
        b.extend_from_slice(labels);
        b.push(b'}');
    }
    b.push(b' ');
    strconv::append_float(b, v, b'g', -1, 64);
    b.push(b'\n');
}

fn write_histogram(b: &mut Vec<u8>, name: &str, help: &str, source: &Histogram) {
    write_header(b, name, help, "histogram");

    let bucket = format!("{name}_bucket");
    let h = source.snapshot();
    let mut cumulative: u64 = 0;
    for (i, bound) in BUCKET_BOUNDS_MS.iter().enumerate() {
        cumulative = cumulative.wrapping_add(h.0[i]);
        let mut labels = b"le=\"".to_vec();
        strconv::append_float(&mut labels, bound / 1000.0, b'g', -1, 64);
        labels.push(b'"');
        write_sample(b, &bucket, &labels, cumulative as f64);
    }

    cumulative = cumulative.wrapping_add(h.0[NUM_BUCKETS - 1]);
    write_sample(b, &bucket, b"le=\"+Inf\"", cumulative as f64);
    write_sample(b, &format!("{name}_sum"), b"", source.sum_seconds());
    write_sample(b, &format!("{name}_count"), b"", cumulative as f64);
}

fn escape_label(s: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(s.len());
    for &c in s {
        match c {
            b'\\' => out.extend_from_slice(b"\\\\"),
            b'"' => out.extend_from_slice(b"\\\""),
            b'\n' => out.extend_from_slice(b"\\n"),
            _ => out.push(c),
        }
    }

    out
}
