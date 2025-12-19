import { useCallback, useState } from "react";
import { apiFetch } from "../../utils/api";
import {useToast} from "../notification/useToast.js";

export function useEntities() {
    const { show } = useToast();
    const [list, setList] = useState([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    const loadAll = useCallback((query) => {
        setLoading(true);
        setError(null);
        setList([]);

        apiFetch(`/api/query`, { method: "POST", json: query })
            .then(setList)
            .catch(setError)
            .finally(() => setLoading(false));
    }, []);

    const dropEntity = useCallback(async (entity, entityId) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entity}/${entityId}`, { method: "DELETE" });
            show("success", `Entity "${entityId}" deleted`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to delete "${entityId}"`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    return { list, loading, error, loadAll, dropEntity };
}