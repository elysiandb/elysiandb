import { describe, expect, it } from "vitest";
import { bindDocumentLanguage } from "../../src/i18n/document.js";
import { useLanguageStore } from "../../src/i18n/store.js";

describe("bindDocumentLanguage", () => {
    it("sets the document language right away", () => {
        useLanguageStore.setState({ language: "fr" });
        const root = document.createElement("html");

        const unbind = bindDocumentLanguage(root);

        expect(root.lang).toBe("fr");
        unbind();
    });

    it("follows language changes until unbound", () => {
        const root = document.createElement("html");
        const unbind = bindDocumentLanguage(root);

        useLanguageStore.getState().setLanguage("fr");
        expect(root.lang).toBe("fr");

        useLanguageStore.getState().setLanguage("en");
        expect(root.lang).toBe("en");

        unbind();
        useLanguageStore.getState().setLanguage("fr");
        expect(root.lang).toBe("en");
    });

    it("targets the html element by default", () => {
        const unbind = bindDocumentLanguage();

        useLanguageStore.getState().setLanguage("fr");

        expect(document.documentElement.lang).toBe("fr");
        unbind();
    });
});
