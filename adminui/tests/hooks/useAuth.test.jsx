import { describe, expect, it, vi } from "vitest";
import { act, renderHook, waitFor } from "@testing-library/react";
import { AuthStatus, useAuth } from "../../src/hooks/account/useAuth.ts";
import { mockApi, requestsTo } from "../helpers.jsx";

async function logoutAndExpectGuest() {
    const { result } = renderHook(() => useAuth());

    expect(result.current.status).toBe(AuthStatus.Authenticated);

    act(() => result.current.logout());

    await waitFor(() => expect(result.current.status).toBe(AuthStatus.Guest));
    await new Promise((resolve) => setTimeout(resolve, 0));
}

describe("useAuth logout", () => {
    it("ends the server session with a POST request", async () => {
        const fetchMock = mockApi({
            "POST /api/security/logout": () => new Response(null, { status: 204 }),
        });

        await logoutAndExpectGuest();

        expect(requestsTo(fetchMock, "POST", "/api/security/logout")).toHaveLength(1);
        expect(requestsTo(fetchMock, "GET", "/api/security/logout")).toHaveLength(0);
    });

    it.each([
        ["the server fails", () => new Response(null, { status: 500 })],
        ["the server cannot be reached", () => {
            throw new TypeError("Failed to fetch");
        }],
    ])("signs out locally when %s", async (_, handler) => {
        const fetchMock = mockApi({ "POST /api/security/logout": handler });

        await logoutAndExpectGuest();

        expect(requestsTo(fetchMock, "POST", "/api/security/logout")).toHaveLength(1);
    });

    it("signs out locally and reloads when the session has already expired", async () => {
        const reload = vi.fn();
        vi.stubGlobal("location", { ...window.location, reload });
        const fetchMock = mockApi({
            "POST /api/security/logout": () => new Response(null, { status: 401 }),
        });

        await logoutAndExpectGuest();

        expect(requestsTo(fetchMock, "POST", "/api/security/logout")).toHaveLength(1);
        expect(reload).toHaveBeenCalledOnce();
    });
});
