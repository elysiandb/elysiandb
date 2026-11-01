import { act, renderHook, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import RefreshControl from "../../src/components/monitoring/RefreshControl.jsx";
import { readStoredInterval, useStoredInterval } from "../../src/hooks/monitoring/useStoredInterval.js";
import { expectNoRawTranslationKeys, renderWithProviders } from "../helpers.jsx";

describe("useStoredInterval", () => {
    it("uses the default until an interval is chosen, then remembers it per page", () => {
        const { result } = renderHook(() => useStoredInterval("dashboard", 5000));
        expect(result.current[0]).toBe(5000);

        act(() => result.current[1](30000));

        expect(result.current[0]).toBe(30000);
        expect(localStorage.getItem("refresh.dashboard")).toBe("30000");
        expect(renderHook(() => useStoredInterval("dashboard", 5000)).result.current[0]).toBe(30000);
        expect(renderHook(() => useStoredInterval("monitoring.15m", 2000)).result.current[0]).toBe(2000);
    });

    it("ignores values that are not offered", () => {
        localStorage.setItem("refresh.dashboard", "1234");

        expect(readStoredInterval("dashboard", 5000)).toBe(5000);
    });

    it("works when the storage is not available", () => {
        vi.spyOn(Storage.prototype, "getItem").mockImplementation(() => {
            throw new Error("blocked");
        });
        vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
            throw new Error("blocked");
        });

        const { result } = renderHook(() => useStoredInterval("dashboard", 5000));
        expect(result.current[0]).toBe(5000);

        act(() => result.current[1](10000));
        expect(result.current[0]).toBe(10000);
    });
});

describe("RefreshControl", () => {
    it("offers the refresh intervals and pause in English", async () => {
        const user = userEvent.setup();
        const onIntervalChange = vi.fn();
        const onPausedChange = vi.fn();
        renderWithProviders(
            <RefreshControl
                interval={5000}
                onIntervalChange={onIntervalChange}
                paused={false}
                onPausedChange={onPausedChange}
                lastUpdated={null}
            />
        );

        const select = screen.getByRole("combobox", { name: "Refresh interval" });
        expect(select).toHaveValue("5000");
        expect([...select.options].map((option) => option.value)).toEqual(["0", "2000", "5000", "10000", "30000", "60000"]);
        expect(select.options[0]).toHaveTextContent("Off");
        expect(screen.getByText("Not updated yet")).toBeInTheDocument();

        await user.selectOptions(select, "30000");
        expect(onIntervalChange).toHaveBeenCalledWith(30000);

        await user.click(screen.getByRole("button", { name: "Pause" }));
        expect(onPausedChange).toHaveBeenCalledWith(true);
        expectNoRawTranslationKeys();
    });

    it("is translated in French", () => {
        renderWithProviders(
            <RefreshControl interval={0} onIntervalChange={() => {}} paused onPausedChange={() => {}} lastUpdated={Date.now()} />,
            { language: "fr" }
        );

        expect(screen.getByRole("combobox", { name: "Intervalle de rafraîchissement" })).toHaveValue("0");
        expect(screen.getByRole("button", { name: "Reprendre" })).toBeInTheDocument();
        expect(screen.getByText(/Mis à jour/)).toBeInTheDocument();
        expectNoRawTranslationKeys();
    });
});
