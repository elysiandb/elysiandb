import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { MemoryRouter } from "react-router-dom";
import { ToastProvider } from "../../src/components/notification/ToastProvider.jsx";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import EntitiesBrowser from "../../src/pages/EntitiesBrowser.jsx";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

const ROUTES = {
    "GET /api/entity/types": { entities: [JSON.stringify({ id: "articles", _manual: false })] },
    "POST /api/query": [{ id: "a1", title: "Hello" }, { title: "Without id" }],
    "DELETE /api/articles/a1": {},
};

async function openQuery(user, title) {
    await user.click(await screen.findByText(title));
}

describe("EntitiesBrowser", () => {
    it("is displayed in French", async () => {
        const user = userEvent.setup();
        mockApi(ROUTES);
        renderWithProviders(<EntitiesBrowser />, { language: "fr" });

        expect(await screen.findByText("Entités")).toBeInTheDocument();
        expect(await screen.findAllByText("a1")).not.toHaveLength(0);
        expect(screen.getByText("Document")).toBeInTheDocument();
        expect(screen.getByRole("button", { name: "Supprimer" })).toBeInTheDocument();

        await openQuery(user, "Requête");

        expect(screen.getByRole("button", { name: "Rechercher" })).toBeInTheDocument();
        for (const title of ["Filtres", "Tri", "Pagination"]) {
            expect(screen.getByText(title)).toBeInTheDocument();
        }
        expectNoRawTranslationKeys();
    });

    it("is displayed in English", async () => {
        const user = userEvent.setup();
        mockApi(ROUTES);
        renderWithProviders(<EntitiesBrowser />);

        expect(await screen.findByText("Entities")).toBeInTheDocument();
        await screen.findAllByText("a1");
        await openQuery(user, "Query");

        expect(screen.getByRole("button", { name: "Search" })).toBeInTheDocument();
        for (const title of ["Filters", "Sort", "Pagination"]) {
            expect(screen.getByText(title)).toBeInTheDocument();
        }
        expectNoRawTranslationKeys();
    });

    it("reports an invalid query in the current language", async () => {
        const user = userEvent.setup();
        const alert = vi.spyOn(window, "alert").mockImplementation(() => {});
        mockApi(ROUTES);
        renderWithProviders(<EntitiesBrowser />, { language: "fr" });

        await screen.findAllByText("a1");
        await openQuery(user, "Requête");
        fireEvent.change(screen.getByRole("textbox"), { target: { value: "{ invalid" } });
        await user.click(screen.getByRole("button", { name: "Rechercher" }));

        expect(alert).toHaveBeenCalledWith("Requête JSON invalide");
    });

    it("reports a failed query in the current language", async () => {
        mockApi({ ...ROUTES, "POST /api/query": new Response(null, { status: 500 }) });
        renderWithProviders(<EntitiesBrowser />, { language: "fr" });

        expect(await screen.findByText("Erreur lors de l'exécution de la requête")).toBeInTheDocument();
    });

    it("confirms and notifies a deletion in the current language", async () => {
        const user = userEvent.setup();
        const confirm = vi.spyOn(window, "confirm").mockReturnValue(true);
        const fetchMock = mockApi(ROUTES);
        renderWithProviders(<EntitiesBrowser />, { language: "fr" });

        await screen.findAllByText("a1");
        await user.click(screen.getByRole("button", { name: "Supprimer" }));

        expect(confirm).toHaveBeenCalledWith("Supprimer l'entité « a1 » ?");
        expect(await screen.findByText("Entité « a1 » supprimée")).toBeInTheDocument();
        expect(requestsTo(fetchMock, "DELETE", "/api/articles/a1")).toHaveLength(1);
    });

    it("opens the entity type given in the URL", async () => {
        const fetchMock = mockApi({
            ...ROUTES,
            "GET /api/entity/types": {
                entities: [JSON.stringify({ id: "articles", _manual: false }), JSON.stringify({ id: "books", _manual: false })],
            },
        });
        render(
            <ToastProvider>
                <MemoryRouter initialEntries={["/admin/browse?entity=books"]}>
                    <EntitiesBrowser />
                </MemoryRouter>
            </ToastProvider>
        );

        await waitFor(() => {
            const selected = document.querySelector(".entity-list-item.selected");
            expect(selected).toHaveTextContent("books");
        });
        await waitFor(() => {
            const queries = requestsTo(fetchMock, "POST", "/api/query").map(([, options]) => JSON.parse(options.body).entity);
            expect(queries).toEqual(["books"]);
        });
    });
});
