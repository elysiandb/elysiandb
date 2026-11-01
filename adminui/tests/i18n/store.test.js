import { afterEach, describe, expect, it, vi } from "vitest";
import { useLanguageStore } from "../../src/i18n/store.js";

function storedLanguage() {
    return JSON.parse(localStorage.getItem("language")).state.language;
}

async function loadFreshStore(browserLanguages) {
    vi.spyOn(navigator, "languages", "get").mockReturnValue(browserLanguages);
    vi.resetModules();
    const module = await import("../../src/i18n/store.js");
    return module.useLanguageStore;
}

describe("language store", () => {
    afterEach(() => {
        vi.resetModules();
    });

    it("changes and persists the language", () => {
        useLanguageStore.getState().setLanguage("fr");

        expect(useLanguageStore.getState().language).toBe("fr");
        expect(storedLanguage()).toBe("fr");
    });

    it("ignores unsupported languages", () => {
        useLanguageStore.getState().setLanguage("fr");
        useLanguageStore.getState().setLanguage("it");
        useLanguageStore.getState().setLanguage(undefined);

        expect(useLanguageStore.getState().language).toBe("fr");
        expect(storedLanguage()).toBe("fr");
    });

    it("starts with the browser language when nothing is stored", async () => {
        localStorage.removeItem("language");

        const store = await loadFreshStore(["fr-FR", "en-US"]);

        expect(store.getState().language).toBe("fr");
    });

    it.each([
        [["es-MX"], "es"],
        [["zh-TW", "en"], "zh"],
        [["de-CH", "fr"], "de"],
        [["ja"], "ja"],
        [["ko-KR"], "ko"],
        [["pt-PT"], "pt"],
        [["nl-BE"], "nl"],
    ])("starts with the browser language %j as %s", async (browserLanguages, expected) => {
        localStorage.removeItem("language");

        const store = await loadFreshStore(browserLanguages);

        expect(store.getState().language).toBe(expected);
    });

    it("starts in English when the browser language is not supported", async () => {
        localStorage.removeItem("language");

        const store = await loadFreshStore(["it-IT", "ru"]);

        expect(store.getState().language).toBe("en");
    });

    it("restores the stored language over the browser language", async () => {
        localStorage.setItem("language", JSON.stringify({ state: { language: "fr" }, version: 0 }));

        const store = await loadFreshStore(["en-US"]);

        expect(store.getState().language).toBe("fr");
    });

    it("ignores an invalid stored language", async () => {
        localStorage.setItem("language", JSON.stringify({ state: { language: "klingon" }, version: 0 }));

        const store = await loadFreshStore(["fr-FR"]);

        expect(store.getState().language).toBe("fr");
    });

    it("ignores corrupted stored state", async () => {
        localStorage.setItem("language", JSON.stringify({ state: null, version: 0 }));

        const store = await loadFreshStore(["en-US"]);

        expect(store.getState().language).toBe("en");
        expect(typeof store.getState().setLanguage).toBe("function");
    });
});
