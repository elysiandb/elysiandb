import { vi } from "vitest";

export function mockApi(routes) {
    const fetchMock = vi.fn(async (url, options = {}) => {
        const { pathname } = new URL(url);
        const handler = routes[`${options.method ?? "GET"} ${pathname}`];

        if (handler === undefined) {
            return new Response(null, { status: 404 });
        }

        const body = typeof handler === "function" ? handler(options) : handler;

        if (body instanceof Response) {
            return body;
        }

        return new Response(JSON.stringify(body), {
            status: 200,
            headers: { "content-type": "application/json" },
        });
    });

    vi.stubGlobal("fetch", fetchMock);

    return fetchMock;
}

export function requestsTo(fetchMock, method, pathname) {
    return fetchMock.mock.calls.filter(([url, options = {}]) =>
        new URL(url).pathname === pathname && (options.method ?? "GET") === method
    );
}
