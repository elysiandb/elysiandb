import {useCallback, useState} from "react";
import {apiFetch} from "../../utils/api";

export function useEntityTypeActions() {
    const [loading, setLoading] = useState(null);
    const [error, setError] = useState(null);

    const dropEntityType = useCallback(async (entityId) => {
        if (!confirm(`Drop entity type "${entityId}" ?`)) return;

        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}`, { method: "DELETE" });
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            throw err;
        } finally {
            setLoading(null);
        }
    }, []);

    const updateEntitySchema = useCallback(async (entityId, schema) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}/schema`, {
                method: "PUT",
                json: { fields: schema }
            });
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            throw err;
        } finally {
            setLoading(null);
        }
    }, []);

    return { dropEntityType, updateEntitySchema, loading, error };
}
