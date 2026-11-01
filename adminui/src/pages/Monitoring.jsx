import { useMemo } from "react";
import { Alert, Button, ButtonGroup, Card, Col, Container, Row, Spinner, Table } from "react-bootstrap";
import { useSearchParams } from "react-router-dom";
import { apiFetch } from "../utils/api";
import { RANGES, seriesFetcher, values } from "../utils/series.js";
import { useTranslation } from "../hooks/i18n/useTranslation.js";
import { useFormatters } from "../hooks/i18n/useFormatters.js";
import { usePolling } from "../hooks/monitoring/usePolling.js";
import { useStoredInterval } from "../hooks/monitoring/useStoredInterval.js";
import RefreshControl, { ConnectionLostAlert } from "../components/monitoring/RefreshControl.jsx";
import RelativeTime from "../components/monitoring/RelativeTime.jsx";
import ChartCard from "../components/monitoring/ChartCard.jsx";

const CHART_HEIGHT = 170;
const SYNC_KEY = "monitoring";

const RANGE_OPTIONS = [["15m", 15], ["1h", 60], ["24h", 1440]];

const SECTIONS = [
    {
        id: "traffic",
        title: "monitoring.sections.traffic",
        extra: "routes",
        charts: [
            { title: "monitoring.charts.requests", unit: "rate", series: [
                { name: "http_rps", label: "monitoring.series.http", color: "accent" },
                { name: "tcp_rps", label: "monitoring.series.tcp", color: "cyan" },
            ] },
            { title: "monitoring.charts.httpStatus", unit: "rate", series: [
                { name: "http_2xx_rps", label: "monitoring.series.status2xx", color: "green" },
                { name: "http_4xx_rps", label: "monitoring.series.status4xx", color: "orange" },
                { name: "http_5xx_rps", label: "monitoring.series.status5xx", color: "red" },
            ] },
            { title: "monitoring.charts.tcpCommands", unit: "rate", series: [
                { name: "tcp_get_rps", label: "monitoring.series.get", color: "accent" },
                { name: "tcp_set_rps", label: "monitoring.series.set", color: "cyan" },
                { name: "tcp_mget_rps", label: "monitoring.series.mget", color: "green" },
                { name: "tcp_del_rps", label: "monitoring.series.del", color: "red" },
                { name: "tcp_other_rps", label: "monitoring.series.other", color: "purple" },
            ] },
        ],
    },
    {
        id: "latency",
        title: "monitoring.sections.latency",
        charts: [
            { title: "monitoring.charts.httpLatency", unit: "ms", threshold: true, series: [
                { name: "http_p50_ms", label: "monitoring.series.p50", color: "green" },
                { name: "http_p95_ms", label: "monitoring.series.p95", color: "accent" },
                { name: "http_p99_ms", label: "monitoring.series.p99", color: "orange" },
            ] },
            { title: "monitoring.charts.tcpLatency", unit: "ms", threshold: true, series: [
                { name: "tcp_p50_ms", label: "monitoring.series.p50", color: "green" },
                { name: "tcp_p95_ms", label: "monitoring.series.p95", color: "accent" },
                { name: "tcp_p99_ms", label: "monitoring.series.p99", color: "orange" },
            ] },
            { title: "monitoring.charts.slowRequests", unit: "perMinute", scale: 60, series: [
                { name: "slow_rps", label: "monitoring.series.slow", color: "red" },
            ] },
        ],
    },
    {
        id: "errors",
        title: "monitoring.sections.errors",
        charts: [
            { title: "monitoring.charts.serverErrors", unit: "rate", series: [
                { name: "http_5xx_rps", label: "monitoring.series.status5xx", color: "red" },
            ] },
            { title: "monitoring.charts.authFailures", unit: "perMinute", scale: 60, series: [
                { name: "unauthorized_rps", label: "monitoring.series.unauthorized", color: "orange" },
                { name: "forbidden_rps", label: "monitoring.series.forbidden", color: "purple" },
                { name: "failed_logins_rps", label: "monitoring.series.failedLogins", color: "red" },
            ] },
            { title: "monitoring.charts.hookErrors", unit: "perMinute", scale: 60, series: [
                { name: "hook_errors_rps", label: "monitoring.series.hookErrors", color: "red" },
            ] },
            { title: "monitoring.charts.logLines", unit: "perMinute", scale: 60, series: [
                { name: "log_warnings_rps", label: "monitoring.series.warnings", color: "accent" },
                { name: "log_errors_rps", label: "monitoring.series.errors", color: "red" },
            ] },
        ],
    },
    {
        id: "kv",
        title: "monitoring.sections.kv",
        charts: [
            { title: "monitoring.charts.kvKeys", unit: "count", series: [
                { name: "kv_keys", label: "monitoring.series.keys", color: "accent" },
                { name: "kv_keys_ttl", label: "monitoring.series.keysWithTtl", color: "cyan" },
            ] },
            { title: "monitoring.charts.kvOperations", unit: "rate", series: [
                { name: "kv_gets_rps", label: "monitoring.series.gets", color: "accent" },
                { name: "kv_sets_rps", label: "monitoring.series.sets", color: "cyan" },
                { name: "kv_deletes_rps", label: "monitoring.series.deletes", color: "red" },
                { name: "kv_expired_rps", label: "monitoring.series.expired", color: "purple" },
            ] },
            { title: "monitoring.charts.kvHitRatio", unit: "percent", series: [
                { name: "kv_hit_ratio", label: "monitoring.series.hitRatio", color: "green" },
            ] },
        ],
    },
    {
        id: "documents",
        title: "monitoring.sections.documents",
        extra: "entities",
        charts: [
            { title: "monitoring.charts.docOperations", unit: "rate", series: [
                { name: "doc_reads_rps", label: "monitoring.series.reads", color: "accent" },
                { name: "doc_writes_rps", label: "monitoring.series.writes", color: "cyan" },
                { name: "doc_queries_rps", label: "monitoring.series.queries", color: "green" },
            ] },
            { title: "monitoring.charts.entities", unit: "count", series: [
                { name: "entities", label: "monitoring.series.entities", color: "accent" },
            ] },
            { title: "monitoring.charts.indexBacklog", unit: "count", series: [
                { name: "index_backlog", label: "monitoring.series.indexBacklog", color: "orange" },
            ] },
            { title: "monitoring.charts.cacheEntries", unit: "count", series: [
                { name: "cache_entries", label: "monitoring.series.cacheEntries", color: "cyan" },
            ] },
            { title: "monitoring.charts.cacheHitRatio", unit: "percent", series: [
                { name: "cache_hit_ratio", label: "monitoring.series.hitRatio", color: "green" },
            ] },
        ],
    },
    {
        id: "persistence",
        title: "monitoring.sections.persistence",
        extra: "persistence",
        charts: [
            { title: "monitoring.charts.flushDuration", unit: "ms", series: [
                { name: "flush_ms", label: "monitoring.series.flush", color: "green" },
            ] },
            { title: "monitoring.charts.flushFailures", unit: "perMinute", scale: 60, series: [
                { name: "flush_failures_rps", label: "monitoring.series.flushFailures", color: "red" },
                { name: "forced_flushes_rps", label: "monitoring.series.forcedFlushes", color: "accent" },
            ] },
            { title: "monitoring.charts.recoveryLogs", unit: "bytes", series: [
                { name: "recovery_json_bytes", label: "monitoring.series.recoveryJson", color: "accent" },
                { name: "recovery_store_bytes", label: "monitoring.series.recoveryStore", color: "cyan" },
            ] },
            { title: "monitoring.charts.diskSize", unit: "bytes", series: [
                { name: "disk_bytes", label: "monitoring.series.disk", color: "purple" },
            ] },
        ],
    },
    {
        id: "runtime",
        title: "monitoring.sections.runtime",
        charts: [
            { title: "monitoring.charts.memory", unit: "bytes", series: [
                { name: "heap_bytes", label: "monitoring.series.heap", color: "accent" },
                { name: "total_bytes", label: "monitoring.series.total", color: "cyan" },
            ] },
            { title: "monitoring.charts.goroutines", unit: "count", series: [
                { name: "goroutines", label: "monitoring.series.goroutines", color: "green" },
            ] },
            { title: "monitoring.charts.gcPause", unit: "ms", series: [
                { name: "gc_pause_p99_ms", label: "monitoring.series.gcPause", color: "orange" },
            ] },
            { title: "monitoring.charts.gcCycles", unit: "perMinute", scale: 60, series: [
                { name: "gc_cycles_rps", label: "monitoring.series.gcCycles", color: "purple" },
            ] },
            { title: "monitoring.charts.cpu", unit: "percent", scale: 0.01, series: [
                { name: "cpu_percent", label: "monitoring.series.cpu", color: "red" },
            ] },
            { title: "monitoring.charts.openFds", unit: "count", series: [
                { name: "open_fds", label: "monitoring.series.openFds", color: "teal" },
            ] },
        ],
    },
    {
        id: "connections",
        title: "monitoring.sections.connections",
        charts: [
            { title: "monitoring.charts.connections", unit: "count", series: [
                { name: "http_conns", label: "monitoring.series.http", color: "accent" },
                { name: "tcp_conns", label: "monitoring.series.tcp", color: "cyan" },
            ] },
            { title: "monitoring.charts.adminSessions", unit: "count", series: [
                { name: "admin_sessions", label: "monitoring.series.adminSessions", color: "green" },
            ] },
            { title: "monitoring.charts.transactions", unit: "count", series: [
                { name: "open_transactions", label: "monitoring.series.openTransactions", color: "purple" },
            ] },
            { title: "monitoring.charts.oldestTransaction", unit: "seconds", series: [
                { name: "oldest_transaction_seconds", label: "monitoring.series.oldestTransaction", color: "orange" },
            ] },
        ],
    },
    {
        id: "hooks",
        title: "monitoring.sections.hooks",
        extra: "hooks",
        charts: [
            { title: "monitoring.charts.hookExecutions", unit: "rate", series: [
                { name: "hook_executions_rps", label: "monitoring.series.executions", color: "accent" },
            ] },
            { title: "monitoring.charts.hookDuration", unit: "ms", series: [
                { name: "hook_p95_ms", label: "monitoring.series.p95", color: "orange" },
            ] },
        ],
    },
    {
        id: "mongodb",
        title: "monitoring.sections.mongodb",
        mongodb: true,
        charts: [
            { title: "monitoring.charts.mongoPing", unit: "ms", series: [
                { name: "mongo_ping_ms", label: "monitoring.series.ping", color: "green" },
            ] },
            { title: "monitoring.charts.mongoErrors", unit: "perMinute", scale: 60, series: [
                { name: "mongo_errors_rps", label: "monitoring.series.errors", color: "red" },
            ] },
            { title: "monitoring.charts.mongoPool", unit: "count", series: [
                { name: "mongo_pool_in_use", label: "monitoring.series.poolInUse", color: "cyan" },
            ] },
        ],
    },
];

