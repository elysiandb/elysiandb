import { render } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { expect, vi } from "vitest";
import { ToastProvider } from "../src/components/notification/ToastProvider.jsx";
import { useLanguageStore } from "../src/i18n/store.js";
import en from "../src/i18n/locales/en.json";

export function renderWithProviders(ui, { language = "en" } = {}) {
    useLanguageStore.setState({ language });

    return render(
        <ToastProvider>
            <MemoryRouter>{ui}</MemoryRouter>
        </ToastProvider>
    );
}

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

const rawKeyPattern = new RegExp(`\\b(${Object.keys(en).join("|")})\\.[a-zA-Z_]+`);

export function expectNoRawTranslationKeys(container = document.body) {
    const text = [
        container.textContent,
        ...[...container.querySelectorAll("[placeholder], [aria-label], [title]")].flatMap((element) => [
            element.getAttribute("placeholder"),
            element.getAttribute("aria-label"),
            element.getAttribute("title"),
        ]),
    ].filter(Boolean).join("\n");

    expect(text).not.toMatch(rawKeyPattern);
}
