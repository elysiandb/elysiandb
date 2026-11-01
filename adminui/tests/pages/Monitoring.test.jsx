import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { describe, expect, it } from "vitest";
import Monitoring from "../../src/pages/Monitoring.jsx";
import { ToastProvider } from "../../src/components/notification/ToastProvider.jsx";
import { useLanguageStore } from "../../src/i18n/store.js";
import { expectNoRawTranslationKeys, mockApi, requestsTo } from "../helpers.jsx";
import { OVERVIEW, series } from "../fixtures/monitoring.js";

const DETAILS = {
    routes: {
        since_startup: [{ method: "GET", route: "/api/{entity}", count: 1200, errors: 3, error_rate: 0.0025, p95_ms: 0.5 }],
        last_15m: [{ method: "POST", route: "/api/query", count: 40, errors: 0, error_rate: 0, p95_ms: 2.1 }],
    },
    hooks: [{ hook_id: "7f3c", entity: "books", phase: "post_read", executions: 52, errors: 2, avg_ms: 0.4, p95_ms: 1.1, last_error: "TypeError", last_error_at: null }],
    entity_types: [{ name: "books", count: 21004 }],
};

function LocationProbe() {
    const location = useLocation();
    return <output data-testid="location">{location.search}</output>;
}

function renderMonitoring(url = "/admin/monitoring", language = "en") {
    useLanguageStore.setState({ language });

    return render(
        <ToastProvider>
            <MemoryRouter initialEntries={[url]}>
                <Routes>
                    <Route path="/admin/monitoring" element={<><Monitoring /><LocationProbe /></>} />
                </Routes>
            </MemoryRouter>
        </ToastProvider>
    );
}

function routes(overview = OVERVIEW) {
    return {
        "GET /monitoring/overview": overview,
        "GET /monitoring/series": (options, url) => series(url.searchParams.get("range")),
        "GET /monitoring/details": DETAILS,
    };
}

describe("Monitoring", () => {
    it("shows every section with its charts and tables", async () => {
        mockApi(routes());
        renderMonitoring();

        expect(await screen.findByRole("heading", { name: "Traffic" })).toBeInTheDocument();
        for (const section of ["Latency", "Errors and security", "KV store", "Documents", "Persistence", "Runtime", "Connections", "Hooks"]) {
            expect(screen.getByRole("heading", { name: section })).toBeInTheDocument();
        }
        expect(screen.queryByRole("heading", { name: "MongoDB" })).not.toBeInTheDocument();
        expect(screen.getByRole("img", { name: "Requests per second" })).toBeInTheDocument();
        expect(screen.getByRole("img", { name: "HTTP latency" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "/api/{entity}" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "/api/query" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "21,004" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "Post-read" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "15 min" })).toHaveAttribute("aria-pressed", "true");
        expectNoRawTranslationKeys();
    });

    it("shows the MongoDB section with the MongoDB engine", async () => {
        mockApi(routes({ ...OVERVIEW, instance: { ...OVERVIEW.instance, engine: "mongodb" } }));
        renderMonitoring();

        expect(await screen.findByRole("heading", { name: "MongoDB" })).toBeInTheDocument();
        expect(screen.getByRole("img", { name: "MongoDB ping" })).toBeInTheDocument();
    });

    it("changes the time range and keeps it in the URL", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi(routes());
        renderMonitoring();

        await screen.findByRole("heading", { name: "Traffic" });
        await user.click(screen.getByRole("button", { name: "24 hr" }));

        expect(screen.getByTestId("location")).toHaveTextContent("range=24h");
        await waitFor(() => {
            const ranges = requestsTo(fetchMock, "GET", "/monitoring/series").map(([url]) => new URL(url).searchParams.get("range"));
            expect(ranges).toContain("24h");
        });
        expect(screen.getByRole("combobox", { name: "Refresh interval" })).toHaveValue("60000");
    });

    it("uses a 2 second refresh for the live range", async () => {
        mockApi(routes());
        renderMonitoring("/admin/monitoring?range=15m");

        await screen.findByRole("heading", { name: "Traffic" });
        expect(screen.getByRole("combobox", { name: "Refresh interval" })).toHaveValue("2000");
    });

    it("is displayed in French", async () => {
        mockApi(routes());
        renderMonitoring("/admin/monitoring?range=1h", "fr");

        expect(await screen.findByRole("heading", { name: "Supervision" })).toBeInTheDocument();
        expect(screen.getByRole("heading", { name: "Erreurs et sécurité" })).toBeInTheDocument();
        expect(screen.getByRole("img", { name: "Requêtes par seconde" })).toBeInTheDocument();
        expect(screen.getByRole("combobox", { name: "Intervalle de rafraîchissement" })).toHaveValue("30000");
        expectNoRawTranslationKeys();
    });
});
