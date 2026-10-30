import { act, renderHook } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { useTranslation } from "../../src/hooks/i18n/useTranslation.js";

describe("useTranslation", () => {
    it("translates with the current language", () => {
        const { result } = renderHook(() => useTranslation());

        expect(result.current.language).toBe("en");
        expect(result.current.t("nav.logout")).toBe("Logout");
    });

    it("re-renders with the new language", () => {
        const { result } = renderHook(() => useTranslation());

        act(() => result.current.setLanguage("fr"));

        expect(result.current.language).toBe("fr");
        expect(result.current.t("nav.logout")).toBe("Déconnexion");
        expect(result.current.t("nav.hello", { username: "alice" })).toBe("Bonjour alice");
    });

    it("keeps the same t function while the language does not change", () => {
        const { result, rerender } = renderHook(() => useTranslation());
        const first = result.current.t;

        rerender();
        expect(result.current.t).toBe(first);

        act(() => result.current.setLanguage("fr"));
        expect(result.current.t).not.toBe(first);
    });
});
