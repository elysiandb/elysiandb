import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MemoryRouter, Route, Routes, useLocation } from "react-router-dom";
import { describe, expect, it } from "vitest";
import Logs from "../../src/pages/Logs.jsx";
import { ToastProvider } from "../../src/components/notification/ToastProvider.jsx";
import { useLanguageStore } from "../../src/i18n/store.js";
import { expectNoRawTranslationKeys, mockApi, requestsTo } from "../helpers.jsx";
import { LOG_ENTRIES, OVERVIEW, logsResponse } from "../fixtures/monitoring.js";

function LocationProbe() {
    const location = useLocation();
    return <output data-testid="location">{location.search}</output>;
}

function renderLogs(url = "/admin/logs", language = "en") {
    useLanguageStore.setState({ language });

    return render(
        <ToastProvider>
            <MemoryRouter initialEntries={[url]}>
                <Routes>
                    <Route path="/admin/logs" element={<><Logs /><LocationProbe /></>} />
                </Routes>
            </MemoryRouter>
        </ToastProvider>
    );
}

function searchOf() {
    return new URLSearchParams(screen.getByTestId("location").textContent);
}

function logQueries(fetchMock) {
    return requestsTo(fetchMock, "GET", "/monitoring/logs").map(([url]) => Object.fromEntries(new URL(url).searchParams));
}

const LEVEL = { level: "info", configured: "info", until: null };

function routes(overrides = {}) {
    return {
        "GET /monitoring/overview": OVERVIEW,
        "GET /monitoring/logs/level": LEVEL,
        "GET /monitoring/logs": logsResponse(LOG_ENTRIES),
        ...overrides,
    };
}

