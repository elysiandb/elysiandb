import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import App from "../src/App.jsx";
import Dashboard from "../src/pages/Dashboard.jsx";
import { ToastProvider } from "../src/components/notification/ToastProvider.jsx";
import { useAccountStore } from "../src/account/store.ts";
import { useLanguageStore } from "../src/i18n/store.js";
import { mockApi, renderWithProviders, requestsTo } from "./helpers.jsx";
import { OVERVIEW, logsResponse, series } from "./fixtures/monitoring.js";

function renderApp(language) {
    useLanguageStore.setState({ language });

    return render(
        <ToastProvider>
            <App />
        </ToastProvider>
    );
}

describe("App", () => {
    afterEach(() => {
        window.history.pushState({}, "", "/");
    });

    it("translates the loading indicator", () => {
        useAccountStore.setState({ account: undefined });
        renderApp("fr");

        expect(screen.getByRole("status")).toHaveTextContent("Chargement...");
    });

    it("shows the login page in the current language to guests", () => {
        useAccountStore.setState({ account: null });
        renderApp("fr");

        expect(screen.getByRole("button", { name: "Se connecter" })).toBeInTheDocument();
        expect(screen.getByRole("combobox", { name: "Langue" })).toHaveValue("fr");
    });

    it("switches language without reloading the current page", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi({
            "GET /api/security/user": { users: [{ username: "alice", role: "user" }] },
        });
        window.history.pushState({}, "", "/admin/users");
        renderApp("en");

        await screen.findByRole("cell", { name: "alice" });
        await user.type(screen.getByPlaceholderText("Search user…"), "ali");
        await user.selectOptions(screen.getByRole("combobox", { name: "Language" }), "fr");

        expect(screen.getByRole("link", { name: "Utilisateurs" })).toHaveClass("active");
        expect(screen.getByPlaceholderText("Rechercher un utilisateur…")).toHaveValue("ali");
        expect(screen.getByRole("columnheader", { name: "Nom d'utilisateur" })).toBeInTheDocument();
        expect(requestsTo(fetchMock, "GET", "/api/security/user")).toHaveLength(1);
    });
});

describe("Dashboard", () => {
    it.each([
        ["en", "Dashboard"],
        ["fr", "Tableau de bord"],
        ["es", "Panel"],
        ["zh", "仪表盘"],
        ["de", "Dashboard"],
        ["ja", "ダッシュボード"],
        ["ko", "대시보드"],
        ["pt", "Painel"],
        ["nl", "Dashboard"],
    ])("shows its title in %s", async (language, title) => {
        mockApi({
            "GET /monitoring/overview": OVERVIEW,
            "GET /monitoring/series": series(),
            "GET /monitoring/logs": logsResponse([]),
        });
        renderWithProviders(<Dashboard />, { language });

        expect(await screen.findByRole("heading", { name: title })).toBeInTheDocument();
    });

    it("is the home page of the Admin UI", async () => {
        mockApi({
            "GET /monitoring/overview": OVERVIEW,
            "GET /monitoring/series": series(),
            "GET /monitoring/logs": logsResponse([]),
        });
        window.history.pushState({}, "", "/admin");
        renderApp("en");

        expect(await screen.findByRole("heading", { name: "Dashboard" })).toBeInTheDocument();
        expect(screen.getByRole("link", { name: "Dashboard" })).toHaveClass("active");
    });
});
