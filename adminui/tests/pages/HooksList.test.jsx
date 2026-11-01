import { fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import HooksList from "../../src/pages/HooksList.jsx";
import { expectNoRawTranslationKeys, mockApi, renderWithProviders, requestsTo } from "../helpers.jsx";

vi.mock("../../src/components/hook/ScriptEditor.jsx", () => ({
    default: ({ value, onChange }) => (
        <textarea aria-label="script-editor" value={value} onChange={(e) => onChange(e.target.value)} />
    ),
}));

const HOOK = {
    id: "h1",
    entity: "articles",
    name: "enrich",
    event: "post_read",
    priority: 5,
    language: "javascript",
    script: "function wrong() {}",
    bypass_acl: false,
    enabled: false,
};

const ROUTES = {
    "GET /api/entity/types": { entities: [JSON.stringify({ id: "articles", _manual: false })] },
    "GET /api/hook/articles": [HOOK],
    "GET /api/hook/id/h1": HOOK,
    "PUT /api/hook/id/h1": {},
    "POST /api/hook/articles": {},
};

async function openHook(user) {
    await user.click(await screen.findByText("5 - enrich"));
    return screen.findByLabelText("script-editor");
}

describe("HooksList", () => {
    it("is displayed in French", async () => {
        const user = userEvent.setup();
        mockApi(ROUTES);
        renderWithProviders(<HooksList />, { language: "fr" });

        expect(await screen.findByText("Entités")).toBeInTheDocument();
        expect(await screen.findByText("INACTIF")).toBeInTheDocument();

        await openHook(user);
        expect(screen.getByText("Hook")).toBeInTheDocument();
        expect(screen.getByText("Options")).toBeInTheDocument();
        expect(screen.getByText("Script")).toBeInTheDocument();

        await user.click(screen.getByRole("button", { name: "Afficher" }));

        expect(screen.getByRole("button", { name: "Masquer" })).toBeInTheDocument();
        expect(screen.getByLabelText("Activé")).not.toBeChecked();
        expect(screen.getByLabelText("Ignorer les ACL")).not.toBeChecked();
        for (const label of ["Langage", "Événement", "Priorité", "Nom"]) {
            expect(screen.getByText(label)).toBeInTheDocument();
        }
        expectNoRawTranslationKeys();
    });

    it("is displayed in English", async () => {
        const user = userEvent.setup();
        mockApi(ROUTES);
        renderWithProviders(<HooksList />);

        expect(await screen.findByText("OFF")).toBeInTheDocument();

        await openHook(user);
        await user.click(screen.getByRole("button", { name: "Show" }));

        expect(screen.getByRole("button", { name: "Hide" })).toBeInTheDocument();
        expect(screen.getByLabelText("Enabled")).toBeInTheDocument();
        expect(screen.getByLabelText("Bypass ACL")).toBeInTheDocument();
        for (const label of ["Language", "Event", "Priority", "Name", "Script"]) {
            expect(screen.getByText(label)).toBeInTheDocument();
        }
        expectNoRawTranslationKeys();
    });

    it.each([
        ["en", "post_read", "function wrong() {}", "post_read hooks must start with: function postRead"],
        ["fr", "post_read", "function wrong() {}", "Les hooks post_read doivent commencer par : function postRead"],
        ["fr", "pre_read", "function postRead() {}", "Les hooks pre_read doivent commencer par : function preRead"],
    ])("validates the script signature in %s for %s", async (language, event, script, message) => {
        const user = userEvent.setup();
        mockApi({ ...ROUTES, "GET /api/hook/id/h1": { ...HOOK, event, script } });
        renderWithProviders(<HooksList />, { language });

        await openHook(user);

        expect(screen.getByRole("alert")).toHaveTextContent(message);
        expect(screen.getByRole("button", { name: language === "fr" ? "Enregistrer" : "Save" })).toBeDisabled();
    });

    it("notifies a saved hook in the current language", async () => {
        const user = userEvent.setup();
        const fetchMock = mockApi(ROUTES);
        renderWithProviders(<HooksList />, { language: "fr" });

        const editor = await openHook(user);
        fireEvent.change(editor, { target: { value: "function postRead(entity) { return entity; }" } });

        expect(screen.queryByRole("alert")).not.toBeInTheDocument();
        await user.click(screen.getByRole("button", { name: "Enregistrer" }));

        expect(await screen.findByText("Hook « enrich » enregistré")).toBeInTheDocument();
        expect(requestsTo(fetchMock, "PUT", "/api/hook/id/h1")).toHaveLength(1);
    });

    it("translates the create hook modal and its notification", async () => {
        const user = userEvent.setup();
        mockApi(ROUTES);
        const { container } = renderWithProviders(<HooksList />, { language: "fr" });

        await screen.findByText("5 - enrich");
        await user.click(container.querySelector(".create-entity-type-button"));

        const dialog = await screen.findByRole("dialog");
        expect(within(dialog).getByText("Créer un hook")).toBeInTheDocument();
        expect(within(dialog).getByText("Événement")).toBeInTheDocument();
        expect(within(dialog).getByText("Nom")).toBeInTheDocument();
        expect(within(dialog).getByRole("button", { name: "Annuler" })).toBeInTheDocument();

        await user.type(dialog.querySelector("input[type=text]"), "audit");
        await user.click(within(dialog).getByRole("button", { name: "Enregistrer" }));

        expect(await screen.findByText("Le hook « audit » a été créé pour « articles »")).toBeInTheDocument();
    });
});
