import en from "./locales/en.json";
import fr from "./locales/fr.json";
import es from "./locales/es.json";
import zh from "./locales/zh.json";
import de from "./locales/de.json";
import ja from "./locales/ja.json";
import ko from "./locales/ko.json";
import pt from "./locales/pt.json";
import nl from "./locales/nl.json";
import { DEFAULT_LANGUAGE } from "./languages.js";

export const dictionaries = { en, fr, es, zh, de, ja, ko, pt, nl };

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