const PHASES = {
    pre_read: "monitoring.hooks.preRead",
    post_read: "monitoring.hooks.postRead",
};

function createMonitoringFetcher(range) {
    const series = seriesFetcher(range);

    return async () => {
        const [history, details, overview] = await Promise.all([
            series(),
            apiFetch("/monitoring/details").catch(() => null),
            apiFetch("/monitoring/overview").catch(() => null),
        ]);

        return { series: history, details, overview };
    };
}

function unitFormatter(format, unit) {
    switch (unit) {
        case "rate":
            return format.rate;
        case "perMinute":
            return format.perMinute;
        case "ms":
            return format.ms;
        case "bytes":
            return format.bytes;
        case "percent":
            return format.percent;
        case "seconds":
            return format.duration;
        default:
            return format.number;
    }
}

function RoutesTable({ title, routes }) {
    const { t } = useTranslation();
    const format = useFormatters();

    return (
        <Col xs={12} xl={6}>
            <Card className="h-100 monitoring-card">
                <Card.Header>{title}</Card.Header>
                {(routes ?? []).length === 0
                    ? <Card.Body className="text-secondary small">{t("monitoring.noData")}</Card.Body>
                    : (
                        <Table size="sm" responsive className="monitoring-table mb-0">
                            <thead>
                                <tr>
                                    <th>{t("monitoring.routes.method")}</th>
                                    <th>{t("monitoring.routes.route")}</th>
                                    <th className="text-end">{t("monitoring.routes.requests")}</th>
                                    <th className="text-end">{t("monitoring.routes.errors")}</th>
                                    <th className="text-end">{t("monitoring.routes.p95")}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {routes.map((route) => (
                                    <tr key={`${route.method} ${route.route}`}>
                                        <td>{route.method}</td>
                                        <td className="text-break">{route.route}</td>
                                        <td className="text-end">{format.count(route.count)}</td>
                                        <td className="text-end">{format.count(route.errors)} ({format.percent(route.error_rate)})</td>
                                        <td className="text-end">{format.ms(route.p95_ms)}</td>
                                    </tr>
                                ))}
                            </tbody>
                        </Table>
                    )}
            </Card>
        </Col>
    );
}

