import { Badge, Card, ListGroup } from "react-bootstrap";
import { Link } from "react-router-dom";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";
import { useFormatters } from "../../hooks/i18n/useFormatters.js";
import { useNow } from "../../hooks/monitoring/useNow.js";
import { PROBLEMS, SEVERITIES, STATUS_LEVELS, problemParams } from "./problems.js";

function ProblemAge({ since }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const now = useNow(1000);
    const started = Date.parse(since);

    if (Number.isNaN(started)) return null;

    return (
        <span className="text-secondary small" title={format.dateTime(since)}>
            {t("status.since", { duration: format.duration(Math.max(0, (now - started) / 1000)) })}
        </span>
    );
}

export default function StatusPanel({ status }) {
    const { t } = useTranslation();
    const format = useFormatters();
    const level = STATUS_LEVELS[status?.level] ?? STATUS_LEVELS.healthy;
    const problems = status?.problems ?? [];

    return (
        <Card className="monitoring-status" data-level={status?.level ?? "healthy"}>
            <Card.Header className="d-flex align-items-center gap-2">
                <span>{t("status.title")}</span>
                <Badge bg={level.variant}>{t(level.label)}</Badge>
                <span className="text-secondary small">
                    {problems.length ? t("status.problemCount", { count: format.count(problems.length) }) : t("status.noProblem")}
                </span>
            </Card.Header>
            {problems.length > 0 && (
                <ListGroup variant="flush">
                    {problems.map((problem, index) => {
                        const known = PROBLEMS[problem.code];
                        const params = problemParams(problem, format);
                        const severity = SEVERITIES[problem.severity] ?? SEVERITIES.warning;

                        return (
                            <ListGroup.Item key={`${problem.code}-${index}`} className="d-flex align-items-center gap-2 flex-wrap">
                                <Badge bg={severity.variant}>{t(severity.label)}</Badge>
                                <span className="flex-grow-1">
                                    {known ? t(known.message, params) : t("status.problems.unknown", params)}
                                </span>
                                {problem.since && <ProblemAge since={problem.since} />}
                                {known && (
                                    <Link to={known.link} className="monitoring-link">
                                        {t(known.linkLabel)} →
                                    </Link>
                                )}
                            </ListGroup.Item>
                        );
                    })}
                </ListGroup>
            )}
        </Card>
    );
}
