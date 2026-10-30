import { useLanguageStore } from "./store.js";

export function bindDocumentLanguage(root = document.documentElement) {
    root.lang = useLanguageStore.getState().language;

    return useLanguageStore.subscribe((state) => {
        root.lang = state.language;
    });
}
