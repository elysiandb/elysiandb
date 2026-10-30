import { describe, expect, it } from "vitest";
import { DEFAULT_LANGUAGE, LANGUAGES, detectLanguage, isSupportedLanguage } from "../../src/i18n/languages.js";

describe("languages", () => {
    it("uses English as default language", () => {
        expect(DEFAULT_LANGUAGE).toBe("en");
        expect(isSupportedLanguage(DEFAULT_LANGUAGE)).toBe(true);
    });

    it("offers English and French", () => {
        expect(LANGUAGES.map(({ code }) => code)).toEqual(["en", "fr"]);
    });

    it("labels each language in its own language", () => {
        expect(LANGUAGES.map(({ label }) => label)).toEqual(["English", "Français"]);
    });

    it.each([
        ["en", true],
        ["fr", true],
        ["de", false],
        ["EN", false],
        ["", false],
        [undefined, false],
        [null, false],
    ])("isSupportedLanguage(%j) is %s", (code, expected) => {
        expect(isSupportedLanguage(code)).toBe(expected);
    });
});

describe("detectLanguage", () => {
    it.each([
        [["fr-FR", "en-US"], "fr"],
        [["fr"], "fr"],
        [["FR-ca"], "fr"],
        [["en-GB", "fr"], "en"],
        [["de-DE", "fr-FR"], "fr"],
        [["de-DE", "es"], "en"],
        [[], "en"],
        [undefined, "en"],
        [null, "en"],
    ])("detects %j as %s", (preferences, expected) => {
        expect(detectLanguage(preferences)).toBe(expected);
    });
});
