import { useState } from "react";
import { Alert, Badge, Button, Card, Col, Container, ListGroup, Row, Spinner } from "react-bootstrap";
import { Link } from "react-router-dom";
import { apiFetch } from "../utils/api";
import { seriesFetcher, sum, values } from "../utils/series.js";
import { levelOf } from "../utils/logs.js";
import { useTranslation } from "../hooks/i18n/useTranslation.js";
import { useFormatters } from "../hooks/i18n/useFormatters.js";
import { useToast } from "../hooks/notification/useToast.js";
import { usePolling } from "../hooks/monitoring/usePolling.js";
import { useStoredInterval } from "../hooks/monitoring/useStoredInterval.js";
import RefreshControl, { ConnectionLostAlert } from "../components/monitoring/RefreshControl.jsx";
import RelativeTime from "../components/monitoring/RelativeTime.jsx";
import StatusPanel from "../components/monitoring/StatusPanel.jsx";
import KpiCard from "../components/monitoring/KpiCard.jsx";
import ChartCard from "../components/monitoring/ChartCard.jsx";

const DEFAULT_INTERVAL = 5000;
const RECENT_LIMIT = 5;
const CHART_HEIGHT = 150;

const FEATURES = {
    cache: "dashboard.features.cache",
    hooks: "dashboard.features.hooks",
    schema: "dashboard.features.schema",
    crash_recovery: "dashboard.features.crash_recovery",
    admin_ui: "dashboard.features.admin_ui",
    stats: "dashboard.features.stats",
    prometheus: "dashboard.features.prometheus",
};

const ENGINES = {
    internal: "dashboard.engines.internal",
    mongodb: "dashboard.engines.mongodb",
};

function createDashboardFetcher() {
    const series = seriesFetcher("15m");
    let lastHistory = null;

    return async () => {
        const [overview, history, problems, activity] = await Promise.all([
            apiFetch("/monitoring/overview"),
            series().then((value) => {
                lastHistory = value;
                return value;
            }, () => lastHistory),
            apiFetch(`/monitoring/logs?level=warn&limit=${RECENT_LIMIT}`).catch(() => null),
            apiFetch(`/monitoring/logs?component=audit&limit=${RECENT_LIMIT}`).catch(() => null),
        ]);

        return {
            overview,
            series: history,
            problems: problems ? problems.entries ?? [] : null,
            activity: activity ? activity.entries ?? [] : null,
        };
    };
}

function RecentList({ title, entries, emptyText, link, showLevel }) {
    const { t } = useTranslation();
    const format = useFormatters();

    return (
        <Card className="h-100 monitoring-card">
            <Card.Header className="d-flex justify-content-between align-items-center">
                <span>{title}</span>
                <Link to={link} className="monitoring-link small">{t("dashboard.allLogs")} →</Link>
            </Card.Header>
            {entries === null && <Card.Body className="text-secondary small">{t("dashboard.logsUnavailable")}</Card.Body>}
            {entries !== null && entries.length === 0 && <Card.Body className="text-secondary small">{emptyText}</Card.Body>}
            {entries !== null && entries.length > 0 && (
                <ListGroup variant="flush" className="monitoring-recent">
                    {entries.map((entry) => {
                        const level = levelOf(entry);
                        const actor = entry.attrs?.actor ?? entry.attrs?.user;
                        return (
                            <ListGroup.Item key={entry.seq} className="d-flex gap-2 align-items-baseline">
                                <span className="logs-time">{format.time(entry.time)}</span>
                                {showLevel && <Badge bg={level.variant}>{t(level.label)}</Badge>}
                                {!showLevel && actor && <span className="monitoring-actor">{String(actor)}</span>}
                                <span className="text-truncate" title={entry.msg}>{entry.msg}</span>
                            </ListGroup.Item>
                        );
                    })}
                </ListGroup>
            )}
        </Card>
    );
}

function DataCard({ data }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const top = (data?.top_entity_types ?? []).slice(0, 5);
    const max = Math.max(1, ...top.map((item) => item.count ?? 0));

    return (
        <Card className="h-100 monitoring-card">
            <Card.Header>{t("dashboard.data.title")}</Card.Header>
            <Card.Body className="small">
                <dl className="monitoring-facts">
                    <dt>{t("dashboard.data.kvKeys")}</dt><dd>{format.count(data?.kv_keys)}</dd>
                    <dt>{t("dashboard.data.kvKeysWithTtl")}</dt><dd>{format.count(data?.kv_keys_with_ttl)}</dd>
                    <dt>{t("dashboard.data.entityTypes")}</dt><dd>{format.count(data?.entity_types)}</dd>
                    <dt>{t("dashboard.data.entities")}</dt><dd>{format.count(data?.entities)}</dd>
                    <dt>{t("dashboard.data.disk")}</dt><dd>{format.bytes(data?.disk_bytes)}</dd>
                </dl>
                {top.length > 0 && (
                    <div className="monitoring-top">
                        <div className="text-secondary mb-1">{t("dashboard.data.largest")}</div>
                        {top.map((item) => (
                            <Link key={item.name} to={`/admin/browse?entity=${encodeURIComponent(item.name)}`} className="monitoring-bar-row">
                                <span className="monitoring-bar-label">{item.name}</span>
                                <span className="monitoring-bar"><span style={{ width: `${Math.round(((item.count ?? 0) / max) * 100)}%` }} /></span>
                                <span className="monitoring-bar-value">{format.count(item.count)}</span>
                            </Link>
                        ))}
                    </div>
                )}
            </Card.Body>
        </Card>
    );
}

