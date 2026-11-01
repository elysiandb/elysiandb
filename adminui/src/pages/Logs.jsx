import { useCallback, useMemo, useRef, useState } from "react";
import { Alert, Badge, Button, Card, Col, Container, Form, Row, Spinner } from "react-bootstrap";
import { useSearchParams } from "react-router-dom";
import { apiFetch } from "../utils/api";
import { useTranslation } from "../hooks/i18n/useTranslation.js";
import { useFormatters } from "../hooks/i18n/useFormatters.js";
import { useToast } from "../hooks/notification/useToast.js";
import { usePolling } from "../hooks/monitoring/usePolling.js";
import { ConnectionLostAlert } from "../components/monitoring/RefreshControl.jsx";
import RelativeTime from "../components/monitoring/RelativeTime.jsx";
import LogEntryRow from "../components/log/LogEntryRow.jsx";
import LogLevelControl from "../components/log/LogLevelControl.jsx";
import { FILTER_KEYS, LOG_LEVELS, LOG_RANGES, TEXT_FILTER_KEYS, filtersKey, logsQuery, readFilters } from "../utils/logs.js";

const PAGE_SIZE = 200;
const FOLLOW_INTERVAL = 2000;
const META_INTERVAL = 30000;
const MAX_ENTRIES = 5000;
const RENDER_STEP = 200;
const FOLLOW_STORAGE_KEY = "logs.follow";

function readFollow() {
    try {
        return localStorage.getItem(FOLLOW_STORAGE_KEY) !== "false";
    } catch {
        return true;
    }
}

function writeFollow(value) {
    try {
        localStorage.setItem(FOLLOW_STORAGE_KEY, String(value));
    } catch {
        return;
    }
}

function createLogsFetcher(filters) {
    let state = null;
    let olderRequested = false;
    let atTop = true;

    const fetcher = async () => {
        if (!state) {
            const response = await apiFetch(`/monitoring/logs?${logsQuery(filters, { limit: PAGE_SIZE })}`);
            state = { entries: response.entries ?? [], pending: [], meta: response, hasMore: Boolean(response.has_more) };
            return state;
        }

        if (olderRequested) {
            olderRequested = false;
            const oldest = state.entries[state.entries.length - 1];
            if (oldest) {
                const response = await apiFetch(`/monitoring/logs?${logsQuery(filters, { before: oldest.seq, limit: PAGE_SIZE })}`);
                state = {
                    ...state,
                    entries: [...state.entries, ...(response.entries ?? [])].slice(0, MAX_ENTRIES),
                    hasMore: Boolean(response.has_more),
                    meta: { ...state.meta, ...response, entries: undefined },
                };
            }
            return state;
        }

        const newest = state.pending[0] ?? state.entries[0];
        const after = newest ? newest.seq : (state.meta?.latest_seq ?? 0);
        const response = await apiFetch(`/monitoring/logs?${logsQuery(filters, { after, limit: 1000 })}`);
        const fresh = response.entries ?? [];
        const meta = { ...state.meta, ...response, entries: undefined };

        if (atTop) {
            const merged = [...fresh, ...state.pending, ...state.entries];
            state = {
                ...state,
                entries: merged.slice(0, MAX_ENTRIES),
                pending: [],
                hasMore: state.hasMore || merged.length > MAX_ENTRIES,
                meta,
            };
        } else {
            state = { ...state, pending: [...fresh, ...state.pending].slice(0, MAX_ENTRIES), meta };
        }

        return state;
    };

    return {
        fetcher,
        requestOlder: () => {
            olderRequested = true;
        },
        setAtTop: (value) => {
            atTop = value;
        },
    };
}

async function fetchLogsMeta() {
    const [overview, level, latest] = await Promise.all([
        apiFetch("/monitoring/overview").catch(() => null),
        apiFetch("/monitoring/logs/level").catch(() => null),
        apiFetch("/monitoring/logs?limit=1").catch(() => null),
    ]);
    return { overview, level, components: latest?.components ?? [] };
}

