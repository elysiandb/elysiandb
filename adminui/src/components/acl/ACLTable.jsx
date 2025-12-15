import { Container, Spinner, Table } from "react-bootstrap";
import { useEffect, useMemo, useState } from "react";
import ACLCell from "./ACLCell.jsx";
import { useACL } from "../../hooks/acl/useACL.js";

const isEqual = (a, b) =>
    JSON.stringify(a) === JSON.stringify(b);

export default function ACLTable({ username, entities, onUpdate }) {
    const { list, loadAll, loading } = useACL();
    const [changes, setChanges] = useState({});
    const [showSpinner, setShowSpinner] = useState(false);

    useEffect(() => {
        loadAll(username);
        setChanges({});
        onUpdate({});
    }, [username, loadAll, onUpdate]);

    useEffect(() => {
        let t;

        if (loading) {
            t = setTimeout(() => setShowSpinner(true), 200);
        } else {
            setShowSpinner(false);
        }

        return () => clearTimeout(t);
    }, [loading]);

    const aclByEntity = useMemo(() => {
        if (!Array.isArray(list)) return {};
        return list.reduce((acc, acl) => {
            acc[acl.entity] = acl.permissions;
            return acc;
        }, {});
    }, [list]);

    const handleChange = (entity, current, initial) => {
        setChanges(prev => {
            const next = { ...prev };

            if (isEqual(current, initial)) {
                delete next[entity];
            } else {
                next[entity] = current;
            }

            onUpdate(next);
            return next;
        });
    };

    if (loading && showSpinner) {
        return (
            <Container className="py-5 text-center" style={{ minHeight: "348.9px" }}>
                <Spinner animation="border" />
            </Container>
        );
    } else if (loading) {
        return (
            <Container className="py-5 text-center" style={{ minHeight: "348.9px" }}>
            </Container>
        );
    }

    return (
        <Table className="users-table align-middle">
            <thead>
            <tr>
                {entities.map(entity => (
                    <th key={entity}>{entity}</th>
                ))}
            </tr>
            </thead>
            <tbody>
            <tr>
                {entities.map(entity => (
                    <td key={entity}>
                        <ACLCell
                            entity={entity}
                            permissions={aclByEntity[entity]}
                            onChange={handleChange}
                        />
                    </td>
                ))}
            </tr>
            </tbody>
        </Table>
    );
}
