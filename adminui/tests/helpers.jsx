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
        const parsed = new URL(url);
        const handler = routes[`${options.method ?? "GET"} ${parsed.pathname}`];

        if (handler === undefined) {
            return new Response(null, { status: 404 });
        }

        const body = typeof handler === "function" ? handler(options, parsed) : handler;

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

const BREAKPOINTS = ["xs", "sm", "md", "lg", "xl", "xxl"];

function matchSpan(element, prefix, breakpoint) {
    const infix = breakpoint === "xs" ? "" : `-${breakpoint}`;
    const pattern = new RegExp(`^${prefix}${infix}-(\\d+)$`);
    const match = [...element.classList].map((name) => name.match(pattern)).find(Boolean);
    return match ? Number(match[1]) : null;
}

function columnSpan(column, row, breakpoint) {
    for (let index = BREAKPOINTS.indexOf(breakpoint); index >= 0; index--) {
        const own = matchSpan(column, "col", BREAKPOINTS[index]);
        if (own !== null) {
            return own;
        }
        const perRow = matchSpan(row, "row-cols", BREAKPOINTS[index]);
        if (perRow !== null) {
            return 12 / perRow;
        }
    }
    return 12;
}

export function gridLines(row, breakpoint) {
    const lines = [];
    let current = [];
    let width = 0;

    for (const column of row.children) {
        const span = columnSpan(column, row, breakpoint);
        if (width + span > 12) {
            lines.push({ width, columns: current });
            current = [];
            width = 0;
        }
        current.push(column);
        width += span;
    }
    if (current.length > 0) {
        lines.push({ width, columns: current });
    }

    return lines;
}

export function expectFullRows(container = document.body) {
    const rows = [...container.querySelectorAll(".row")].filter((row) =>
        [...row.children].some((column) => column.querySelector(".card"))
    );

    expect(rows.length).toBeGreaterThan(0);
    for (const row of rows) {
        for (const breakpoint of BREAKPOINTS) {
            for (const line of gridLines(row, breakpoint)) {
                expect(line.width, `${breakpoint}: ${line.columns.map((column) => column.textContent.slice(0, 40)).join(" | ")}`).toBe(12);
            }
        }
    }
}
