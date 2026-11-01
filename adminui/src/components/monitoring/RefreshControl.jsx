import { Alert, Button, Form } from "react-bootstrap";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";
import { useFormatters } from "../../hooks/i18n/useFormatters.js";
import { REFRESH_INTERVALS } from "../../hooks/monitoring/useStoredInterval.js";
import RelativeTime from "./RelativeTime.jsx";

export function ConnectionLostAlert({ connectionLost }) {
    const { t } = useTranslation();

    if (!connectionLost) return null;

    return (
        <Alert variant="warning" className="monitoring-connection-lost" role="alert">
            {t("refresh.connectionLost")}
        </Alert>
    );
}

export default function RefreshControl({
    interval,
    onIntervalChange,
    paused,
    onPausedChange,
    lastUpdated,
    onRefresh,
    intervals = REFRESH_INTERVALS,
}) {
    const { t } = useTranslation();
    const format = useFormatters();

    return (
        <div className="monitoring-refresh d-flex align-items-center gap-2 flex-wrap">
            <span className="monitoring-updated text-secondary small">
                {lastUpdated
                    ? <>{t("refresh.updated")} <RelativeTime value={lastUpdated} /></>
                    : t("refresh.never")}
            </span>
            <Form.Select
                size="sm"
                className="monitoring-select"
                aria-label={t("refresh.interval")}
                value={String(interval)}
                onChange={(event) => onIntervalChange(Number(event.target.value))}
            >
                {intervals.map((value) => (
                    <option key={value} value={String(value)}>
                        {value === 0 ? t("refresh.off") : format.interval(value)}
                    </option>
                ))}
            </Form.Select>
            <Button
                size="sm"
                variant="outline-secondary"
                className="monitoring-button"
                aria-pressed={paused}
                onClick={() => onPausedChange(!paused)}
            >
                {paused ? t("refresh.resume") : t("refresh.pause")}
            </Button>
            {onRefresh && (
                <Button size="sm" variant="outline-secondary" className="monitoring-button" onClick={onRefresh}>
                    {t("refresh.now")}
                </Button>
            )}
        </div>
    );
}