function InstanceCard({ instance }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const toast = useToast();
    const [flushing, setFlushing] = useState(false);
    const features = Object.entries(instance?.features ?? {}).filter(([, enabled]) => enabled);

    const flush = async () => {
        setFlushing(true);
        try {
            await apiFetch("/save", { method: "POST" });
            toast?.show("success", t("dashboard.actions.flushed"));
        } catch {
            toast?.show("error", t("dashboard.actions.flushFailed"));
        } finally {
            setFlushing(false);
        }
    };

    const address = (server) => (server?.enabled ? server.address : t("common.disabled"));

    return (
        <Card className="monitoring-card">
            <Card.Header className="d-flex justify-content-between align-items-center">
                <span>{t("dashboard.instance.title")}</span>
                <Link to="/admin/configuration" className="monitoring-link small">{t("nav.configuration")} →</Link>
            </Card.Header>
            <Card.Body className="small">
                <Row className="g-3">
                    <Col md={8}>
                        <dl className="monitoring-facts monitoring-facts-wide">
                            <dt>{t("dashboard.instance.version")}</dt><dd>{instance?.version ?? "–"}</dd>
                            <dt>{t("dashboard.instance.engine")}</dt><dd>{ENGINES[instance?.engine] ? t(ENGINES[instance.engine]) : instance?.engine ?? "–"}</dd>
                            <dt>{t("dashboard.instance.uptime")}</dt><dd>{format.duration(instance?.uptime_seconds)}</dd>
                            <dt>{t("dashboard.instance.startedAt")}</dt><dd>{format.dateTime(instance?.started_at)}</dd>
                            <dt>{t("dashboard.instance.http")}</dt><dd>{address(instance?.http)}</dd>
                            <dt>{t("dashboard.instance.tcp")}</dt><dd>{address(instance?.tcp)}</dd>
                            <dt>{t("dashboard.instance.authentication")}</dt><dd>{instance?.auth_mode === "disabled" ? t("common.disabled") : instance?.auth_mode ?? "–"}</dd>
                            <dt>{t("dashboard.instance.dataFolder")}</dt><dd className="text-break">{instance?.data_folder ?? "–"}</dd>
                        </dl>
                        {features.length > 0 && (
                            <div className="d-flex gap-1 flex-wrap">
                                {features.map(([name]) => (
                                    <Badge key={name} bg="secondary">{FEATURES[name] ? t(FEATURES[name]) : name}</Badge>
                                ))}
                            </div>
                        )}
                    </Col>
                    <Col md={4} className="d-flex flex-column gap-2 align-items-md-end">
                        <a
                            className="btn btn-sm btn-outline-secondary monitoring-button"
                            href="/api/backup"
                            download
                            onClick={() => toast?.show("success", t("dashboard.actions.backupStarted"))}
                        >
                            {t("dashboard.actions.backup")}
                        </a>
                        <Button size="sm" variant="outline-secondary" className="monitoring-button" disabled={flushing} onClick={flush}>
                            {t("dashboard.actions.flush")}
                        </Button>
                    </Col>
                </Row>
            </Card.Body>
        </Card>
    );
}

