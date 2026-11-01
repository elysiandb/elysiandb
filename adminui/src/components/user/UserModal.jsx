import { Button, Form, Modal } from "react-bootstrap";
import { useEffect, useState } from "react";
import { useUsers } from "../../hooks/user/useUsers.js";
import {useToast} from "../../hooks/notification/useToast.js";
import {useTranslation} from "../../hooks/i18n/useTranslation.js";

const MIN_PASSWORD_LENGTH = 8;

export default function UserModal({ showModal, onHide, user, onSaved }) {
    const { show } = useToast();
    const { createUser, changeUserPassword } = useUsers();
    const { t } = useTranslation();

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
        if (!user && password === "") {
            show("error", t("users.passwordRequired"));
            return;
        }

        if (password !== "" && [...password].length < MIN_PASSWORD_LENGTH) {
            show("error", t("users.passwordTooShort", { min: MIN_PASSWORD_LENGTH }));
            return;
        }

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
                show("error", t("users.passwordRequired"));
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
                <Modal.Title>{user ? t("users.editTitle") : t("users.createTitle")}</Modal.Title>
            </Modal.Header>
            <Modal.Body>
                <Form>
                    <Form.Group className="mb-3">
                        <Form.Label>{t("common.username")}</Form.Label>
                        <Form.Control
                            value={username}
                            disabled={!!user}
                            onChange={(e) => setUsername(e.target.value)}
                        />
                    </Form.Group>

                    <Form.Group className="mb-3">
                        <Form.Label>{t("common.password")}</Form.Label>
                        <Form.Control
                            type="password"
                            value={password}
                            onChange={(e) => setPassword(e.target.value)}
                        />
                    </Form.Group>

                    {!user && (
                        <Form.Group>
                            <Form.Label>{t("common.role")}</Form.Label>
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
                    {t("common.cancel")}
                </Button>
                <Button onClick={handleSave} disabled={saving}>
                    {t("common.save")}
                </Button>
            </Modal.Footer>
        </Modal>
    );
}
