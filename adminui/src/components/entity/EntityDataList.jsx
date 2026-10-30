import { Card, Spinner, Button } from "react-bootstrap";
import { useEffect, useState } from "react";
import { useEntities } from "../../hooks/entity/useEntities.js";
import JsonView from "../json/JsonView.jsx";
import QueryHelp from "./QueryHelp.jsx";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";

export default function EntityDataList({ entity }) {
    const { list, loadAll, loading, error, dropEntity } = useEntities();
    const [queryText, setQueryText] = useState("");
    const [queryOpen, setQueryOpen] = useState(false);
    const { t } = useTranslation();

    useEffect(() => {
        if (!entity) return;

        const baseQuery = { entity, limit: 50 };
        setQueryText(JSON.stringify(baseQuery, null, 2));
        loadAll(baseQuery);
        setQueryOpen(false);
    }, [entity, loadAll]);

    const runQuery = () => {
        try {
            loadAll(JSON.parse(queryText));
        } catch {
            alert(t("browse.invalidQuery"));
        }
    };

    const onDelete = async (id) => {
        if (!confirm(t("browse.confirmDelete", { id }))) return;
        await dropEntity(entity, id);
        runQuery();
    };

    return (
        <Card className="entity-content">
            <Card.Header className="fw-bold text-warning">
                {entity}
            </Card.Header>

            <Card.Body>
                <div
                    className="query-header"
                    onClick={() => setQueryOpen(v => !v)}
                >
                    <span className="query-title">{t("browse.query")}</span>
                    <span className="query-chevron">
                        {queryOpen ? "▾" : "▸"}
                    </span>
                </div>

                {queryOpen && (
                    <div className="query-panel">
                        <div className="query-grid">
                            <div className="query-editor">
                                <textarea
                                    className="query-textarea"
                                    value={queryText}
                                    onChange={e => setQueryText(e.target.value)}
                                    rows={10}
                                />

                                <div className="query-actions">
                                    <Button
                                        size="sm"
                                        variant="warning"
                                        onClick={runQuery}
                                        disabled={loading}
                                    >
                                        {t("browse.search")}
                                    </Button>
                                </div>
                            </div>

                            <QueryHelp />
                        </div>
                    </div>
                )}

                {loading && (
                    <div className="py-4 text-center">
                        <Spinner animation="border" />
                    </div>
                )}

                {error && (
                    <div className="text-danger mb-3">
                        {t("browse.queryError")}
                    </div>
                )}

                {!loading && Array.isArray(list) && (
                    <div className="entity-documents">
                        {list.map((doc, i) => (
                            <Card key={doc.id ?? i} className="entity-document mb-3">
                                <Card.Header className="d-flex justify-content-between align-items-center fw-bold">
                                    <span>{doc.id ?? t("browse.document")}</span>
                                    {doc.id && (
                                        <Button
                                            size="sm"
                                            variant="danger"
                                            onClick={() => onDelete(doc.id)}
                                            disabled={loading === doc.id}
                                        >
                                            {t("common.delete")}
                                        </Button>
                                    )}
                                </Card.Header>

                                <Card.Body className="json-container">
                                    <JsonView data={doc} />
                                </Card.Body>
                            </Card>
                        ))}
                    </div>
                )}
            </Card.Body>
        </Card>
    );
}