function LogFilters({ filters, components, onChange, onClear }) {
    const { t } = useTranslation();
    const options = useMemo(() => {
        const list = new Set(components ?? []);
        if (filters.component) list.add(filters.component);
        return [...list].sort();
    }, [components, filters.component]);

    const submit = (event) => {
        event.preventDefault();
        const data = new FormData(event.currentTarget);
        onChange(Object.fromEntries(TEXT_FILTER_KEYS.map((key) => [key, String(data.get(key) ?? "").trim()])));
    };

    return (
        <Form className="logs-filters" onSubmit={submit} role="search">
            <Row className="g-2 align-items-end">
                <Col xs={6} md={2}>
                    <Form.Label htmlFor="logs-level">{t("logs.filters.level")}</Form.Label>
                    <Form.Select
                        id="logs-level"
                        size="sm"
                        className="monitoring-select"
                        value={filters.level}
                        onChange={(event) => onChange({ level: event.target.value })}
                    >
                        <option value="">{t("logs.filters.allLevels")}</option>
                        {Object.entries(LOG_LEVELS).map(([value, level]) => (
                            <option key={value} value={value}>{t(level.label)}</option>
                        ))}
                    </Form.Select>
                </Col>
                <Col xs={6} md={2}>
                    <Form.Label htmlFor="logs-component">{t("logs.filters.component")}</Form.Label>
                    <Form.Select
                        id="logs-component"
                        size="sm"
                        className="monitoring-select"
                        value={filters.component}
                        onChange={(event) => onChange({ component: event.target.value })}
                    >
                        <option value="">{t("logs.filters.allComponents")}</option>
                        {options.map((component) => (
                            <option key={component} value={component}>{component}</option>
                        ))}
                    </Form.Select>
                </Col>
                <Col xs={6} md={2}>
                    <Form.Label htmlFor="logs-range">{t("logs.filters.range")}</Form.Label>
                    <Form.Select
                        id="logs-range"
                        size="sm"
                        className="monitoring-select"
                        value={filters.range}
                        onChange={(event) => onChange({ range: event.target.value })}
                    >
                        <option value="">{t("logs.ranges.all")}</option>
                        {Object.entries(LOG_RANGES).map(([value, range]) => (
                            <option key={value} value={value}>{t(range.label)}</option>
                        ))}
                    </Form.Select>
                </Col>
                <Col xs={6} md={2}>
                    <Form.Label htmlFor="logs-q">{t("logs.filters.search")}</Form.Label>
                    <Form.Control key={`q:${filters.q}`} id="logs-q" name="q" size="sm" className="monitoring-input" defaultValue={filters.q} />
                </Col>
                <Col xs={6} md={1}>
                    <Form.Label htmlFor="logs-user">{t("logs.filters.user")}</Form.Label>
                    <Form.Control key={`user:${filters.user}`} id="logs-user" name="user" size="sm" className="monitoring-input" defaultValue={filters.user} />
                </Col>
                <Col xs={6} md={2}>
                    <Form.Label htmlFor="logs-request-id">{t("logs.filters.requestId")}</Form.Label>
                    <Form.Control key={`request_id:${filters.request_id}`} id="logs-request-id" name="request_id" size="sm" className="monitoring-input" defaultValue={filters.request_id} />
                </Col>
                <Col xs={12} md="auto" className="d-flex gap-2">
                    <Button type="submit" size="sm" className="monitoring-button">{t("logs.filters.apply")}</Button>
                    <Button type="button" size="sm" variant="outline-secondary" className="monitoring-button" onClick={onClear}>
                        {t("logs.filters.clear")}
                    </Button>
                </Col>
            </Row>
        </Form>
    );
}

