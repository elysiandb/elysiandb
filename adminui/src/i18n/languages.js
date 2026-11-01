export const DEFAULT_LANGUAGE = "en";

export const LANGUAGES = [
    { code: "en", label: "English" },
    { code: "fr", label: "Français" },
    { code: "es", label: "Español" },
    { code: "zh", label: "简体中文" },
    { code: "de", label: "Deutsch" },
    { code: "ja", label: "日本語" },
    { code: "ko", label: "한국어" },
    { code: "pt", label: "Português" },
    { code: "nl", label: "Nederlands" },
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
