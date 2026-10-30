import { useCallback } from "react";
import { useLanguageStore } from "../../i18n/store.js";
import { translate } from "../../i18n/translate.js";

export function useTranslation() {
    const language = useLanguageStore((state) => state.language);
    const setLanguage = useLanguageStore((state) => state.setLanguage);

    const t = useCallback((key, params) => translate(language, key, params), [language]);

    return { t, language, setLanguage };
}
