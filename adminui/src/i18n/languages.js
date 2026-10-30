export const DEFAULT_LANGUAGE = "en";

export const LANGUAGES = [
    { code: "en", label: "English" },
    { code: "fr", label: "Français" },
];

export function isSupportedLanguage(code) {
    return LANGUAGES.some((language) => language.code === code);
}

export function detectLanguage(preferences) {
    for (const preference of preferences ?? []) {
        const code = String(preference).toLowerCase().split("-")[0];
        if (isSupportedLanguage(code)) {
            return code;
        }
    }
    return DEFAULT_LANGUAGE;
}