function EntitiesTable({ entityTypes }) {
    const { t } = useTranslation();
    const format = useFormatters();

    return (
        <Col xs={12} xl={6}>
            <Card className="h-100 monitoring-card">
                <Card.Header>{t("monitoring.entities.title")}</Card.Header>
                {(entityTypes ?? []).length === 0
                    ? <Card.Body className="text-secondary small">{t("monitoring.noData")}</Card.Body>
                    : (
                        <Table size="sm" responsive className="monitoring-table mb-0">
                            <thead>
                                <tr>
                                    <th>{t("monitoring.entities.type")}</th>
                                    <th className="text-end">{t("monitoring.entities.count")}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {entityTypes.map((item) => (
                                    <tr key={item.name}>
                                        <td>{item.name}</td>
                                        <td className="text-end">{format.count(item.count)}</td>
                                    </tr>
                                ))}
                            </tbody>
                        </Table>
                    )}
            </Card>
        </Col>
    );
}

function HooksTable({ hooks }) {
    const { t } = useTranslation();
    const format = useFormatters();

    return (
        <Col xs={12}>
            <Card className="monitoring-card">
                <Card.Header>{t("monitoring.hooks.title")}</Card.Header>
                {(hooks ?? []).length === 0
                    ? <Card.Body className="text-secondary small">{t("monitoring.noData")}</Card.Body>
                    : (
                        <Table size="sm" responsive className="monitoring-table mb-0">
                            <thead>
                                <tr>
                                    <th>{t("monitoring.hooks.id")}</th>
                                    <th>{t("monitoring.hooks.entity")}</th>
                                    <th>{t("monitoring.hooks.phase")}</th>
                                    <th className="text-end">{t("monitoring.hooks.executions")}</th>
                                    <th className="text-end">{t("monitoring.hooks.errors")}</th>
                                    <th className="text-end">{t("monitoring.hooks.average")}</th>
                                    <th className="text-end">{t("monitoring.hooks.p95")}</th>
                                    <th>{t("monitoring.hooks.lastError")}</th>
                                </tr>
                            </thead>
                            <tbody>
                                {hooks.map((hook) => (
                                    <tr key={hook.hook_id}>
                                        <td className="text-break">{hook.hook_id}</td>
                                        <td>{hook.entity}</td>
                                        <td>{PHASES[hook.phase] ? t(PHASES[hook.phase]) : hook.phase}</td>
                                        <td className="text-end">{format.count(hook.executions)}</td>
                                        <td className="text-end">{format.count(hook.errors)}</td>
                                        <td className="text-end">{format.ms(hook.avg_ms)}</td>
                                        <td className="text-end">{format.ms(hook.p95_ms)}</td>
                                        <td className="text-break">
                                            {hook.last_error
                                                ? <>{hook.last_error}{hook.last_error_at && <> · <RelativeTime value={hook.last_error_at} /></>}</>
                                                : "–"}
                                        </td>
                                    </tr>
                                ))}
                            </tbody>
                        </Table>
                    )}
            </Card>
        </Col>
    );
}

