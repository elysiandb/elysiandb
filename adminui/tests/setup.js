import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import { useLanguageStore } from "../src/i18n/store.js";
import { useAccountStore } from "../src/account/store.ts";

vi.mock("uplot", () => ({
    default: class {
        constructor(options, data, target) {
            this.options = options;
            this.data = data;
            this.target = target;
        }

        setData(data) {
            this.data = data;
        }

        setSize() {}

        destroy() {}
    },
}));

beforeEach(() => {
    useLanguageStore.setState({ language: "en" });
    useAccountStore.setState({ account: { id: 1, username: "admin" } });
});

afterEach(() => {
    cleanup();
    localStorage.clear();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
});
