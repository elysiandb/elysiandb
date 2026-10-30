import { act, renderHook, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ToastProvider } from "../../src/components/notification/ToastProvider.jsx";
import { useEntityTypeActions } from "../../src/hooks/entity/useEntityTypeActions.js";
import { useEntities } from "../../src/hooks/entity/useEntities.js";
import { useHooks } from "../../src/hooks/hook/useHooks.js";
import { useUsers } from "../../src/hooks/user/useUsers.js";
import { useLanguageStore } from "../../src/i18n/store.js";
import { mockApi } from "../helpers.jsx";

const failure = () => new Response(null, { status: 500 });

function renderInLanguage(hook, language) {
    useLanguageStore.setState({ language });
    return renderHook(hook, { wrapper: ToastProvider });
}

async function expectToast(message) {
    expect(await screen.findByText(message)).toBeInTheDocument();
}

describe("notifications", () => {
    describe("useEntityTypeActions", () => {
        it.each([
            ["en", "Entity \"books\" created successfully", "Entity \"books\" deleted", "Schema updated for \"books\""],
            ["fr", "Entité « books » créée avec succès", "Entité « books » supprimée", "Schéma mis à jour pour « books »"],
        ])("notifies successes in %s", async (language, created, dropped, updated) => {
            mockApi({ "POST /api/books/create": {}, "DELETE /api/books": {}, "PUT /api/books/schema": {} });
            const { result } = renderInLanguage(() => useEntityTypeActions(), language);

            await act(() => result.current.createEntityType("books"));
            await act(() => result.current.dropEntityType("books"));
            await act(() => result.current.updateEntitySchema("books", {}));

            await expectToast(created);
            await expectToast(dropped);
            await expectToast(updated);
        });

        it.each([
            ["en", "Failed to create entity \"books\"", "Failed to delete \"books\"", "Failed to update schema for \"books\""],
            ["fr", "Échec de la création de l'entité « books »", "Échec de la suppression de « books »", "Échec de la mise à jour du schéma pour « books »"],
        ])("notifies failures in %s", async (language, createFailed, dropFailed, updateFailed) => {
            mockApi({ "POST /api/books/create": failure, "DELETE /api/books": failure, "PUT /api/books/schema": failure });
            const { result } = renderInLanguage(() => useEntityTypeActions(), language);

            await act(() => expect(result.current.createEntityType("books")).rejects.toThrow());
            await act(() => expect(result.current.dropEntityType("books")).rejects.toThrow());
            await act(() => expect(result.current.updateEntitySchema("books", {})).rejects.toThrow());

            await expectToast(createFailed);
            await expectToast(dropFailed);
            await expectToast(updateFailed);
        });
    });

    describe("useHooks", () => {
        it.each([
            ["en", "The hook \"audit\" was created for \"articles\"", "Hook \"audit\" saved"],
            ["fr", "Le hook « audit » a été créé pour « articles »", "Hook « audit » enregistré"],
        ])("notifies successes in %s", async (language, created, saved) => {
            mockApi({ "POST /api/hook/articles": {}, "PUT /api/hook/id/h1": {} });
            const { result } = renderInLanguage(() => useHooks(), language);

            await act(() => result.current.create("articles", "post_read", "audit"));
            await act(() => result.current.update({ id: "h1", name: "audit" }));

            await expectToast(created);
            await expectToast(saved);
        });

        it.each([
            ["en", "Failed to create a hook for \"articles\"", "Failed to save hook"],
            ["fr", "Échec de la création d'un hook pour « articles »", "Échec de l'enregistrement du hook"],
        ])("notifies failures in %s", async (language, createFailed, saveFailed) => {
            mockApi({ "POST /api/hook/articles": failure, "PUT /api/hook/id/h1": failure });
            const { result } = renderInLanguage(() => useHooks(), language);

            await act(() => expect(result.current.create("articles", "post_read", "audit")).rejects.toThrow());
            await act(() => expect(result.current.update({ id: "h1", name: "audit" })).rejects.toThrow());

            await expectToast(createFailed);
            await expectToast(saveFailed);
        });
    });

    describe("useEntities", () => {
        it.each([
            ["en", "Failed to delete \"a1\""],
            ["fr", "Échec de la suppression de « a1 »"],
        ])("notifies a failed deletion in %s", async (language, message) => {
            mockApi({ "DELETE /api/articles/a1": failure });
            const { result } = renderInLanguage(() => useEntities(), language);

            await act(() => expect(result.current.dropEntity("articles", "a1")).rejects.toThrow());

            await expectToast(message);
        });
    });

    describe("useUsers", () => {
        it.each([
            ["en", "User created successfully"],
            ["fr", "Utilisateur créé avec succès"],
        ])("notifies a created user in %s", async (language, message) => {
            mockApi({ "POST /api/security/user": {} });
            const { result } = renderInLanguage(() => useUsers(), language);

            await act(() => result.current.createUser({ username: "bob", password: "secret", role: "user" }));

            await expectToast(message);
        });

        it.each([
            ["en", "Failed to create user", "Failed to change user's password", "Failed to delete user"],
            ["fr", "Échec de la création de l'utilisateur", "Échec de la modification du mot de passe de l'utilisateur", "Échec de la suppression de l'utilisateur"],
        ])("notifies failures in %s", async (language, createFailed, passwordFailed, deleteFailed) => {
            mockApi({
                "POST /api/security/user": failure,
                "PUT /api/security/user/bob/password": failure,
                "DELETE /api/security/user/bob": failure,
            });
            const { result } = renderInLanguage(() => useUsers(), language);

            await act(() => expect(result.current.createUser({ username: "bob" })).rejects.toThrow());
            await act(() => expect(result.current.changeUserPassword("bob", "secret")).rejects.toThrow());
            await act(() => expect(result.current.deleteUser("bob")).rejects.toThrow());

            await expectToast(createFailed);
            await expectToast(passwordFailed);
            await expectToast(deleteFailed);
        });

        it("translates the fallback error stored by the hook", async () => {
            mockApi({ "POST /api/security/user": () => { throw undefined; } });
            const { result } = renderInLanguage(() => useUsers(), "fr");

            await act(() => expect(result.current.createUser({ username: "bob" })).rejects.toBeUndefined());

            expect(result.current.error).toBe("Erreur inconnue");
        });
    });
});