function PersistenceFacts({ persistence }) {
    const { t } = useTranslation();
    const format = useFormatters();

    return (
        <Col xs={12}>
            <Card className="monitoring-card">
                <Card.Body className="small">
                    <dl className="monitoring-facts monitoring-facts-inline mb-0">
                        <dt>{t("monitoring.persistence.lastSuccess")}</dt>
                        <dd>{persistence?.last_success_at ? <RelativeTime value={persistence.last_success_at} /> : "–"}</dd>
                        <dt>{t("monitoring.persistence.failures")}</dt>
                        <dd>{format.count(persistence?.flush_failures)}</dd>
                        <dt>{t("monitoring.persistence.lastError")}</dt>
                        <dd className="text-break">{persistence?.last_flush_error || "–"}</dd>
                    </dl>
                </Card.Body>
            </Card>
        </Col>
    );
}

function SectionExtra({ extra, data }) {
    const { t } = useTranslation();

    switch (extra) {
        case "routes":
            return (
                <>
                    <RoutesTable title={t("monitoring.routes.last15m")} routes={data.details?.routes?.last_15m} />
                    <RoutesTable title={t("monitoring.routes.sinceStartup")} routes={data.details?.routes?.since_startup} />
                </>
            );
        case "entities":
            return <EntitiesTable entityTypes={data.details?.entity_types} />;
        case "hooks":
            return <HooksTable hooks={data.details?.hooks} />;
        case "persistence":
            return <PersistenceFacts persistence={data.overview?.persistence} />;
        default:
            return null;
    }
}

