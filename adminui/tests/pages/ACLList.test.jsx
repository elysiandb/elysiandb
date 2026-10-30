import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import ACLList from "../../src/pages/ACLList.jsx";
import en from "../../src/i18n/locales/en.json";
import fr from "../../src/i18n/locales/fr.json";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

const ROUTES = {
    "GET /api/security/user": { users: [{ username: "alice", role: "user" }] },
    "GET /api/entity/types/name": { entities: ["articles", "comments"] },
    "GET /api/acl/alice": [
        { entity: "articles", permissions: { read: true, create: false } },
        { entity: "comments", permissions: { read: false } },
    ],
    "PUT /api/acl/alice/articles": {},
    "PUT /api/acl/alice/articles/default": {},
    "PUT /api/acl/alice/comments/default": {},
};

async function waitForTable() {
    await screen.findAllByRole("checkbox");
}

function createCheckboxOfFirstEntity() {
    const label = within(screen.getAllByRole("cell")[0]).getByText("create");
    return label.closest(".form-check").querySelector("input[type=checkbox]");
}

describe("ACLList", () => {
    it.each([
        ["en", en, "Users", "Search user…", "Search entity…"],
        ["fr", fr, "Utilisateurs", "Rechercher un utilisateur…", "Rechercher une entité…"],
    ])("is displayed in %s", async (language, dictionary, usersTitle, searchUser, searchEntity) => {
        mockApi(ROUTES);
        renderWithProviders(<ACLList />, { language });
        await waitForTable();

        expect(screen.getByText(usersTitle)).toBeInTheDocument();
        expect(screen.getByPlaceholderText(searchUser)).toBeInTheDocument();
        expect(screen.getByPlaceholderText(searchEntity)).toBeInTheDocument();
        expect(screen.getByRole("button", { name: dictionary.acl.update })).toBeDisabled();
        expect(screen.getByRole("button", { name: dictionary.acl.restore })).toBeEnabled();
        expect(screen.getByText(dictionary.acl.overview)).toBeInTheDocument();
        for (const [permission, description] of Object.entries(dictionary.acl.permissions)) {
            expect(screen.getByText(description)).toBeInTheDocument();
            expect(screen.getAllByText(permission).length).toBeGreaterThan(0);
        }
        expectNoRawTranslationKeys();
    });

    it("lists every permission described in the dictionaries", () => {
        expect(Object.keys(fr.acl.permissions)).toEqual([
            "read",
            "create",
            "update",
            "delete",
            "owning_read",
            "owning_write",
            "owning_update",
            "owning_delete",
        ]);
    });

    it("notifies an ACL update in the current language", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi(ROUTES);
        renderWithProviders(<ACLList />, { language: "fr" });
        await waitForTable();

        await user.click(createCheckboxOfFirstEntity());
        await user.click(screen.getByRole("button", { name: "Mettre à jour les ACL" }));

        expect(await screen.findByText("ACL mises à jour pour « alice » (entités : 1)")).toBeInTheDocument();
        expect(requestsTo(fetchMock, "PUT", "/api/acl/alice/articles")).toHaveLength(1);
    });

    it("notifies a failed ACL update in the current language", async () => {
        const user = userEvent.setup();
        mockApi({ ...ROUTES, "PUT /api/acl/alice/articles": new Response(null, { status: 500 }) });
        renderWithProviders(<ACLList />);
        await waitForTable();

        await user.click(createCheckboxOfFirstEntity());
        await user.click(screen.getByRole("button", { name: "Update ACLs" }));

        expect(await screen.findByText("Failed to update ACLs for \"alice\"")).toBeInTheDocument();
    });

    it("asks for restore confirmation in the current language", async () => {
        const user = userEvent.setup();
        const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
        mockApi(ROUTES);
        renderWithProviders(<ACLList />, { language: "fr" });
        await waitForTable();

        await user.click(screen.getByRole("button", { name: "Restaurer les valeurs par défaut" }));

        expect(confirm).toHaveBeenCalledWith("Restaurer les permissions par défaut pour « alice » ?");
        expect(await screen.findByText("ACL par défaut restaurées pour « alice »")).toBeInTheDocument();
    });
});
