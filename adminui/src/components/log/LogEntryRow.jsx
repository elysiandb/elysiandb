import { Badge, Button } from "react-bootstrap";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";
import { useFormatters } from "../../hooks/i18n/useFormatters.js";
import { attributeText, levelOf } from "../../utils/logs.js";

const FILTERABLE_ATTRIBUTES = ["request_id", "user"];

export default function LogEntryRow({ entry, expanded, onToggle, onFilter, onCopy }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const level = levelOf(entry);
    const attributes = Object.entries(entry.attrs ?? {});

    return (
        <div className={`logs-row${expanded ? " expanded" : ""}`} data-level={String(entry.level ?? "").toLowerCase()}>
            <button type="button" className="logs-row-summary" aria-expanded={expanded} onClick={() => onToggle(entry.seq)}>
                <span className="logs-time" title={format.dateTime(entry.time)}>
                    {format.time(entry.time, { milliseconds: true })}
                </span>
                <Badge bg={level.variant} className="logs-level">{t(level.label)}</Badge>
                <span className="logs-component">{entry.component}</span>
                <span className="logs-message">{entry.msg}</span>
            </button>
            {attributes.length > 0 && (
                <div className="logs-attributes">
                    {attributes.map(([key, value]) => {
                        const text = `${key}=${attributeText(value)}`;
                        if (FILTERABLE_ATTRIBUTES.includes(key) && value !== null && value !== undefined && value !== "") {
                            return (
                                <button
                                    key={key}
                                    type="button"
                                    className="logs-chip logs-chip-filter"
                                    title={t("logs.filterBy", { key })}
                                    onClick={() => onFilter(key, attributeText(value))}
                                >
                                    {text}
                                </button>
                            );
                        }
                        return <span key={key} className="logs-chip">{text}</span>;
                    })}
                </div>
            )}
            {expanded && (
                <div className="logs-details">
                    <pre>{JSON.stringify(entry, null, 2)}</pre>
                    <Button size="sm" variant="outline-secondary" className="monitoring-button" onClick={() => onCopy(entry)}>
                        {t("logs.copy")}
                    </Button>
                </div>
            )}
        </div>
    );
}
