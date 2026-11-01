import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import NavBar from "../../src/components/header/NavBar.jsx";
import { expectNoRawTranslationKeys, renderWithProviders } from "../helpers.jsx";

const LINKS = [
    ["/admin", "Dashboard", "Tableau de bord"],
    ["/admin/monitoring", "Monitoring", "Supervision"],
    ["/admin/logs", "Logs", "Journaux"],
    ["/admin/configuration", "Configuration", "Configuration"],
    ["/admin/users", "Users", "Utilisateurs"],
    ["/admin/acl", "ACL", "ACL"],
    ["/admin/schema", "Schema", "Schéma"],
    ["/admin/hooks", "Hooks", "Hooks"],
    ["/admin/browse", "Browse", "Explorer"],
];

describe("NavBar", () => {
    it("is displayed in English", () => {
        renderWithProviders(<NavBar />);

        for (const [href, english] of LINKS) {
            expect(screen.getByRole("link", { name: english })).toHaveAttribute("href", href);
        }
        expect(screen.getByText("Hello admin")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Logout" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Toggle navigation" })).toBeInTheDocument();
        expect(screen.getByRole("link", { name: "ElysianDB Admin" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("is displayed in French", () => {
        renderWithProviders(<NavBar />, { language: "fr" });

        for (const [href, , french] of LINKS) {
            expect(screen.getByRole("link", { name: french })).toHaveAttribute("href", href);
        }
        expect(screen.getByText("Bonjour admin")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Déconnexion" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Afficher la navigation" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("switches language from its selector", async () => {
        const user = userEvent.setup();
        renderWithProviders(<NavBar />);

        await user.selectOptions(screen.getByRole("combobox", { name: "Language" }), "fr");

        expect(screen.getByRole("link", { name: "Utilisateurs" })).toBeInTheDocument();
        expect(screen.queryByRole("link", { name: "Users" })).not.toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Déconnexion" })).toBeInTheDocument();
    });
});
