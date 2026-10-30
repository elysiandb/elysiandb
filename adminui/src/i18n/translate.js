import en from "./locales/en.json";
import fr from "./locales/fr.json";
import { DEFAULT_LANGUAGE } from "./languages.js";

export const dictionaries = { en, fr };

function lookup(dictionary, key) {
    const value = key.split(".").reduce((node, part) => node?.[part], dictionary);
    return typeof value === "string" ? value : undefined;
}

export function translate(language, key, params = {}) {
    const template = lookup(dictionaries[language], key)
        ?? lookup(dictionaries[DEFAULT_LANGUAGE], key)
        ?? key;

    return template.replace(/\{(\w+)\}/g, (placeholder, name) =>
        Object.hasOwn(params, name) ? String(params[name] ?? "") : placeholder
    );
}
