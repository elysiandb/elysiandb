import { useState } from "react";
import { Badge, Button } from "react-bootstrap";
import { apiFetch } from "../../utils/api";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";
import { useFormatters } from "../../hooks/i18n/useFormatters.js";
import { useToast } from "../../hooks/notification/useToast.js";
import { LOG_LEVELS } from "../../utils/logs.js";

const DEBUG_MINUTES = 15;

export default function LogLevelControl({ level, onChanged }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const toast = useToast();
    const [saving, setSaving] = useState(false);

    if (!level) return null;

    const current = LOG_LEVELS[level.level] ?? LOG_LEVELS.info;
    const configured = LOG_LEVELS[level.configured] ?? LOG_LEVELS.info;
    const overridden = level.level !== level.configured;

    const change = async (body) => {
        setSaving(true);
        try {
            const result = await apiFetch("/monitoring/logs/level", { method: "PUT", json: body });
            toast?.show("success", t("logs.levelChanged", { level: t((LOG_LEVELS[result?.level] ?? current).label) }));
            onChanged?.(result);
        } catch {
            toast?.show("error", t("logs.levelChangeFailed"));
        } finally {
            setSaving(false);
        }
    };

    return (
        <div className="logs-level-control d-flex align-items-center gap-2 flex-wrap">
            <span>{t("logs.currentLevel")}</span>
            <Badge bg={current.variant}>{t(current.label)}</Badge>
            {overridden && (
                <span className="text-secondary small">
                    {t("logs.configuredLevel", { level: t(configured.label) })}
                    {level.until ? ` · ${t("logs.until", { time: format.time(level.until, { seconds: false }) })}` : ""}
                </span>
            )}
            {level.level !== "debug" && (
                <Button
                    size="sm"
                    variant="outline-secondary"
                    className="monitoring-button"
                    disabled={saving}
                    onClick={() => change({ level: "debug", minutes: DEBUG_MINUTES })}
                >
                    {t("logs.debugFor", { duration: format.range(DEBUG_MINUTES) })}
                </Button>
            )}
            {overridden && (
                <Button
                    size="sm"
                    variant="outline-secondary"
                    className="monitoring-button"
                    disabled={saving}
                    onClick={() => change({ level: level.configured })}
                >
                    {t("logs.resetLevel", { level: t(configured.label) })}
                </Button>
            )}
        </div>
    );
}
