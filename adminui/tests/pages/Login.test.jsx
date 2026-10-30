import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import { Login } from "../../src/pages/Login.tsx";
import { useAccountStore } from "../../src/account/store.ts";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

describe("Login", () => {
    it("is displayed in English", () => {
        renderWithProviders(<Login />);

        expect(screen.getByLabelText("Username")).toHaveAttribute("type", "text");
        expect(screen.getByLabelText("Password")).toHaveAttribute("type", "password");
        expect(screen.getByRole("button", { name: "Log in" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("is displayed in French", () => {
        renderWithProviders(<Login />, { language: "fr" });

        expect(screen.getByLabelText("Nom d'utilisateur")).toHaveAttribute("type", "text");
        expect(screen.getByLabelText("Mot de passe")).toHaveAttribute("type", "password");
        expect(screen.getByRole("button", { name: "Se connecter" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("switches language without losing typed values", async () => {
        const user = userEvent.setup();
        renderWithProviders(<Login />);

        await user.clear(screen.getByLabelText("Username"));
        await user.type(screen.getByLabelText("Username"), "bob");
        await user.selectOptions(screen.getByRole("combobox", { name: "Language" }), "fr");

        expect(screen.getByLabelText("Nom d'utilisateur")).toHaveValue("bob");
        expect(screen.getByRole("button", { name: "Se connecter" })).toBeInTheDocument();
        expect(document.body).not.toHaveTextContent("Log in");
    });

    it("logs in with the typed credentials", async () => {
        const user = userEvent.setup();
        useAccountStore.setState({ account: null });
        const fetchMock = mockApi({ "POST /api/security/login": { id: 2, username: "bob" } });
        renderWithProviders(<Login />, { language: "fr" });

        await user.clear(screen.getByLabelText("Nom d'utilisateur"));
        await user.type(screen.getByLabelText("Nom d'utilisateur"), "bob");
        await user.clear(screen.getByLabelText("Mot de passe"));
        await user.type(screen.getByLabelText("Mot de passe"), "secret");
        await user.click(screen.getByRole("button", { name: "Se connecter" }));

        const [[, options]] = requestsTo(fetchMock, "POST", "/api/security/login");
        expect(JSON.parse(options.body)).toEqual({ username: "bob", password: "secret" });
        await waitFor(() => expect(useAccountStore.getState().account).toEqual({ id: 2, username: "bob" }));
    });
});
