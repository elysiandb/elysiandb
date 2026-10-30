import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import UsersList from "../../src/pages/UsersList.jsx";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

const USERS = { users: [{ username: "alice", role: "user" }, { username: "admin", role: "admin" }] };

function aliceRow() {
    return screen.getByRole("cell", { name: "alice" }).closest("tr");
}

describe("UsersList", () => {
    it("is displayed in English", async () => {
        mockApi({ "GET /api/security/user": USERS });
        renderWithProviders(<UsersList />);

        expect(await screen.findByRole("columnheader", { name: "Username" })).toBeInTheDocument();
        expect(screen.getByRole("columnheader", { name: "Role" })).toBeInTheDocument();
        expect(screen.getByRole("columnheader", { name: "Actions" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Create a user" })).toBeInTheDocument();
        expect(screen.getByPlaceholderText("Search user…")).toBeInTheDocument();
        expect(screen.getAllByRole("button", { name: "Change password" })).toHaveLength(2);
        expect(screen.getAllByRole("button", { name: "Delete" })).toHaveLength(2);
        expectNoRawTranslationKeys();
    });

    it("is displayed in French", async () => {
        mockApi({ "GET /api/security/user": USERS });
        renderWithProviders(<UsersList />, { language: "fr" });

        expect(await screen.findByRole("columnheader", { name: "Nom d'utilisateur" })).toBeInTheDocument();
        expect(screen.getByRole("columnheader", { name: "Rôle" })).toBeInTheDocument();
        expect(screen.getByRole("columnheader", { name: "Actions" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Créer un utilisateur" })).toBeInTheDocument();
        expect(screen.getByPlaceholderText("Rechercher un utilisateur…")).toBeInTheDocument();
        expect(screen.getAllByRole("button", { name: "Changer le mot de passe" })).toHaveLength(2);
        expect(screen.getAllByRole("button", { name: "Supprimer" })).toHaveLength(2);
        expectNoRawTranslationKeys();
    });

    it("asks for delete confirmation in the current language", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi({ "GET /api/security/user": USERS });
        const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
        renderWithProviders(<UsersList />, { language: "fr" });

        await screen.findByRole("cell", { name: "alice" });
        await user.click(within(aliceRow()).getByRole("button", { name: "Supprimer" }));

        expect(confirm).toHaveBeenCalledWith("Supprimer l'utilisateur « alice » ?");
        expect(requestsTo(fetchMock, "DELETE", "/api/security/user/alice")).toHaveLength(0);
    });

    it("notifies a deletion in the current language", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi({ "GET /api/security/user": USERS, "DELETE /api/security/user/alice": {} });
        const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
        renderWithProviders(<UsersList />);

        await screen.findByRole("cell", { name: "alice" });
        await user.click(within(aliceRow()).getByRole("button", { name: "Delete" }));

        expect(confirm).toHaveBeenCalledWith("Delete user \"alice\"?");
        expect(await screen.findByText("User successfully deleted")).toBeInTheDocument();
        expect(requestsTo(fetchMock, "DELETE", "/api/security/user/alice")).toHaveLength(1);
    });

    it("notifies a role change in the current language", async () => {
        const user = userEvent.setup();
        mockApi({ "GET /api/security/user": USERS, "PUT /api/security/user/alice/role": {} });
        renderWithProviders(<UsersList />, { language: "fr" });

        await screen.findByRole("cell", { name: "alice" });
        await user.selectOptions(within(aliceRow()).getByRole("combobox"), "admin");

        expect(await screen.findByText("Rôle de l'utilisateur modifié avec succès")).toBeInTheDocument();
    });

    it("notifies a failed role change in the current language", async () => {
        const user = userEvent.setup();
        mockApi({
            "GET /api/security/user": USERS,
            "PUT /api/security/user/alice/role": new Response(null, { status: 500 }),
        });
        renderWithProviders(<UsersList />, { language: "fr" });

        await screen.findByRole("cell", { name: "alice" });
        await user.selectOptions(within(aliceRow()).getByRole("combobox"), "admin");

        expect(await screen.findByText("Échec de la modification du rôle de l'utilisateur")).toBeInTheDocument();
    });

    it("translates the create user modal", async () => {
        const user = userEvent.setup();
        mockApi({ "GET /api/security/user": USERS });
        renderWithProviders(<UsersList />, { language: "fr" });

        await user.click(await screen.findByRole("button", { name: "Créer un utilisateur" }));

        const dialog = await screen.findByRole("dialog");
        expect(within(dialog).getByText("Créer un utilisateur")).toBeInTheDocument();
        expect(within(dialog).getByText("Nom d'utilisateur")).toBeInTheDocument();
        expect(within(dialog).getByText("Mot de passe")).toBeInTheDocument();
        expect(within(dialog).getByText("Rôle")).toBeInTheDocument();
        expect(within(dialog).getByRole("button", { name: "Annuler" })).toBeInTheDocument();
        expect(within(dialog).getByRole("button", { name: "Enregistrer" })).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("translates the edit user modal and its notifications", async () => {
        const user = userEvent.setup();
        mockApi({ "GET /api/security/user": USERS, "PUT /api/security/user/alice/password": {} });
        renderWithProviders(<UsersList />, { language: "fr" });

        await screen.findByRole("cell", { name: "alice" });
        await user.click(within(aliceRow()).getByRole("button", { name: "Changer le mot de passe" }));

        const dialog = await screen.findByRole("dialog");
        expect(within(dialog).getByText("Modifier l'utilisateur")).toBeInTheDocument();

        await user.type(dialog.querySelector("input[type=password]"), "new-secret");
        await user.click(within(dialog).getByRole("button", { name: "Enregistrer" }));

        expect(await screen.findByText("Mot de passe de l'utilisateur modifié avec succès")).toBeInTheDocument();
    });

    it("asks for a password in the current language", async () => {
        const user = userEvent.setup();
        mockApi({ "GET /api/security/user": USERS });
        renderWithProviders(<UsersList />, { language: "fr" });

        await screen.findByRole("cell", { name: "alice" });
        await user.click(within(aliceRow()).getByRole("button", { name: "Changer le mot de passe" }));
        await user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Enregistrer" }));

        expect(await screen.findByText("Veuillez saisir un mot de passe.")).toBeInTheDocument();
    });
});
