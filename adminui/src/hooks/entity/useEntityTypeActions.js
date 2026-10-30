import { useCallback, useState } from "react";
import { apiFetch } from "../../utils/api";
import {useToast} from "../notification/useToast.js";
import {useTranslation} from "../i18n/useTranslation.js";

export function useEntityTypeActions() {
    const { show } = useToast();
    const { t } = useTranslation();
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
            show("success", t("schema.created", { entity: entityId }));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("schema.createFailed", { entity: entityId }));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    const dropEntityType = useCallback(async (entityId) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}`, { method: "DELETE" });
            show("success", t("schema.dropped", { entity: entityId }));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("schema.dropFailed", { entity: entityId }));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    const updateEntitySchema = useCallback(async (entityId, schema) => {
        setLoading(entityId);
        setError(null);

        try {
            await apiFetch(`/api/${entityId}/schema`, {
                method: "PUT",
                json: { fields: schema }
            });
            show("success", t("schema.updated", { entity: entityId }));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("schema.updateFailed", { entity: entityId }));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    return { dropEntityType, updateEntitySchema, createEntityType, loading, error };
}
