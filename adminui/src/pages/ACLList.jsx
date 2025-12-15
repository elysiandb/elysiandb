import {
    Button,
    Card,
    Col,
    Container,
    ListGroup,
    Row,
    Form,
    Spinner
} from "react-bootstrap";
import { useEffect, useMemo, useState } from "react";
import { useUsers } from "../hooks/user/useUsers.js";
import { useEntityTypes } from "../hooks/entity/useEntityTypes.js";
import ACLTable from "../components/acl/ACLTable.jsx";
import { useACL } from "../hooks/acl/useACL.js";
import { useToast } from "../hooks/notification/useToast.js";

export default function ACLList() {
    const { list: users, loadAll: loadUsers, loading: loadingUsers } = useUsers();
    const { list: entities, loadAllNames: loadEntities, loading: loadingEntities } = useEntityTypes();
    const { updateACL, restoreACL } = useACL();
    const { show } = useToast();

    const [pending, setPending] = useState({});
    const [selectedUser, setSelectedUser] = useState(null);
    const [searchUser, setSearchUser] = useState("");
    const [searchEntity, setSearchEntity] = useState("");
    const [reloadKey, setReloadKey] = useState(0);

    useEffect(() => {
        loadUsers();
        loadEntities();
    }, [loadUsers, loadEntities]);

    const filteredUsers = useMemo(() => {
        if (!searchUser.trim()) return users;
        const q = searchUser.toLowerCase();
        return users.filter(u => u.username.toLowerCase().includes(q));
    }, [users, searchUser]);

    const filteredEntities = useMemo(() => {
        if (!entities?.entities) return [];
        if (!searchEntity.trim()) return entities.entities;
        const q = searchEntity.toLowerCase();
        return entities.entities.filter(e => e.toLowerCase().includes(q));
    }, [entities, searchEntity]);

    useEffect(() => {
        if (filteredUsers.length && !selectedUser) {
            setSelectedUser(filteredUsers[0].username);
        }
    }, [filteredUsers, selectedUser]);

    if (loadingUsers || loadingEntities) {
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
                            Users
                        </Card.Header>

                        <Form.Control
                            placeholder="Search user…"
                            value={searchUser}
                            onChange={(e) => setSearchUser(e.target.value)}
                            className="user-sidebar-search"
                        />

                        <ListGroup variant="flush">
                            {filteredUsers.map(user => (
                                <ListGroup.Item
                                    key={user.username}
                                    onClick={() => {
                                        setSelectedUser(user.username);
                                        setPending({});
                                        setReloadKey(v => v + 1);
                                    }}
                                    className={
                                        "entity-list-item" +
                                        (selectedUser === user.username ? " selected" : "")
                                    }
                                >
                                    {user.username} ({user.role})
                                </ListGroup.Item>
                            ))}
                        </ListGroup>
                    </Card>
                </Col>

                <Col xs={9}>
                    {selectedUser && (
                        <>
                            <Row className="align-items-center mb-3">
                                <Col>
                                    <Form.Control
                                        placeholder="Search entity…"
                                        value={searchEntity}
                                        onChange={(e) => setSearchEntity(e.target.value)}
                                        className="entity-table-search"
                                    />
                                </Col>
                                <Col xs="auto" className="d-flex gap-2">
                                    <Button
                                        variant="primary"
                                        disabled={!Object.keys(pending).length}
                                        onClick={async () => {
                                            try {
                                                for (const [entity, permissions] of Object.entries(pending)) {
                                                    await updateACL(entity, selectedUser, permissions);
                                                }

                                                show(
                                                    "success",
                                                    `ACLs updated for "${selectedUser}" (${Object.keys(pending).length} entities)`
                                                );

                                                setPending({});
                                                setReloadKey(v => v + 1);
                                            } catch {
                                                show(
                                                    "error",
                                                    `Failed to update ACLs for "${selectedUser}"`
                                                );
                                            }
                                        }}
                                    >
                                        Update ACLs
                                    </Button>
                                    <Button
                                        variant="secondary"
                                        onClick={async () => {
                                            if (!window.confirm(`Restore default permissions for "${selectedUser}" ?`)) {
                                                return;
                                            }

                                            try {
                                                for (const entity of filteredEntities) {
                                                    await restoreACL(entity, selectedUser);
                                                }

                                                show(
                                                    "success",
                                                    `Default ACLs restored for "${selectedUser}"`
                                                );

                                                setPending({});
                                                setReloadKey(v => v + 1);
                                            } catch {
                                                show(
                                                    "error",
                                                    `Failed to restore default ACLs for "${selectedUser}"`
                                                );
                                            }
                                        }}
                                    >
                                        Restore defaults
                                    </Button>
                                </Col>
                            </Row>

                            <div className="acl-table-scroll mb-4">
                                <ACLTable
                                    key={reloadKey}
                                    username={selectedUser}
                                    entities={filteredEntities}
                                    onUpdate={setPending}
                                />
                            </div>

                            <Card>
                                <Card.Header className="text-warning fw-bold">
                                    Permissions overview
                                </Card.Header>
                                <Card.Body className="text-dim">
                                    <Row className="g-3">
                                        <Col md={6}>
                                            <strong className="text-info">read</strong>
                                            <div>Allows reading all records of the entity.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">create</strong>
                                            <div>Allows creating new records in the entity.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">update</strong>
                                            <div>Allows updating any record of the entity.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">delete</strong>
                                            <div>Allows deleting any record of the entity.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">owning_read</strong>
                                            <div>Allows reading only records owned by the user.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">owning_write</strong>
                                            <div>Allows creating records owned by the user.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">owning_update</strong>
                                            <div>Allows updating only records owned by the user.</div>
                                        </Col>
                                        <Col md={6}>
                                            <strong className="text-info">owning_delete</strong>
                                            <div>Allows deleting only records owned by the user.</div>
                                        </Col>
                                    </Row>
                                </Card.Body>
                            </Card>
                        </>
                    )}
                </Col>
            </Row>
        </Container>
    );
}
