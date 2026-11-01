import { Card, Col, Container, ListGroup, Row, Spinner } from "react-bootstrap";
import { useEffect, useMemo, useState } from "react";
import { useSearchParams } from "react-router-dom";
import { useEntityTypes } from "../hooks/entity/useEntityTypes.js";
import { useTranslation } from "../hooks/i18n/useTranslation.js";
import EntityDataList from "../components/entity/EntityDataList.jsx";

export default function EntitiesBrowser() {
    const { list, loadAll, loading } = useEntityTypes();
    const { t } = useTranslation();
    const [searchParams] = useSearchParams();
    const requested = searchParams.get("entity");

    useEffect(() => {
        loadAll();
    }, [loadAll]);

    const entities = useMemo(() => {
        if (!list || !list.entities) return [];
        return list.entities
            .filter(e => e && e !== "null")
            .map(e => {
                try {
                    const parsed = JSON.parse(e);
                    return { id: parsed.id, manual: parsed._manual };
                } catch {
                    return null;
                }
            })
            .filter(e => e !== null);
    }, [list]);

    const [selectedState, setSelectedState] = useState(null);
    const fromUrl = entities.some((entity) => entity.id === requested) ? requested : null;
    const selected = selectedState ?? fromUrl ?? (entities.length ? entities[0].id : null);

    if (loading) {
        return (
            <Container className="py-5 text-center">
                <Spinner animation="border" />
            </Container>
        );
    }

    return (
        <Container fluid className="py-4 entity-types-layout">
            <Row className="g-4">
                <Col xs={3}>
                    <Card className="entity-sidebar">
                        <Card.Header className="text-warning fw-bold">
                            {t("common.entities")}
                        </Card.Header>

                        <ListGroup variant="flush">
                            {entities.map(entity => (
                                <ListGroup.Item
                                    key={entity.id}
                                    onClick={() => setSelectedState(entity.id)}
                                    className={
                                        "entity-list-item" +
                                        (selected === entity.id ? " selected" : "")
                                    }
                                >
                                    {entity.id}
                                </ListGroup.Item>
                            ))}
                        </ListGroup>
                    </Card>
                </Col>

                <Col xs={9}>
                    {selected && (
                        <EntityDataList entity={selected} />
                    )}
                </Col>
            </Row>
        </Container>
    );
}
