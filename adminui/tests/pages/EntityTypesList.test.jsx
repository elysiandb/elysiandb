import { screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import EntityTypesList from "../../src/pages/EntityTypesList.jsx";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

function routes({ manual = true } = {}) {
    return {
        "GET /api/entity/types": { entities: [JSON.stringify({ id: "articles", _manual: manual })] },
        "GET /api/articles/schema": {
            id: "articles",
            _manual: manual,
            fields: { title: { name: "title", type: "string", required: true } },
        },
        "PUT /api/articles/schema": {},
        "POST /api/books/create": {},
    };
}

describe("EntityTypesList", () => {
    it("is displayed in English", async () => {
        mockApi(routes());
        renderWithProviders(<EntityTypesList />);

        expect(await screen.findByText("articles (manual)")).toBeInTheDocument();
        expect(screen.getByText("Entities")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "+ Add field" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Update schema" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Drop Entity Type" })).toBeInTheDocument();
        expect(screen.getByText("required")).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it("is displayed in French", async () => {
        mockApi(routes());
        renderWithProviders(<EntityTypesList />, { language: "fr" });

        expect(await screen.findByText("articles (manuel)")).toBeInTheDocument();
        expect(screen.getByText("Entités")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "+ Ajouter un champ" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Mettre à jour le schéma" })).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Supprimer le type d'entité" })).toBeInTheDocument();
        expect(screen.getByText("requis")).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });

    it.each([
        ["en", "articles (auto-managed)"],
        ["fr", "articles (géré automatiquement)"],
    ])("translates auto-managed entity types in %s", async (language, title) => {
        mockApi(routes({ manual: false }));
        renderWithProviders(<EntityTypesList />, { language });

        expect(await screen.findByText(title)).toBeInTheDocument();
    });

    it("notifies a schema update in the current language", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi(routes());
        renderWithProviders(<EntityTypesList />, { language: "fr" });

        await user.click(await screen.findByRole("button", { name: "Mettre à jour le schéma" }));

        expect(await screen.findByText("Schéma mis à jour pour « articles »")).toBeInTheDocument();
        expect(requestsTo(fetchMock, "PUT", "/api/articles/schema")).toHaveLength(1);
    });

    it("asks for drop confirmation in the current language", async () => {
        const user = userEvent.setup();
        const confirm = vi.spyOn(window, "confirm").mockReturnValue(false);
        const fetchMock = mockApi(routes());
        renderWithProviders(<EntityTypesList />, { language: "fr" });

        await user.click(await screen.findByRole("button", { name: "Supprimer le type d'entité" }));

        expect(confirm).toHaveBeenCalledWith("Supprimer le type d'entité « articles » ?");
        expect(requestsTo(fetchMock, "DELETE", "/api/articles")).toHaveLength(0);
    });

    it("translates the create entity type modal and its notification", async () => {
        const user = userEvent.setup();
        mockApi(routes());
        const { container } = renderWithProviders(<EntityTypesList />, { language: "fr" });

        await screen.findByText("articles (manuel)");
        await user.click(container.querySelector(".create-entity-type-button"));

        const dialog = await screen.findByRole("dialog");
        expect(within(dialog).getByText("Créer un type d'entité")).toBeInTheDocument();
        expect(within(dialog).getByRole("button", { name: "Annuler" })).toBeInTheDocument();

        await user.type(within(dialog).getByPlaceholderText("Nom de l'entité"), "books");
        await user.click(within(dialog).getByRole("button", { name: "Créer" }));

        expect(await screen.findByText("Entité « books » créée avec succès")).toBeInTheDocument();
    });
});
