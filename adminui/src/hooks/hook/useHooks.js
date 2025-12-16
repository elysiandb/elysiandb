import {useCallback, useState} from "react";
import {apiFetch} from "../../utils/api";
import {useToast} from "../notification/useToast.js";

export function useHooks() {
    const { show } = useToast();
    const [list, setList] = useState([]);
    const [hook, setHook] = useState(null);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    const loadAllForEntity = useCallback((entity) => {
        setLoading(true);

        apiFetch("/api/hook/" + entity)
            .then((response) => {
                setList(response);
                setLoading(false);
            })
            .catch((err) => {
                setError(err);
                setLoading(false);
            });
    }, []);

    const loadOne = useCallback((id) => {
        setLoading(true);

        apiFetch("/api/hook/id/" + id)
            .then((response) => {
                setHook(response);
                setLoading(false);
            })
            .catch((err) => {
                setError(err);
                setLoading(false);
            });
    }, []);

    const create = useCallback( async (entity, event, name) => {
        setLoading(true);

        try {
            await apiFetch(`/api/hook/${entity}`, {
                method: "POST",
                json: {
                    name,
                    entity,
                    event,
                    priority: 1,
                    script: "",
                    language: "javascript",
                    bypass_acl: true,
                    enabled: false,
                },
            });
            show("success", `The hook "${name}" was created for "${entity}"`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to create a hook for "${entity}"`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    const update = useCallback(async (hook) => {
        setLoading(true);

        try {
            await apiFetch(`/api/hook/id/${hook.id}`, {
                method: "PUT",
                json: hook,
            });
            show("success", `Hook "${hook.name}" saved`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", "Failed to save hook");
            throw err;
        } finally {
            setLoading(false);
        }
    }, [show]);

    const refreshSelected = useCallback(async (id, entity) => {
        setLoading(true);
        try {
            const [hooks, current] = await Promise.all([
                apiFetch("/api/hook/" + entity),
                apiFetch("/api/hook/id/" + id),
            ]);
            setList(hooks);
            setHook(current);
        } catch (err) {
            setError(err);
        } finally {
            setLoading(false);
        }
    }, []);

    return { list, hook, loading, error, loadAllForEntity, loadOne, create, update, refreshSelected };
}
