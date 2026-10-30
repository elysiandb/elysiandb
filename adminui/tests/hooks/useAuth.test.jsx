import { describe, expect, it, beforeEach } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { AuthStatus, useAuth } from "../../src/hooks/account/useAuth.ts";
import { useAccountStore } from "../../src/account/store.ts";
import { mockApi, requestsTo } from "../helpers.jsx";

describe("useAuth logout", () => {
    beforeEach(() => {
        useAccountStore.setState({ account: { id: 1, username: "admin" } });
    });

    it("sends a POST request and clears the account", async () => {
        const fetchMock = mockApi({
            "POST /api/security/logout": () => new Response(null, { status: 204 }),
        });
        const { result } = renderHook(() => useAuth());

        expect(result.current.status).toBe(AuthStatus.Authenticated);

        act(() => result.current.logout());

        await waitFor(() => expect(result.current.status).toBe(AuthStatus.Guest));
        expect(requestsTo(fetchMock, "POST", "/api/security/logout")).toHaveLength(1);
        expect(requestsTo(fetchMock, "GET", "/api/security/logout")).toHaveLength(0);
        expect(useAccountStore.getState().account).toBeNull();
    });

    it("clears the account when the logout request fails", async () => {
        const fetchMock = mockApi({
            "POST /api/security/logout": () => new Response(null, { status: 500 }),
        });
        const { result } = renderHook(() => useAuth());

        act(() => result.current.logout());

        await waitFor(() => expect(result.current.status).toBe(AuthStatus.Guest));
        await new Promise((resolve) => setTimeout(resolve, 0));

        expect(requestsTo(fetchMock, "POST", "/api/security/logout")).toHaveLength(1);
        expect(useAccountStore.getState().account).toBeNull();
    });
});
