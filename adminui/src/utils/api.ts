export async function apiFetch<T>(
    url: string,
    { json, method }: { json?: Record<string, unknown>; method?: string } = {}
): Promise<T> {
    const base = `${window.location.protocol}//${window.location.hostname}:${window.location.port}`;
    method ??= json ? "POST" : "GET";
    const body = json ? JSON.stringify(json) : undefined;

    const r = await fetch(base + url, {
        method,
        credentials: "include",
        body,
        headers: {
            accept: "application/json",
            "content-type": "application/json",
        },
    });

    if (r.status === 204) {
        return undefined as T;
    }

    if (r.ok) {
        const text = await r.text();
        return text ? (JSON.parse(text) as T) : (undefined as T);
    }

    let data: any = null;
    try {
        const text = await r.text();
        data = text ? JSON.parse(text) : null;
    } catch {
        data = null;
    }

    throw new ApiError(r.status, data || {});
}

class ApiError extends Error {
    constructor(public status: number, public data: Record<string, unknown>) {
        if (status === 401) {
            localStorage.removeItem("account");
            window.location.reload();
        }
        super();
    }
}
