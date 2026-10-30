import {useCallback, useState} from "react";
import {apiFetch} from "../../utils/api";
import {useToast} from "../notification/useToast.js";
import {useTranslation} from "../i18n/useTranslation.js";

export function useUsers() {
    const { show } = useToast();
    const { t } = useTranslation();
    const [list, setList] = useState([]);
    const [loading, setLoading] = useState(true);
    const [error, setError] = useState(null);

    const loadAll = useCallback(() => {
        setLoading(true);

        apiFetch("/api/security/user")
            .then((response) => {
                setList(response.users);
                setLoading(false);
            })
            .catch((err) => {
                setError(err);
                setLoading(false);
            });
    }, []);

    const createUser = useCallback(async (user) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user`, {
                method: "POST",
                json: user,
            });
            show("success", t("users.created"));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("users.createFailed"));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    const changeUserPassword = useCallback(async (username, password) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user/`+username+`/password`, {
                method: "PUT",
                json: {password: password},
            });
            show("success", t("users.passwordChanged"));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("users.passwordChangeFailed"));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    const changeUserRole = useCallback(async (username, role) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user/`+username+`/role`, {
                method: "PUT",
                json: {role: role},
            });
            show("success", t("users.roleChanged"));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("users.roleChangeFailed"));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    const deleteUser = useCallback(async (username, password) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user/`+username, {
                method: "DELETE",
                json: {password: password},
            });
            show("success", t("users.deleted"));
        } catch (err) {
            setError(err?.message ?? t("common.unknownError"));
            show("error", t("users.deleteFailed"));
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show, t]);

    return { list, loading, error, loadAll, deleteUser, createUser, changeUserPassword, changeUserRole };
}
