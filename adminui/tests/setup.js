import "@testing-library/jest-dom/vitest";
import { afterEach, beforeEach, vi } from "vitest";
import { cleanup } from "@testing-library/react";
import uPlot from "uplot";
import { useLanguageStore } from "../src/i18n/store.js";
import { useAccountStore } from "../src/account/store.ts";

vi.mock("uplot", () => ({
    default: class Plot {
        static instances = [];

        constructor(options, data, target) {
            this.options = options;
            this.data = data;
            this.target = target;
            Plot.instances.push(this);
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
    uPlot.instances.length = 0;
    localStorage.clear();
    vi.restoreAllMocks();
    vi.unstubAllGlobals();
});
