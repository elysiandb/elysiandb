import {
    Button,
    Card,
    Col,
    Container,
    ListGroup, Modal,
    Row,
    Form,
    Spinner
} from "react-bootstrap";
import { useEffect, useMemo, useState } from "react";
import { useEntityTypes } from "../hooks/entity/useEntityTypes.js";
import EntityTypes from "../components/entity/EntityType.jsx";
import {useEntityTypeActions} from "../hooks/entity/useEntityTypeActions.js";

export default function EntityTypesList() {
    const { list, loadAll, loading } = useEntityTypes();
    const { createEntityType } = useEntityTypeActions();

    const [showModal, setShowModal] = useState(false);
    const [newEntityName, setNewEntityName] = useState("");

    const openModal = () => setShowModal(true);
    const closeModal = () => {
        setNewEntityName("");
        setShowModal(false);
    };

    const handleCreate = async () => {
        if (!newEntityName.trim()) return;
        const name = newEntityName.trim();
        await createEntityType(name);
        await loadAll();
        setSelectedState(name);
        closeModal();
    };

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
    const selected = selectedState ?? (entities.length ? entities[0].id : null);

    if (loading) {
        return (
            <Container className="py-5 text-center">
                <Spinner animation="border" />
            </Container>
        );
    }

    const selectedEntity = entities.find(e => e.id === selected) || null;

    const handleDropSuccess = async () => {
        await loadAll();

        const updated = entities.filter(e => e.id !== selected);

        if (updated.length > 0) {
            setSelectedState(updated[0].id);
        } else {
            setSelectedState(null);
        }
    };

    return (
        <Container fluid className="py-4 entity-types-layout">
            <Row className="g-4">
                <Col xs={3}>
                    <Card className="entity-sidebar">
                        <Card.Header className="text-warning fw-bold">
                            Entities
                            <Button
                                variant="primary"
                                className="create-entity-type-button"
                                onClick={openModal}
                            >
                                +
                            </Button>
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
                    {selectedEntity && (
                        <EntityTypes
                            entityType={selectedEntity.id}
                            onDropSuccess={handleDropSuccess}
                        />
                    )}
                </Col>

            </Row>

            <Modal show={showModal} onHide={closeModal} centered>
                <Modal.Header closeButton>
                    <Modal.Title>Create Entity Type</Modal.Title>
                </Modal.Header>
                <Modal.Body>
                    <Form.Control
                        type="text"
                        placeholder="Entity name"
                        value={newEntityName}
                        onChange={(e) => setNewEntityName(e.target.value)}
                    />
                </Modal.Body>
                <Modal.Footer>
                    <Button variant="secondary" onClick={closeModal}>
                        Cancel
                    </Button>
                    <Button variant="primary" onClick={handleCreate}>
                        Create
                    </Button>
                </Modal.Footer>
            </Modal>
        </Container>
    );
}
