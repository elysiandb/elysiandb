import { Spinner, Form } from "react-bootstrap";
import { useEffect, useState } from "react";

export default function ACLCell({ entity, permissions, onChange }) {
    const [current, setCurrent] = useState(null);
    const [initial, setInitial] = useState(null);

    useEffect(() => {
        if (permissions) {
            setCurrent(permissions);
            setInitial(permissions);
        }
    }, [permissions]);

    if (!current) {
        return null;
    }

    const toggle = (key) => {
        const updated = { ...current, [key]: !current[key] };
        setCurrent(updated);
        onChange(entity, updated, initial);
    };

    return (
        <Form className="d-flex flex-column gap-1">
            {Object.entries(current).map(([permission, enabled]) => (
                <Form.Check
                    key={permission}
                    type="checkbox"
                    label={permission}
                    checked={enabled}
                    onChange={() => toggle(permission)}
                />
            ))}
        </Form>
    );
}
