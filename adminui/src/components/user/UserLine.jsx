import { Button } from "react-bootstrap";

export default function UserLine({ user, onChangePassword, onDeleted }) {
    return (
        <tr>
            <td>{user.username}</td>
            <td>{user.role}</td>
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
