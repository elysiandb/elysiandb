import { Button, Form } from "react-bootstrap";
import { useState } from "react";

export default function UserLine({ user, onChangePassword, onDeleted, onChangeRole }) {
    const [role, setRole] = useState(user.role);
    const [pending, setPending] = useState(false);

    const handleChange = async (e) => {
        const nextRole = e.target.value;
        const prevRole = role;

        setRole(nextRole);
        setPending(true);

        try {
            await onChangeRole(user.username, nextRole);
        } catch {
            setRole(prevRole);
        } finally {
            setPending(false);
        }
    };

    return (
        <tr>
            <td>{user.username}</td>
            <td>
                <Form.Select
                    size="sm"
                    value={role}
                    disabled={pending}
                    onChange={handleChange}
                >
                    <option value="admin">admin</option>
                    <option value="user">user</option>
                </Form.Select>
            </td>
            <td className="table-actions">
                <Button size="sm" onClick={() => onChangePassword(user)}>
                    Change password
                </Button>
                <Button
                    size="sm"
                    variant="danger"
                    onClick={onDeleted}
                >
                    Delete
                </Button>
            </td>
        </tr>
    );
}
