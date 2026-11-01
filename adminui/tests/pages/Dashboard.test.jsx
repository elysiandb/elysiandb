import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import Dashboard from "../../src/pages/Dashboard.jsx";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";
import { AUDIT_ENTRIES, LOG_ENTRIES, OVERVIEW, logsResponse, series } from "../fixtures/monitoring.js";

function routes(overrides = {}) {
    return {
        "GET /monitoring/overview": OVERVIEW,
        "GET /monitoring/series": series(),
        "GET /monitoring/logs": (options, url) =>
            logsResponse(url.searchParams.get("component") === "audit" ? AUDIT_ENTRIES : LOG_ENTRIES),
        "POST /save": new Response(null, { status: 204 }),
        ...overrides,
    };
}

describe("Dashboard", () => {
    it("shows the status, the KPIs, the recent problems and the data", async () => {
        const fetchMock = mockApi(routes());
        renderWithProviders(<Dashboard />);

        expect(await screen.findByRole("heading", { name: "Dashboard" })).toBeInTheDocument();

        expect(screen.getByText("Problems: 2")).toBeInTheDocument();
        expect(screen.getByText("The admin account still uses the default password")).toBeInTheDocument();
        expect(screen.getByText("Hook 7f3c failed in the last 5 minutes")).toBeInTheDocument();
        expect(screen.getByRole("link", { name: "Users →" })).toHaveAttribute("href", "/admin/users");
        expect(screen.getByRole("link", { name: "Hooks →" })).toHaveAttribute("href", "/admin/hooks");

        expect(screen.getByText("1,284/s")).toBeInTheDocument();
        expect(screen.getByText("HTTP 1,200/s · TCP 84/s")).toBeInTheDocument();
        expect(screen.getByText("0.02%")).toBeInTheDocument();
        expect(screen.getByText("0.8 ms")).toBeInTheDocument();
        expect(screen.getByText("12 ms")).toBeInTheDocument();
        expect(screen.getByText("OK")).toBeInTheDocument();

        expect(screen.getByText("request failed")).toBeInTheDocument();
        expect(screen.getByText("schema changed")).toBeInTheDocument();
        expect(screen.getByText("alice")).toBeInTheDocument();
        const allLogs = screen.getAllByRole("link", { name: "All logs →" }).map((link) => link.getAttribute("href"));
        expect(allLogs).toEqual(["/admin/logs?level=warn", "/admin/logs?component=audit"]);

        expect(screen.getByText("120,394")).toBeInTheDocument();
        expect(screen.getByText("48,211")).toBeInTheDocument();
        expect(screen.getByText("1.2 GB")).toBeInTheDocument();
        expect(screen.getByRole("link", { name: /books/ })).toHaveAttribute("href", "/admin/browse?entity=books");

        expect(screen.getByText("0.2.2")).toBeInTheDocument();
        expect(screen.getByText("Internal")).toBeInTheDocument();
        expect(screen.getByText("0.0.0.0:8089")).toBeInTheDocument();
        expect(screen.getByText("4h")).toBeInTheDocument();
        expect(screen.getByText("API cache")).toBeInTheDocument();
        expect(screen.queryByText("Schema")).not.toBeInTheDocument();

        const logRequests = requestsTo(fetchMock, "GET", "/monitoring/logs").map(([url]) => Object.fromEntries(new URL(url).searchParams));
        expect(logRequests).toEqual(expect.arrayContaining([{ level: "warn", limit: "5" }, { component: "audit", limit: "5" }]));
        expect(requestsTo(fetchMock, "GET", "/monitoring/series").map(([url]) => new URL(url).searchParams.get("range"))).toEqual(["15m"]);
        expectNoRawTranslationKeys();
    });

    it("is displayed in French", async () => {
        mockApi(routes());
        renderWithProviders(<Dashboard />, { language: "fr" });

        expect(await screen.findByRole("heading", { name: "Tableau de bord" })).toBeInTheDocument();
        expect(screen.getByText("Le compte admin utilise toujours le mot de passe par défaut")).toBeInTheDocument();
        expect(screen.getByText("Problèmes récents")).toBeInTheDocument();
        expect(screen.getByText("Activité des administrateurs")).toBeInTheDocument();
        expect(screen.getByText("120 394")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Écrire sur disque maintenant" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("shows a healthy status without problems", async () => {
        mockApi(routes({ "GET /monitoring/overview": { ...OVERVIEW, status: { level: "healthy", problems: [] } } }));
        renderWithProviders(<Dashboard />);

        expect(await screen.findByText("No problem detected")).toBeInTheDocument();
        expect(screen.getByText("Healthy")).toBeInTheDocument();
    });

    it("still works when the log buffer is turned off", async () => {
        mockApi(routes({ "GET /monitoring/logs": new Response(null, { status: 404 }) }));
        renderWithProviders(<Dashboard />);

        expect(await screen.findAllByText("The log buffer is turned off")).toHaveLength(2);
        expect(screen.getByText("1,284/s")).toBeInTheDocument();
    });

    it("still shows the overview when the history cannot be loaded", async () => {
        mockApi(routes({ "GET /monitoring/series": new Response(null, { status: 500 }) }));
        renderWithProviders(<Dashboard />);

        expect(await screen.findByText("1,284/s")).toBeInTheDocument();
        expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    });

    it("flushes the data to disk from the quick actions", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi(routes());
        renderWithProviders(<Dashboard />);

        await user.click(await screen.findByRole("button", { name: "Flush to disk now" }));

        expect(await screen.findByText("Data flushed to disk")).toBeInTheDocument();
        expect(requestsTo(fetchMock, "POST", "/save")).toHaveLength(1);
        expect(screen.getByRole("link", { name: "Download a backup" })).toHaveAttribute("href", "/api/backup");
        expect(screen.queryByRole("button", { name: /reset/i })).not.toBeInTheDocument();
    });

    it("reports a failed flush in the KPI", async () => {
        mockApi(routes({
            "GET /monitoring/overview": {
                ...OVERVIEW,
                persistence: { ...OVERVIEW.persistence, last_flush_ok: false, last_flush_error: "disk full" },
            },
        }));
        renderWithProviders(<Dashboard />);

        const badge = await screen.findByText("Failed");
        expect(badge).toHaveAttribute("title", "disk full");
        expect(within(badge.closest(".monitoring-kpi")).getByText("Last flush")).toBeInTheDocument();
    });

    it("shows an error when the overview cannot be loaded", async () => {
        mockApi(routes({ "GET /monitoring/overview": new Response(null, { status: 500 }) }));
        renderWithProviders(<Dashboard />);

        expect(await screen.findByRole("alert")).toHaveTextContent("Error:");
    });
});
