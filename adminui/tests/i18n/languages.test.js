import { describe, expect, it } from "vitest";
import { DEFAULT_LANGUAGE, LANGUAGES, detectLanguage, isSupportedLanguage } from "../../src/i18n/languages.js";

describe("languages", () => {
    it("uses English as default language", () => {
        expect(DEFAULT_LANGUAGE).toBe("en");
        expect(isSupportedLanguage(DEFAULT_LANGUAGE)).toBe(true);
    });

    it("offers English, French, Spanish, Chinese, German, Japanese, Korean, Portuguese and Dutch", () => {
        expect(LANGUAGES.map(({ code }) => code)).toEqual(["en", "fr", "es", "zh", "de", "ja", "ko", "pt", "nl"]);
    });

    it("labels each language in its own language", () => {
        expect(LANGUAGES.map(({ label }) => label)).toEqual([
            "English",
            "Français",
            "Español",
            "简体中文",
            "Deutsch",
            "日本語",
            "한국어",
            "Português",
            "Nederlands",
        ]);
    });

    it.each([
        ["en", true],
        ["fr", true],
        ["es", true],
        ["zh", true],
        ["de", true],
        ["ja", true],
        ["ko", true],
        ["pt", true],
        ["nl", true],
        ["it", false],
        ["zh-TW", false],
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
        [["it-IT", "fr-FR"], "fr"],
        [["it-IT", "ru"], "en"],
        [["es-ES"], "es"],
        [["es-419"], "es"],
        [["zh-CN"], "zh"],
        [["zh-TW"], "zh"],
        [["zh-HK"], "zh"],
        [["zh-Hans-CN"], "zh"],
        [["de-DE"], "de"],
        [["de-AT", "en"], "de"],
        [["ja-JP"], "ja"],
        [["ko-KR"], "ko"],
        [["pt-BR"], "pt"],
        [["pt-PT"], "pt"],
        [["nl-NL"], "nl"],
        [["nl-BE", "fr-BE"], "nl"],
        [[], "en"],
        [undefined, "en"],
        [null, "en"],
    ])("detects %j as %s", (preferences, expected) => {
        expect(detectLanguage(preferences)).toBe(expected);
    });
});
