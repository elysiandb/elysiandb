import { Card, ListGroup, Spinner, Button, Modal, Form, Row, Col } from "react-bootstrap";
import { useEffect, useState } from "react";
import { useHooks } from "../../hooks/hook/useHooks.js";
import HookEditor from "./HookEditor.jsx";

export default function EntityHooksList({ entity, onSelect }) {
    const {
        list,
        hook,
        loading,
        loadAllForEntity,
        loadOne,
        create,
        update,
    } = useHooks();

    const [selectedId, setSelectedId] = useState(null);
    const [showModal, setShowModal] = useState(false);
    const [event, setEvent] = useState("post_read");
    const [name, setName] = useState("");

    useEffect(() => {
        if (!entity) return;
        setSelectedId(null);
        loadAllForEntity(entity);
    }, [entity, loadAllForEntity]);

    useEffect(() => {
        if (hook && onSelect) onSelect(hook);
    }, [hook, onSelect]);

    const handleSelect = (h) => {
        setSelectedId(h.id);
        loadOne(h.id);
    };

    const handleSaveHook = async (data) => {
        await update(data);
        await loadAllForEntity(entity);
        await loadOne(data.id);
    };

    const openModal = () => setShowModal(true);
    const closeModal = () => {
        setShowModal(false);
        setName("");
        setEvent("post_read");
    };

    const handleCreate = async () => {
        if (!name.trim()) return;
        await create(entity, event, name.trim());
        await loadAllForEntity(entity);
        closeModal();
    };

    return (
        <>
            <Row className="g-4">
                <Col xs={3}>
                    <Card className="entity-sidebar">
                        <Card.Header className="d-flex align-items-center justify-content-between">
                            <span className="text-warning fw-bold">Hooks</span>
                            <Button
                                variant="primary"
                                className="create-entity-type-button"
                                onClick={openModal}
                            >
                                +
                            </Button>
                        </Card.Header>

                        {loading ? (
                            <div className="py-4 text-center">
                                <Spinner animation="border" />
                            </div>
                        ) : (
                            <ListGroup variant="flush">
                                {Array.isArray(list) && list.map(h => (
                                    <ListGroup.Item
                                        key={h.id}
                                        onClick={() => handleSelect(h)}
                                        className={
                                            "entity-list-item d-flex justify-content-between align-items-center" +
                                            (selectedId === h.id ? " selected" : "") +
                                            (h.enabled === false ? " opacity-50" : "")
                                        }
                                    >
                                        <span>
                                            {h.priority} - {h.name || h.id}
                                        </span>
                                        {h.enabled === false && (
                                            <span className="badge rounded-pill text-bg-secondary">
                                                OFF
                                            </span>
                                        )}
                                    </ListGroup.Item>
                                ))}
                            </ListGroup>
                        )}
                    </Card>
                </Col>

                <Col xs={9}>
                    {hook && (
                        <HookEditor
                            hook={hook}
                            loading={loading}
                            onSave={handleSaveHook}
                        />
                    )}
                </Col>
            </Row>

            <Modal show={showModal} onHide={closeModal} centered>
                <Modal.Header closeButton>
                    <Modal.Title>Create hook</Modal.Title>
                </Modal.Header>

                <Modal.Body>
                    <Form>
                        <Form.Group className="mb-3">
                            <Form.Label>Event</Form.Label>
                            <Form.Select
                                value={event}
                                onChange={e => setEvent(e.target.value)}
                            >
                                <option value="post_read">post_read</option>
                            </Form.Select>
                        </Form.Group>

                        <Form.Group>
                            <Form.Label>Name</Form.Label>
                            <Form.Control
                                type="text"
                                value={name}
                                onChange={e => setName(e.target.value)}
                            />
                        </Form.Group>
                    </Form>
                </Modal.Body>

                <Modal.Footer>
                    <Button variant="secondary" onClick={closeModal}>
                        Cancel
                    </Button>
                    <Button variant="primary" onClick={handleCreate}>
                        Save
                    </Button>
                </Modal.Footer>
            </Modal>
        </>
    );
}
