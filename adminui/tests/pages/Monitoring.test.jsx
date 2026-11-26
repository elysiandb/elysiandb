import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { describe, expect, it } from "vitest";
import uPlot from "uplot";
import Monitoring from "../../src/pages/Monitoring.jsx";
import { ToastProvider } from "../../src/components/notification/ToastProvider.jsx";
import { useLanguageStore } from "../../src/i18n/store.js";
import { expectFullRows, expectNoRawTranslationKeys, mockApi, requestsTo } from "../helpers.jsx";
import { OVERVIEW, series } from "../fixtures/monitoring.js";

const DETAILS = {
    routes: {
        since_startup: [{ method: "GET", route: "/api/{entity}", count: 1200, errors: 3, error_rate: 0.0025, p95_ms: 0.5 }],
        last_15m: [{ method: "POST", route: "/api/query", count: 40, errors: 0, error_rate: 0, p95_ms: 2.1 }],
    },
    hooks: [{ hook_id: "7f3c", entity: "books", phase: "post_read", executions: 52, errors: 2, avg_ms: 0.4, p95_ms: 1.1, last_error: "TypeError", last_error_at: null }],
    entity_types: [{ name: "books", count: 21004 }],
    indexes: [
        { entity: "books", index: "b:status", kind: "bitmap", origin: "advisor", state: "ready", entries: 21004, bytes: 40960, uses: 912, last_used_at: null },
        { entity: "books", index: "f:author_id", kind: "ordered", origin: "hook", pinned: true, state: "ready", entries: 63012, bytes: 2097152, uses: 15, last_used_at: null },
        { entity: "books", index: "c:genre,title", kind: "composite", origin: "advisor", state: "building", entries: 0, last_used_at: null },
    ],
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
    it("opens on the traffic tab with its charts and routes", async () => {
        mockApi(routes());
        renderMonitoring();

        expect(await screen.findByRole("tab", { name: "Traffic" })).toHaveAttribute("aria-selected", "true");
        expect(screen.getAllByRole("tab").map((tab) => tab.textContent)).toEqual([
            "Traffic", "Errors and security", "Data", "Persistence", "System", "Hooks",
        ]);
        expect(screen.getByRole("tabpanel", { name: "Traffic" })).toBeInTheDocument();
        for (const chart of ["Requests per second", "HTTP responses by status", "TCP commands per second", "HTTP latency", "TCP latency", "Slow requests per minute"]) {
            expect(screen.getByRole("img", { name: chart })).toBeInTheDocument();
        }
        expect(screen.getByRole("cell", { name: "/api/{entity}" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "/api/query" })).toBeInTheDocument();
        expect(screen.queryByRole("img", { name: "Memory" })).not.toBeInTheDocument();
        expect(screen.queryByRole("heading", { level: 3 })).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "15 min" })).toHaveAttribute("aria-pressed", "true");
        expectNoRawTranslationKeys();
    });

    it("switches tabs and keeps the tab in the URL", async () => {
        const user = userEvent.setup();
        mockApi(routes());
        renderMonitoring("/admin/monitoring?range=1h");

        await user.click(await screen.findByRole("tab", { name: "Data" }));

        expect(screen.getByTestId("location")).toHaveTextContent("range=1h&tab=data");
        expect(screen.getByRole("tab", { name: "Data" })).toHaveAttribute("aria-selected", "true");
        expect(screen.getByRole("tab", { name: "Traffic" })).toHaveAttribute("aria-selected", "false");
        expect(screen.getByRole("heading", { name: "KV store" })).toBeInTheDocument();
        expect(screen.getByRole("heading", { name: "Documents" })).toBeInTheDocument();
        expect(screen.getByRole("img", { name: "API cache hit ratio" })).toBeInTheDocument();
        expect(screen.getAllByRole("cell", { name: "21,004" }).length).toBeGreaterThan(0);
        expect(screen.queryByRole("img", { name: "Requests per second" })).not.toBeInTheDocument();
    });

    it("lists the indexes with their form, origin, state and size", async () => {
        mockApi(routes());
        renderMonitoring("/admin/monitoring?tab=data");

        expect(await screen.findByRole("heading", { name: "Indexes" })).toBeInTheDocument();
        expect(screen.getByRole("img", { name: "Index size" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "b:status" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "Bitmap" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "Hook · pinned" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "Building" })).toBeInTheDocument();
        expect(screen.getByRole("cell", { name: "Composite" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it.each([
        ["errors", ["Server errors (5xx) per second", "Authentication failures per minute", "Hook errors per minute", "Warnings and errors logged per minute"], []],
        ["persistence", ["Flush duration", "Failed and forced flushes per minute", "Log size", "Data folder size"], ["Last successful flush"]],
        ["system", ["Memory", "Process threads", "Process CPU (% of all threads)", "Busy CPU threads", "Open file descriptors", "Memory against the budget", "Memory by use", "Memory pressure (0 normal, 1 high, 2 critical)", "Requests refused for memory per minute", "Open connections", "Admin sessions", "Open transactions", "Oldest open transaction"], ["Runtime", "Memory budget", "Connections"]],
        ["hooks", ["Hook executions per second", "Hook duration (p95)", "Reused hook queries and results"], ["Post-read"]],
    ])("opens the %s tab from the URL", async (tab, charts, texts) => {
        mockApi(routes());
        renderMonitoring(`/admin/monitoring?tab=${tab}`);

        expect(await screen.findByRole("img", { name: charts[0] })).toBeInTheDocument();
        for (const chart of charts) {
            expect(screen.getByRole("img", { name: chart })).toBeInTheDocument();
        }
        for (const text of texts) {
            expect(screen.getByText(text)).toBeInTheDocument();
        }
        expect(screen.queryByRole("img", { name: "Requests per second" })).not.toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("draws the runtime charts from the threads, usable CPUs and allocator series", async () => {
        const runtime = {
            allocated_bytes: [100, 110, 120],
            total_bytes: [400, 400, 410],
            threads: [21, 22, 23],
            cpu_cores: [1.5, 1.25, 1],
            usable_cpus: [8, 8, 8],
            memory_budget_bytes: [1000, 1000, 1000],
            memory_live_bytes: [90, 95, 99],
        };
        mockApi({
            ...routes(),
            "GET /monitoring/series": (options, url) => {
                const data = series(url.searchParams.get("range"));
                return { ...data, series: { ...data.series, ...runtime } };
            },
        });
        renderMonitoring("/admin/monitoring?tab=system");

        const chartData = async (name) => {
            const target = await screen.findByRole("img", { name });
            let plot;
            await waitFor(() => {
                plot = uPlot.instances.findLast((instance) => instance.target === target);
                expect(plot?.data?.[0]).toHaveLength(3);
            });
            return plot.data.slice(1);
        };

        expect(await chartData("Memory")).toEqual([runtime.allocated_bytes, runtime.total_bytes]);
        expect(await chartData("Process threads")).toEqual([runtime.threads]);
        expect(await chartData("Busy CPU threads")).toEqual([runtime.cpu_cores, runtime.usable_cpus]);
        expect(await chartData("Memory against the budget")).toEqual([runtime.memory_budget_bytes, runtime.total_bytes, runtime.memory_live_bytes]);
        expect(within(screen.getByRole("region", { name: "Runtime" })).getAllByRole("img").map((chart) => chart.getAttribute("aria-label"))).toEqual([
            "Memory", "Process threads", "Process CPU (% of all threads)", "Busy CPU threads", "Open file descriptors",
        ]);
    });

    it("fills every line of every tab", async () => {
        const user = userEvent.setup();
        mockApi(routes());
        const { container } = renderMonitoring();

        await screen.findByRole("tab", { name: "Traffic" });
        for (const tab of screen.getAllByRole("tab")) {
            await user.click(tab);
            expect(tab).toHaveAttribute("aria-selected", "true");
            expectFullRows(container);
        }
    });

    it("falls back to the traffic tab for an unknown tab", async () => {
        mockApi(routes());
        renderMonitoring("/admin/monitoring?tab=unknown");

        expect(await screen.findByRole("tab", { name: "Traffic" })).toHaveAttribute("aria-selected", "true");
        expect(screen.getByRole("img", { name: "Requests per second" })).toBeInTheDocument();
    });

    it("changes the time range and keeps it in the URL", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi(routes());
        renderMonitoring();

        await screen.findByRole("tab", { name: "Traffic" });
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

        await screen.findByRole("tab", { name: "Traffic" });
        expect(screen.getByRole("combobox", { name: "Refresh interval" })).toHaveValue("2000");
    });

    it("is displayed in French", async () => {
        mockApi(routes());
        renderMonitoring("/admin/monitoring?range=1h", "fr");

        expect(await screen.findByRole("heading", { name: "Supervision" })).toBeInTheDocument();
        expect(await screen.findByRole("tab", { name: "Trafic" })).toHaveAttribute("aria-selected", "true");
        for (const tab of ["Erreurs et sécurité", "Données", "Persistance", "Système", "Hooks"]) {
            expect(screen.getByRole("tab", { name: tab })).toBeInTheDocument();
        }
        expect(screen.getByRole("img", { name: "Requêtes par seconde" })).toBeInTheDocument();
        expect(screen.getByRole("combobox", { name: "Intervalle de rafraîchissement" })).toHaveValue("30000");
        expectNoRawTranslationKeys();
    });
});
