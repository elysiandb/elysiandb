import {useCallback, useState} from "react";
import {apiFetch} from "../../utils/api";
import {useToast} from "../notification/useToast.js";

export function useUsers() {
    const { show } = useToast();
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

    // const loadOne = useCallback((entityType) => {
    //     setLoading(true);
    //
    //     apiFetch("/api/" + entityType + "/schema")
    //         .then((response) => {
    //             setEntityTypeData(response);
    //
    //             setLoading(false);
    //         })
    //         .catch((err) => {
    //             setError(err);
    //             setLoading(false);
    //         });
    // }, []);

    const createUser = useCallback(async (user) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user`, {
                method: "POST",
                json: user,
            });
            show("success", `User created successfully`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to create user`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    const changeUserPassword = useCallback(async (username, password) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user/`+username+`/password`, {
                method: "PUT",
                json: {password: password},
            });
            show("success", `User's paswword successfully modified`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to change user's password`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    const deleteUser = useCallback(async (username, password) => {
        setLoading(true);

        try {
            await apiFetch(`/api/security/user/`+username, {
                method: "DELETE",
                json: {password: password},
            });
            show("success", `User paswword successfully deleted`);
        } catch (err) {
            setError(err?.message ?? "Unknown error");
            show("error", `Failed to delete user`);
            throw err;
        } finally {
            setLoading(null);
        }
    }, [show]);

    return { list, loading, error, loadAll, deleteUser, createUser, changeUserPassword };
}
