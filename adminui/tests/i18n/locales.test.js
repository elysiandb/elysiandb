import { readdirSync, readFileSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { LANGUAGES } from "../../src/i18n/languages.js";
import { dictionaries } from "../../src/i18n/translate.js";

const SOURCE_DIR = resolve(dirname(fileURLToPath(import.meta.url)), "../../src");
const DYNAMIC_KEY_PREFIXES = ["configuration.fields.", "acl.permissions.", "browse.help."];

function flatten(node, prefix = "") {
    return Object.entries(node).flatMap(([key, value]) => {
        const path = prefix ? `${prefix}.${key}` : key;
        return typeof value === "object" && value !== null ? flatten(value, path) : [[path, value]];
    });
}

function placeholders(text) {
    return [...text.matchAll(/\{(\w+)\}/g)].map(([, name]) => name).sort();
}

function sourceFiles() {
    return readdirSync(SOURCE_DIR, { recursive: true })
        .filter((file) => /\.(jsx?|tsx?)$/.test(file))
        .map((file) => readFileSync(join(SOURCE_DIR, file), "utf8"));
}

const english = Object.fromEntries(flatten(dictionaries.en));
const namespaces = Object.keys(dictionaries.en);
const keyLiteralPattern = new RegExp(`["'\`]((?:${namespaces.join("|")})\\.[\\w.]+)["'\`]`, "g");
const usedKeys = new Set(sourceFiles().flatMap((source) => [...source.matchAll(keyLiteralPattern)].map(([, key]) => key)));

describe("locales", () => {
    it("has a dictionary for every offered language and no other", () => {
        expect(Object.keys(dictionaries).sort()).toEqual(LANGUAGES.map(({ code }) => code).sort());
    });

    describe.each(Object.keys(dictionaries))("%s dictionary", (language) => {
        const entries = flatten(dictionaries[language]);

        it("has exactly the same keys as English", () => {
            expect(entries.map(([key]) => key).sort()).toEqual(Object.keys(english).sort());
        });

        it("only contains non empty texts", () => {
            for (const [key, value] of entries) {
                expect(typeof value, key).toBe("string");
                expect(value.trim(), key).not.toBe("");
            }
        });

        it("uses the same placeholders as English", () => {
            for (const [key, value] of entries) {
                expect(placeholders(value), key).toEqual(placeholders(english[key]));
            }
        });
    });

    it("finds translation keys in the source code", () => {
        expect(usedKeys.size).toBeGreaterThan(50);
    });

    it("defines every key used in the source code", () => {
        const missing = [...usedKeys].filter((key) => english[key] === undefined);
        expect(missing).toEqual([]);
    });

    it("does not define keys that are never used", () => {
        const unused = Object.keys(english).filter((key) =>
            !usedKeys.has(key) && !DYNAMIC_KEY_PREFIXES.some((prefix) => key.startsWith(prefix))
        );
        expect(unused).toEqual([]);
    });
});
