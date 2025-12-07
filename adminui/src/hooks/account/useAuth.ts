import { useCallback } from "react";
import { useAccountStore } from "../../account/store";
import { apiFetch } from "../../utils/api";
import { Account } from "../../account/Account";

export enum AuthStatus {
    Unknown,
    Authenticated,
    Guest,
}

export function useAuth() {
    const { account, setAccount } = useAccountStore();
    let status;
    switch (account) {
        case null:
            status = AuthStatus.Guest;
            break;
        case undefined:
            status = AuthStatus.Unknown;
            break;
        default:
            status = AuthStatus.Authenticated;
            break;
    }

    const authenticate = useCallback(() => {
        apiFetch<Account>("/admin/me")
            .then(setAccount)
            .catch(() => setAccount(null));
    }, []);

    const login = useCallback((username: string, password: string) => {
        apiFetch<Account>("/admin/login", { method: "POST", json: { username, password } })
            .then(setAccount)
            .catch(() => setAccount(null));
    }, []);

    const logout = useCallback(() => {
        apiFetch("/admin/logout", { method: "GET" })
            .finally(() => setAccount(null));
    }, []);

    return {
        account,
        status,
        authenticate,
        login,
        logout,
    };
}
