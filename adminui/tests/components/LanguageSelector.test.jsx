import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";
import LanguageSelector from "../../src/components/i18n/LanguageSelector.jsx";
import { useLanguageStore } from "../../src/i18n/store.js";
import { renderWithProviders } from "../helpers.jsx";

describe("LanguageSelector", () => {
    it("lists the available languages in their own language", () => {
        renderWithProviders(<LanguageSelector />);

        const options = screen.getAllByRole("option");
        expect(options.map((option) => option.textContent)).toEqual([
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
        expect(options.map((option) => option.getAttribute("lang"))).toEqual(["en", "fr", "es", "zh", "de", "ja", "ko", "pt", "nl"]);
    });

    it("shows the current language", () => {
        renderWithProviders(<LanguageSelector />, { language: "fr" });

        expect(screen.getByRole("combobox", { name: "Langue" })).toHaveValue("fr");
    });

    it("changes and remembers the language", async () => {
        const user = userEvent.setup();
        renderWithProviders(<LanguageSelector />);

        await user.selectOptions(screen.getByRole("combobox", { name: "Language" }), "fr");

        expect(useLanguageStore.getState().language).toBe("fr");
        expect(JSON.parse(localStorage.getItem("language")).state.language).toBe("fr");
        expect(screen.getByRole("combobox", { name: "Langue" })).toHaveValue("fr");

        await user.selectOptions(screen.getByRole("combobox", { name: "Langue" }), "en");

        expect(useLanguageStore.getState().language).toBe("en");
        expect(screen.getByRole("combobox", { name: "Language" })).toHaveValue("en");
    });
});
