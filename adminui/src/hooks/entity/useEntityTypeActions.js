import { useCallback, useState } from "react";
import { apiFetch } from "../../utils/api";
import {useToast} from "../notification/useToast.js";

export function useEntityTypeActions() {
    const { show } = useToast();
    const [loading, setLoading] = useState(null);
    const [error, setError] = useState(null);

    const createEntityType = useCallback(async (entityId) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}/create`, {
                method: "POST",
                json: { fields: {} },
            });
            show("success", `Entity "${entityId}" created successfully`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to create entity "${entityId}"`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    const dropEntityType = useCallback(async (entityId) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}`, { method: "DELETE" });
            show("success", `Entity "${entityId}" deleted`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to delete "${entityId}"`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    const updateEntitySchema = useCallback(async (entityId, schema) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}/schema`, {
                method: "PUT",
                json: { fields: schema }
            });
            show("success", `Schema updated for "${entityId}"`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to update schema for "${entityId}"`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    return { dropEntityType, updateEntitySchema, createEntityType, loading, error };
}
