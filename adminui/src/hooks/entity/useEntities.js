import { useCallback, useState } from "react";
import { apiFetch } from "../../utils/api";
import {useToast} from "../notification/useToast.js";
import {useTranslation} from "../i18n/useTranslation.js";

export function useEntities() {
    const { show } = useToast();
    const { t } = useTranslation();
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
            show("success", t("browse.deleted", { id: entityId }));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("browse.deleteFailed", { id: entityId }));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    return { list, loading, error, loadAll, dropEntity };
}