export default function Dashboard() {
    const { t } = useTranslation();
    const format = useFormatters();
    const [interval, setRefreshInterval] = useStoredInterval("dashboard", DEFAULT_INTERVAL);
    const [fetcher] = useState(createDashboardFetcher);
    const { data, error, loading, lastUpdated, connectionLost, paused, setPaused, refresh } = usePolling(fetcher, interval);

    if (loading && !data) {
        return (
            <Container className="py-5 text-center">
                <Spinner animation="border" />
            </Container>
        );
    }

    if (!data) {
        return (
            <Container fluid className="py-4">
                <Alert variant="danger" role="alert">
                    {t("common.error", { message: error?.message || t("common.unknownError") })}
                </Alert>
            </Container>
        );
    }

    const { overview, series } = data;
    const traffic = overview?.traffic ?? {};
    const persistence = overview?.persistence ?? {};
    const time = series?.time ?? [];
    const formatTime = (value) => format.time(value, { seconds: false });
    const threshold = series?.slow_request_ms > 0 ? { value: series.slow_request_ms, label: t("monitoring.series.slowThreshold") } : undefined;

    return (
        <Container fluid className="py-4 monitoring-page dashboard-page">
            <div className="d-flex align-items-center gap-3 flex-wrap mb-3">
                <h2 className="monitoring-title mb-0">{t("dashboard.title")}</h2>
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

            <Row className="g-3">
                <Col xs={12}>
                    <StatusPanel status={overview?.status} />
                </Col>

                <Col xs={12} sm={6} xl={3}>
                    <KpiCard
                        title={t("dashboard.kpi.requests")}
                        value={format.rate(traffic.requests_per_second)}
                        detail={t("dashboard.kpi.split", { http: format.rate(traffic.http_requests_per_second), tcp: format.rate(traffic.tcp_commands_per_second) })}
                        time={time}
                        values={sum(series, ["http_rps", "tcp_rps"])}
                        format={format.rate}
                        formatTime={formatTime}
                        color="accent"
                    />
                </Col>
                <Col xs={12} sm={6} xl={3}>
                    <KpiCard
                        title={t("dashboard.kpi.errors")}
                        value={format.percent(traffic.error_rate)}
                        detail={t("dashboard.kpi.slow", { value: format.perMinute(traffic.slow_requests_per_minute) })}
                        time={time}
                        values={values(series, "http_5xx_rps")}
                        format={format.rate}
                        formatTime={formatTime}
                        color="red"
                        tone={traffic.error_rate > 0.01 ? "danger" : "neutral"}
                    />
                </Col>
                <Col xs={12} sm={6} xl={3}>
                    <KpiCard
                        title={t("dashboard.kpi.latency")}
                        value={format.ms(traffic.p95_ms)}
                        time={time}
                        values={values(series, "http_p95_ms")}
                        format={format.ms}
                        formatTime={formatTime}
                        color="cyan"
                    />
                </Col>
                <Col xs={12} sm={6} xl={3}>
                    <KpiCard
                        title={t("dashboard.kpi.flush")}
                        value={persistence.last_flush_at ? <RelativeTime value={persistence.last_flush_at} /> : "–"}
                        detail={
                            <span className="d-inline-flex gap-2 align-items-center">
                                <Badge bg={persistence.last_flush_ok === false ? "danger" : "success"} title={persistence.last_flush_error || undefined}>
                                    {persistence.last_flush_ok === false ? t("dashboard.kpi.flushFailed") : t("dashboard.kpi.flushOk")}
                                </Badge>
                                <span>{format.ms(persistence.last_flush_ms)}</span>
                            </span>
                        }
                        time={time}
                        values={values(series, "flush_ms")}
                        format={format.ms}
                        formatTime={formatTime}
                        color="green"
                        tone={persistence.last_flush_ok === false ? "danger" : "neutral"}
                    />
                </Col>

                <Col xs={12} lg={6}>
                    <ChartCard
                        title={t("dashboard.charts.throughput")}
                        time={time}
                        series={[
                            { label: t("monitoring.series.http"), values: values(series, "http_rps"), color: "accent" },
                            { label: t("monitoring.series.tcp"), values: values(series, "tcp_rps"), color: "cyan" },
                        ]}
                        format={format.rate}
                        formatTime={formatTime}
                        height={CHART_HEIGHT}
                        syncKey="dashboard"
                    />
                </Col>
                <Col xs={12} lg={6}>
                    <ChartCard
                        title={t("dashboard.charts.latency")}
                        time={time}
                        series={[
                            { label: t("monitoring.series.p50"), values: values(series, "http_p50_ms"), color: "green" },
                            { label: t("monitoring.series.p95"), values: values(series, "http_p95_ms"), color: "accent" },
                            { label: t("monitoring.series.p99"), values: values(series, "http_p99_ms"), color: "orange" },
                        ]}
                        format={format.ms}
                        formatTime={formatTime}
                        height={CHART_HEIGHT}
                        syncKey="dashboard"
                        threshold={threshold}
                    />
                </Col>

                <Col xs={12} lg={4}>
                    <RecentList
                        title={t("dashboard.recentProblems")}
                        entries={data.problems}
                        emptyText={t("dashboard.noRecentProblems")}
                        link="/admin/logs?level=warn"
                        showLevel
                    />
                </Col>
                <Col xs={12} lg={4}>
                    <RecentList
                        title={t("dashboard.adminActivity")}
                        entries={data.activity}
                        emptyText={t("dashboard.noActivity")}
                        link="/admin/logs?component=audit"
                    />
                </Col>
                <Col xs={12} lg={4}>
                    <DataCard data={overview?.data} />
                </Col>

                <Col xs={12}>
                    <InstanceCard instance={overview?.instance} />
                </Col>
            </Row>
        </Container>
    );
}
