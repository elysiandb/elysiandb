import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it } from "vitest";
import App from "../src/App.jsx";
import Home from "../src/pages/Home.jsx";
import { ToastProvider } from "../src/components/notification/ToastProvider.jsx";
import { useAccountStore } from "../src/account/store.ts";
import { useLanguageStore } from "../src/i18n/store.js";
import { mockApi, renderWithProviders, requestsTo } from "./helpers.jsx";

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

    it("ends the session and shows the login page on logout", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi({
            "POST /api/security/logout": () => new Response(null, { status: 204 }),
        });
        window.history.pushState({}, "", "/admin");
        renderApp("en");

        await user.click(screen.getByRole("button", { name: "Logout" }));

        expect(await screen.findByRole("button", { name: "Log in" })).toBeInTheDocument();
        expect(screen.queryByRole("button", { name: "Logout" })).not.toBeInTheDocument();
        expect(requestsTo(fetchMock, "POST", "/api/security/logout")).toHaveLength(1);
    });
});

describe("Home", () => {
    it.each([
        ["en", "Home"],
        ["fr", "Accueil"],
    ])("shows its title in %s", (language, title) => {
        renderWithProviders(<Home />, { language });

        expect(screen.getByRole("heading", { name: title })).toBeInTheDocument();
    });
});
