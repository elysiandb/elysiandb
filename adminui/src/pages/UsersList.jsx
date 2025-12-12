import { Button, Container, Row, Spinner, Table, Form } from "react-bootstrap";
import { useEffect, useMemo, useState } from "react";
import { useUsers } from "../hooks/user/useUsers.js";
import UserLine from "../components/user/UserLine.jsx";
import UserModal from "../components/user/UserModal.jsx";

export default function UsersList() {
    const { list, loadAll, loading, deleteUser } = useUsers();
    const [showModal, setShowModal] = useState(false);
    const [selectedUser, setSelectedUser] = useState(null);
    const [search, setSearch] = useState("");

    useEffect(() => {
        loadAll();
    }, [loadAll]);

    const openCreate = () => {
        setSelectedUser(null);
        setShowModal(true);
    };

    const changePassword = (user) => {
        setSelectedUser(user);
        setShowModal(true);
    };

    const handleDelete = async (username) => {
        if (!window.confirm(`Delete user "${username}" ?`)) return;
        await deleteUser(username);
        loadAll();
    };

    const filteredUsers = useMemo(() => {
        if (!search.trim()) return list;
        const q = search.toLowerCase();
        return list.filter((u) => u.username.toLowerCase().includes(q));
    }, [list, search]);

    if (loading) {
        return (
            <Container className="py-5 text-center">
                <Spinner animation="border" />
            </Container>
        );
    }

    return (
        <Container fluid className="py-4 entity-types-layout">
            <Row className="align-items-center mb-3">
                <div className="d-flex justify-content-between align-items-center w-100">
                    <Button size="sm" onClick={openCreate}>
                        Create a user
                    </Button>

                    <Form.Control
                        placeholder="Search user…"
                        value={search}
                        onChange={(e) => setSearch(e.target.value)}
                        className="users-search-input"
                    />
                </div>
            </Row>

            <Row className="g-4 mt-1">
                <Table className="users-table align-middle">
                    <thead>
                    <tr>
                        <th>Username</th>
                        <th>Role</th>
                        <th>Actions</th>
                    </tr>
                    </thead>
                    <tbody>
                    {filteredUsers.map((user) => (
                        <UserLine
                            key={user.username}
                            user={user}
                            onChangePassword={changePassword}
                            onDeleted={() => handleDelete(user.username)}
                        />
                    ))}
                    </tbody>
                </Table>
            </Row>

            <UserModal
                showModal={showModal}
                user={selectedUser}
                onHide={() => setShowModal(false)}
                onSaved={loadAll}
            />
        </Container>
    );
}