export default function Monitoring() {
    const { t } = useTranslation();
    const format = useFormatters();
    const [searchParams, setSearchParams] = useSearchParams();
    const range = RANGES[searchParams.get("range")] ? searchParams.get("range") : "15m";
    const [interval, setRefreshInterval] = useStoredInterval(`monitoring.${range}`, RANGES[range].defaultInterval);
    const fetcher = useMemo(() => createMonitoringFetcher(range), [range]);
    const { data, error, loading, lastUpdated, connectionLost, paused, setPaused, refresh } = usePolling(fetcher, interval);

    const selectRange = (value) => {
        setSearchParams((previous) => {
            const next = new URLSearchParams(previous);
            next.set("range", value);
            return next;
        }, { replace: true });
    };

    const time = data?.series?.time ?? [];
    const formatTime = (value) => format.time(value, { seconds: range === "15m" });
    const slowRequestMs = data?.series?.slow_request_ms;
    const threshold = slowRequestMs > 0 ? { value: slowRequestMs, label: t("monitoring.series.slowThreshold") } : undefined;
    const isMongo = data?.overview?.instance?.engine === "mongodb";

    return (
        <Container fluid className="py-4 monitoring-page">
            <div className="d-flex align-items-center gap-3 flex-wrap mb-3">
                <h2 className="monitoring-title mb-0">{t("monitoring.title")}</h2>
                <ButtonGroup size="sm" aria-label={t("monitoring.range")}>
                    {RANGE_OPTIONS.map(([value, minutes]) => (
                        <Button
                            key={value}
                            variant={value === range ? "warning" : "outline-secondary"}
                            className="monitoring-button"
                            aria-pressed={value === range}
                            onClick={() => selectRange(value)}
                        >
                            {format.range(minutes)}
                        </Button>
                    ))}
                </ButtonGroup>
                <div className="flex-grow-1" />
                <RefreshControl
                    interval={interval}
                    onIntervalChange={setRefreshInterval}
                    paused={paused}
                    onPausedChange={setPaused}
                    lastUpdated={lastUpdated}
                    onRefresh={refresh}
                />
            </div>

            <ConnectionLostAlert connectionLost={connectionLost} />

            {loading && !data && (
                <Container className="py-5 text-center">
                    <Spinner animation="border" />
                </Container>
            )}

            {!loading && !data && (
                <Alert variant="danger" role="alert">
                    {t("common.error", { message: error?.message || t("common.unknownError") })}
                </Alert>
            )}

            {data && SECTIONS.filter((section) => !section.mongodb || isMongo).map((section) => (
                <section key={section.id} className="monitoring-section mb-4" aria-labelledby={`monitoring-${section.id}`}>
                    <h3 id={`monitoring-${section.id}`} className="monitoring-section-title">{t(section.title)}</h3>
                    <Row className="g-3">
                        {section.charts.map((chart) => (
                            <Col key={chart.title} xs={12} lg={6} xxl={4}>
                                <ChartCard
                                    title={t(chart.title)}
                                    time={time}
                                    series={chart.series.map((item) => ({
                                        label: t(item.label),
                                        values: values(data.series, item.name, chart.scale ?? 1),
                                        color: item.color,
                                    }))}
                                    format={unitFormatter(format, chart.unit)}
                                    formatTime={formatTime}
                                    height={CHART_HEIGHT}
                                    syncKey={SYNC_KEY}
                                    threshold={chart.threshold ? threshold : undefined}
                                />
                            </Col>
                        ))}
                        {section.extra && <SectionExtra extra={section.extra} data={data} />}
                    </Row>
                </section>
            ))}
        </Container>
    );
}