describe("Logs", () => {
    it("reads its filters from the URL and asks the server for them", async () => {
        localStorage.setItem("logs.follow", "false");
        const fetchMock = mockApi(routes());
        renderLogs("/admin/logs?level=error&component=http&request_id=7f3c9a");

        expect(await screen.findByText("request failed")).toBeInTheDocument();
        expect(screen.getByRole("combobox", { name: "Minimum level" })).toHaveValue("error");
        expect(screen.getByRole("textbox", { name: "Request ID" })).toHaveValue("7f3c9a");
        expect(logQueries(fetchMock)).toContainEqual({ level: "error", component: "http", request_id: "7f3c9a", limit: "200" });
        expect(screen.getByRole("link", { name: "Download (NDJSON)" })).toHaveAttribute(
            "href",
            "/monitoring/logs/download?level=error&component=http&request_id=7f3c9a"
        );
        expect(screen.getByText(/^340 of 5,000 lines kept in memory since \d\d:\d\d\. A restart clears them\.$/)).toBeInTheDocument();
        expect(screen.getByText("Errors: 3 in 1 h · 11 in 24 h")).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("writes the filters to the URL", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        const fetchMock = mockApi(routes());
        renderLogs();

        await screen.findByText("request failed");
        await user.selectOptions(screen.getByRole("combobox", { name: "Minimum level" }), "warn");
        expect(searchOf().get("level")).toBe("warn");

        await user.type(screen.getByRole("textbox", { name: "Search" }), "timeout{Enter}");
        expect(searchOf().get("q")).toBe("timeout");

        await waitFor(() => expect(logQueries(fetchMock)).toContainEqual({ level: "warn", q: "timeout", limit: "200" }));

        await user.click(screen.getByRole("button", { name: "Clear" }));
        expect(searchOf().toString()).toBe("");
    });

    it("filters on a request when its id is clicked", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        mockApi(routes());
        renderLogs();

        await user.click(await screen.findByRole("button", { name: "request_id=7f3c9a" }));

        expect(searchOf().get("request_id")).toBe("7f3c9a");
    });

    it("shows the full entry and offers to copy it", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        mockApi(routes());
        renderLogs();

        await user.click(await screen.findByRole("button", { name: /request failed/ }));

        expect(screen.getByText(/"request_id": "7f3c9a"/)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Copy" })).toBeInTheDocument();
    });

    it("adds the newer lines on refresh", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        const fresh = { seq: 13, time: "2026-11-01T12:04:00.000Z", level: "INFO", component: "audit", msg: "user created", attrs: { actor: "alice" } };
        const fetchMock = mockApi(routes({
            "GET /monitoring/logs": (options, url) => logsResponse(url.searchParams.get("after") === "12" ? [fresh] : LOG_ENTRIES),
        }));
        renderLogs();

        await screen.findByText("request failed");
        await user.click(screen.getByRole("button", { name: "Refresh" }));

        expect(await screen.findByText("user created")).toBeInTheDocument();
        expect(logQueries(fetchMock)).toContainEqual({ after: "12", limit: "1000" });
        const messages = screen.getAllByRole("listitem").map((item) => item.querySelector(".logs-message").textContent);
        expect(messages).toEqual(["user created", "request failed", "hook script failed"]);
    });

    it("keeps the list still when scrolled down and offers the new lines", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        const fresh = { seq: 13, time: "2026-11-01T12:04:00.000Z", level: "INFO", component: "audit", msg: "user created", attrs: {} };
        mockApi(routes({
            "GET /monitoring/logs": (options, url) => logsResponse(url.searchParams.get("after") === "12" ? [fresh] : (url.searchParams.get("after") ? [] : LOG_ENTRIES)),
        }));
        renderLogs();

        await screen.findByText("request failed");
        fireEvent.scroll(screen.getByRole("list", { name: "Logs" }), { target: { scrollTop: 500 } });
        await user.click(screen.getByRole("button", { name: "Refresh" }));

        const pending = await screen.findByRole("button", { name: "1 new lines" });
        expect(screen.queryByText("user created")).not.toBeInTheDocument();

        await user.click(pending);

        expect(await screen.findByText("user created")).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "1 new lines" })).not.toBeInTheDocument();
    });

    it("loads older lines", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        const older = { seq: 3, time: "2026-11-01T11:00:00.000Z", level: "INFO", component: "boot", msg: "storage loaded", attrs: {} };
        const fetchMock = mockApi(routes({
            "GET /monitoring/logs": (options, url) => {
                if (url.searchParams.get("before") === "11") return logsResponse([older], { has_more: false });
                return logsResponse(LOG_ENTRIES, { has_more: true });
            },
        }));
        renderLogs();

        await user.click(await screen.findByRole("button", { name: "Load older lines" }));

        expect(await screen.findByText("storage loaded")).toBeInTheDocument();
        expect(logQueries(fetchMock)).toContainEqual({ before: "11", limit: "200" });
    });

    it("explains when the log buffer is turned off", async () => {
        mockApi(routes({ "GET /monitoring/logs": new Response(null, { status: 404 }) }));
        renderLogs();

        expect(await screen.findByText("The log buffer is turned off on this server (log.bufferLines: 0).")).toBeInTheDocument();
    });

    it("switches the log level to debug for 15 minutes", async () => {
        const user = userEvent.setup();
        localStorage.setItem("logs.follow", "false");
        const fetchMock = mockApi(routes({
            "PUT /monitoring/logs/level": { level: "debug", configured: "info", until: "2026-11-01T12:15:00Z" },
        }));
        renderLogs();

        await user.click(await screen.findByRole("button", { name: "Debug for 15 min" }));

        expect(await screen.findByText("Log level set to Debug")).toBeInTheDocument();
        const [[, options]] = requestsTo(fetchMock, "PUT", "/monitoring/logs/level");
        expect(JSON.parse(options.body)).toEqual({ level: "debug", minutes: 15 });
    });

    it("is displayed in French", async () => {
        localStorage.setItem("logs.follow", "false");
        mockApi(routes());
        renderLogs("/admin/logs", "fr");

        expect(await screen.findByText("request failed")).toBeInTheDocument();
        expect(screen.getByRole("heading", { name: "Journaux" })).toBeInTheDocument();
        expect(screen.getByRole("combobox", { name: "Niveau minimum" })).toBeInTheDocument();
        expect(screen.getByRole("checkbox", { name: "Suivre" })).not.toBeChecked();
        expect(screen.getByRole("link", { name: "Télécharger (NDJSON)" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });
});