function LogList({ filters, follow, onFilter }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const toast = useToast();
    const listRef = useRef(null);
    const [source] = useState(() => createLogsFetcher(filters));
    const [expanded, setExpanded] = useState(() => new Set());
    const [visible, setVisible] = useState(RENDER_STEP);
    const polling = usePolling(source.fetcher, follow ? FOLLOW_INTERVAL : 0);
    const { data, error, loading, refresh, lastUpdated, connectionLost } = polling;

    const toggle = useCallback((seq) => {
        setExpanded((previous) => {
            const next = new Set(previous);
            if (next.has(seq)) next.delete(seq); else next.add(seq);
            return next;
        });
    }, []);

    const copy = useCallback(async (entry) => {
        try {
            await navigator.clipboard.writeText(JSON.stringify(entry));
            toast?.show("success", t("logs.copied"));
        } catch {
            toast?.show("error", t("logs.copyFailed"));
        }
    }, [t, toast]);

    const onScroll = (event) => {
        const element = event.currentTarget;
        source.setAtTop(element.scrollTop < 8);
        if (element.scrollTop + element.clientHeight >= element.scrollHeight - 200 && data && visible < data.entries.length) {
            setVisible((previous) => previous + RENDER_STEP);
        }
    };

    const showPending = () => {
        source.setAtTop(true);
        if (listRef.current) {
            listRef.current.scrollTop = 0;
        }
        refresh();
    };

    const loadOlder = () => {
        source.requestOlder();
        setVisible((previous) => previous + PAGE_SIZE);
        refresh();
    };

    if (error?.status === 404) {
        return <Alert variant="info">{t("logs.disabled")}</Alert>;
    }

    if (loading && !data) {
        return (
            <Container className="py-5 text-center">
                <Spinner animation="border" />
            </Container>
        );
    }

    if (!data) {
        return (
            <Alert variant="danger" role="alert">
                {t("common.error", { message: error?.message || t("common.unknownError") })}
            </Alert>
        );
    }

    const entries = data.entries.slice(0, visible);
    const meta = data.meta ?? {};

    return (
        <Card className="logs-card">
            <Card.Header className="d-flex align-items-center gap-3 flex-wrap">
                <span>{t("logs.title")}</span>
                <span className="text-secondary small flex-grow-1">
                    {t("logs.bufferNote", {
                        size: format.count(meta.size),
                        capacity: format.count(meta.capacity),
                        since: format.time(meta.buffer_since, { seconds: false }),
                    })}
                </span>
                <span className="text-secondary small">
                    {lastUpdated ? <>{t("refresh.updated")} <RelativeTime value={lastUpdated} /></> : null}
                </span>
                {!follow && (
                    <Button size="sm" variant="outline-secondary" className="monitoring-button" onClick={refresh}>
                        {t("refresh.now")}
                    </Button>
                )}
            </Card.Header>
            <ConnectionLostAlert connectionLost={connectionLost} />
            {data.pending.length > 0 && (
                <div className="logs-pending">
                    <Button size="sm" className="monitoring-button" onClick={showPending}>
                        {t("logs.newLines", { count: format.count(data.pending.length) })}
                    </Button>
                </div>
            )}
            <div ref={listRef} className="logs-list" onScroll={onScroll} role="list" aria-label={t("logs.title")}>
                {entries.length === 0 && <div className="logs-empty text-secondary">{t("logs.empty")}</div>}
                {entries.map((entry) => (
                    <div role="listitem" key={entry.seq}>
                        <LogEntryRow
                            entry={entry}
                            expanded={expanded.has(entry.seq)}
                            onToggle={toggle}
                            onFilter={onFilter}
                            onCopy={copy}
                        />
                    </div>
                ))}
                {(visible < data.entries.length || data.hasMore) && (
                    <div className="logs-more">
                        <Button size="sm" variant="outline-secondary" className="monitoring-button" onClick={loadOlder}>
                            {t("logs.loadOlder")}
                        </Button>
                    </div>
                )}
            </div>
        </Card>
    );
}

export default function Logs() {
    const { t } = useTranslation();
    const format = useFormatters();
    const [searchParams, setSearchParams] = useSearchParams();
    const filters = useMemo(() => readFilters(searchParams), [searchParams]);
    const [follow, setFollowState] = useState(readFollow);
    const meta = usePolling(fetchLogsMeta, META_INTERVAL);

    const setFollow = (value) => {
        writeFollow(value);
        setFollowState(value);
    };

    const updateFilters = useCallback((changes) => {
        setSearchParams((previous) => {
            const next = new URLSearchParams(previous);
            for (const [key, value] of Object.entries(changes)) {
                if (value) next.set(key, value); else next.delete(key);
            }
            return next;
        }, { replace: true });
    }, [setSearchParams]);

    const clearFilters = useCallback(() => {
        setSearchParams((previous) => {
            const next = new URLSearchParams(previous);
            for (const key of FILTER_KEYS) next.delete(key);
            return next;
        }, { replace: true });
    }, [setSearchParams]);

    const onFilter = useCallback((key, value) => updateFilters({ [key]: value }), [updateFilters]);

    const counters = meta.data?.overview?.logs;
    const downloadHref = `/monitoring/logs/download?${logsQuery(filters)}`;

    return (
        <Container fluid className="py-4 monitoring-page logs-page">
            <div className="d-flex align-items-center gap-3 flex-wrap mb-3">
                <h2 className="monitoring-title mb-0">{t("logs.title")}</h2>
                {counters && (
                    <div className="d-flex gap-2 flex-wrap">
                        <Badge bg="danger">{t("logs.errorsCount", { hour: format.count(counters.errors_1h), day: format.count(counters.errors_24h) })}</Badge>
                        <Badge bg="warning" text="dark">{t("logs.warningsCount", { hour: format.count(counters.warnings_1h), day: format.count(counters.warnings_24h) })}</Badge>
                    </div>
                )}
                <div className="flex-grow-1" />
                <Form.Check
                    type="switch"
                    id="logs-follow"
                    label={t("logs.follow")}
                    checked={follow}
                    onChange={(event) => setFollow(event.target.checked)}
                />
                <a className="btn btn-sm btn-outline-secondary monitoring-button" href={downloadHref} download>
                    {t("logs.download")}
                </a>
            </div>
            <div className="mb-3">
                <LogLevelControl level={meta.data?.level} onChanged={meta.refresh} />
            </div>
            <div className="mb-3">
                <LogFilters filters={filters} components={meta.data?.components} onChange={updateFilters} onClear={clearFilters} />
            </div>
            <LogList key={filtersKey(filters)} filters={filters} follow={follow} onFilter={onFilter} />
        </Container>
    );
}
