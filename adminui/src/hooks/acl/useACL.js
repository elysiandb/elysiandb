import { useCallback, useState } from "react";
import { apiFetch } from "../../utils/api";

export function useACL() {
    const [list, setList] = useState([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    const loadAll = useCallback((username) => {
        setLoading(true);

        apiFetch("/api/acl/" + username)
            .then((response) => {
                setList(response);
                setLoading(false);
            })
            .catch((err) => {
                setError(err);
                setLoading(false);
            });
    }, []);

    const updateACL = useCallback(async (entity, username, permissions) => {
        await apiFetch(`/api/acl/${username}/${entity}`, {
            method: "PUT",
            json: { permissions }
        });
    }, []);

    const restoreACL = useCallback(async (entity, username) => {
        await apiFetch(`/api/acl/${username}/${entity}/default`, {
            method: "PUT",
        });
    }, []);

    return { list, loading, error, loadAll, updateACL, restoreACL };
}
