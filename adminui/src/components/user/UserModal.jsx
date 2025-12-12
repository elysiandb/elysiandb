import { Button, Form, Modal } from "react-bootstrap";
import { useEffect, useState } from "react";
import { useUsers } from "../../hooks/user/useUsers.js";
import {useToast} from "../../hooks/notification/useToast.js";

export default function UserModal({ showModal, onHide, user, onSaved }) {
    const { show } = useToast();
    const { createUser, changeUserPassword } = useUsers();

    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");
    const [role, setRole] = useState("user");
    const [saving, setSaving] = useState(false);

    useEffect(() => {
        if (user) {
            setUsername(user.username);
            setRole(user.role);
        } else {
            setUsername("");
            setRole("user");
        }
        setPassword("");
    }, [user, show]);

    const handleSave = async () => {
        setSaving(true);
        try {
            if (user) {
                if (password) {
                    await changeUserPassword(username, password);
                }
            } else {
                await createUser({ username, password, role });
            }

            if (password === "") {
                show("error", `Please enter a password.`);
            } else {
                onSaved();
                onHide();
            }
        } finally {
            setSaving(false);
        }
    };

    return (
        <Modal show={showModal} onHide={onHide} centered>
            <Modal.Header closeButton>
                <Modal.Title>{user ? "Edit user" : "Create user"}</Modal.Title>
            </Modal.Header>
            <Modal.Body>
                <Form>
                    <Form.Group className="mb-3">
                        <Form.Label>Username</Form.Label>
                        <Form.Control
                            value={username}
                            disabled={!!user}
                            onChange={(e) => setUsername(e.target.value)}
                        />
                    </Form.Group>

                    <Form.Group className="mb-3">
                        <Form.Label>Password</Form.Label>
                        <Form.Control
                            type="password"
                            value={password}
                            onChange={(e) => setPassword(e.target.value)}
                        />
                    </Form.Group>

                    {!user && (
                        <Form.Group>
                            <Form.Label>Role</Form.Label>
                            <Form.Select value={role} onChange={(e) => setRole(e.target.value)}>
                                <option value="admin">admin</option>
                                <option value="user">user</option>
                            </Form.Select>
                        </Form.Group>
                    )}
                </Form>
            </Modal.Body>
            <Modal.Footer>
                <Button variant="secondary" onClick={onHide}>
                    Cancel
                </Button>
                <Button onClick={handleSave} disabled={saving}>
                    Save
                </Button>
            </Modal.Footer>
        </Modal>
    );
}
