import { create } from "zustand";
import { combine, persist } from "zustand/middleware";
import { detectLanguage, isSupportedLanguage } from "./languages.js";

export const useLanguageStore = create(
    persist(
        combine(
            {
                language: detectLanguage(navigator.languages),
            },
            (set) => ({
                setLanguage: (language) => {
                    if (isSupportedLanguage(language)) {
                        set({ language });
                    }
                },
            })
        ),
        {
            name: "language",
            merge: (persisted, current) => ({
                ...current,
                language: isSupportedLanguage(persisted?.language) ? persisted.language : current.language,
            }),
        }
    )
);
