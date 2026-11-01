export const NOW = Date.parse("2026-11-01T12:00:00Z");

export const OVERVIEW = {
    generated_at: "2026-11-01T12:00:00Z",
    instance: {
        version: "0.2.2",
        go_version: "go1.25.3",
        engine: "internal",
        started_at: "2026-11-01T08:00:00Z",
        uptime_seconds: 14400,
        http: { enabled: true, address: "0.0.0.0:8089" },
        tcp: { enabled: false, address: "0.0.0.0:8088" },
        auth_mode: "user",
        features: { cache: true, hooks: true, schema: false, crash_recovery: true, admin_ui: true, stats: false, prometheus: false },
        data_folder: "/data",
    },
    status: {
        level: "warning",
        problems: [
            { code: "default_admin_password", severity: "warning", since: "2026-11-01T08:00:00Z", details: {} },
            { code: "hook_failing", severity: "warning", since: "2026-11-01T11:58:00Z", details: { hook_id: "7f3c" } },
        ],
    },
    traffic: {
        requests_per_second: 1284,
        http_requests_per_second: 1200,
        tcp_commands_per_second: 84,
        error_rate: 0.0002,
        p95_ms: 0.8,
        slow_requests_per_minute: 0,
    },
    data: {
        kv_keys: 120394,
        kv_keys_with_ttl: 812,
        entity_types: 12,
        entities: 48211,
        disk_bytes: 1288490188,
        top_entity_types: [{ name: "books", count: 21004 }, { name: "authors", count: 3100 }],
    },
    persistence: {
        last_flush_at: "2026-11-01T11:59:57Z",
        last_flush_ms: 12,
        last_flush_ok: true,
        last_flush_error: "",
        flush_failures: 0,
        last_success_at: "2026-11-01T11:59:57Z",
    },
    runtime: { heap_bytes: 432013312, total_bytes: 512000000, goroutines: 57, cpu_percent: 3.2 },
    connections: { http: 4, tcp: 0, admin_sessions: 2, open_transactions: 0, oldest_transaction_seconds: 0 },
    logs: { level: "info", errors_1h: 3, warnings_1h: 18, errors_24h: 11, warnings_24h: 140 },
};

export function series(range = "15m", points = 3, start = NOW - points * 1000) {
    const time = Array.from({ length: points }, (_, index) => start + (index + 1) * 1000);
    const fill = (value) => time.map(() => value);

    return {
        range,
        step_seconds: 1,
        slow_request_ms: 500,
        time,
        series: {
            http_rps: fill(1200),
            tcp_rps: fill(84),
            http_5xx_rps: fill(0),
            http_p50_ms: fill(0.3),
            http_p95_ms: fill(0.8),
            http_p99_ms: fill(1.2),
            flush_ms: fill(12),
        },
    };
}

export const LOG_ENTRIES = [
    { seq: 12, time: "2026-11-01T12:03:12.874Z", level: "ERROR", component: "http", msg: "request failed", attrs: { route: "/api/{entity}", status: 500, request_id: "7f3c9a" } },
    { seq: 11, time: "2026-11-01T12:02:57.120Z", level: "WARN", component: "hook", msg: "hook script failed", attrs: { hook_id: "7f3c", entity: "books" } },
];

export const AUDIT_ENTRIES = [
    { seq: 10, time: "2026-11-01T12:01:00.000Z", level: "INFO", component: "audit", msg: "schema changed", attrs: { actor: "alice", entity: "books" } },
];

export function logsResponse(entries, extra = {}) {
    return {
        entries,
        has_more: false,
        oldest_seq: entries.length ? entries[entries.length - 1].seq : 0,
        latest_seq: entries.length ? entries[0].seq : 0,
        size: 340,
        capacity: 5000,
        buffer_since: "2026-11-01T08:14:00Z",
        components: ["audit", "boot", "hook", "http"],
        ...extra,
    };
}
