import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { BACKOFF_DELAYS, nextDelay, usePolling } from "../../src/hooks/monitoring/usePolling.js";

let visibility = "visible";

function setVisibility(value) {
    visibility = value;
    document.dispatchEvent(new Event("visibilitychange"));
}

async function advance(ms) {
    await act(async () => {
        await vi.advanceTimersByTimeAsync(ms);
    });
}

describe("usePolling", () => {
    beforeEach(() => {
        vi.useFakeTimers();
        visibility = "visible";
        Object.defineProperty(document, "visibilityState", { configurable: true, get: () => visibility });
    });

    afterEach(() => {
        vi.useRealTimers();
        delete document.visibilityState;
    });

    it("fetches at once and then at every interval", async () => {
        const fetcher = vi.fn().mockResolvedValue({ value: 1 });
        const { result } = renderHook(() => usePolling(fetcher, 2000));

        await advance(0);
        expect(fetcher).toHaveBeenCalledTimes(1);
        expect(result.current.data).toEqual({ value: 1 });
        expect(result.current.lastUpdated).not.toBeNull();

        await advance(2000);
        expect(fetcher).toHaveBeenCalledTimes(2);

        await advance(2000);
        expect(fetcher).toHaveBeenCalledTimes(3);
    });

    it("keeps a single request in flight", async () => {
        let resolve;
        const fetcher = vi.fn(() => new Promise((done) => {
            resolve = done;
        }));
        const { result } = renderHook(() => usePolling(fetcher, 1000));

        await advance(5000);
        act(() => {
            result.current.refresh();
        });
        act(() => {
            result.current.refresh();
        });
        expect(fetcher).toHaveBeenCalledTimes(1);

        await act(async () => resolve({ ok: true }));
        expect(result.current.data).toEqual({ ok: true });

        await advance(1000);
        expect(fetcher).toHaveBeenCalledTimes(2);
    });

    it("stops while the tab is hidden and catches up when it is visible again", async () => {
        const fetcher = vi.fn().mockResolvedValue({});
        renderHook(() => usePolling(fetcher, 1000));
        await advance(0);
        expect(fetcher).toHaveBeenCalledTimes(1);

        act(() => setVisibility("hidden"));
        await advance(10000);
        expect(fetcher).toHaveBeenCalledTimes(1);

        act(() => setVisibility("visible"));
        await advance(0);
        expect(fetcher).toHaveBeenCalledTimes(2);

        await advance(1000);
        expect(fetcher).toHaveBeenCalledTimes(3);
    });

    it("backs off after errors and flags the lost connection", async () => {
        const fetcher = vi.fn().mockRejectedValue(new Error("down"));
        const { result } = renderHook(() => usePolling(fetcher, 1000));

        await advance(0);
        expect(fetcher).toHaveBeenCalledTimes(1);
        expect(result.current.connectionLost).toBe(true);

        await advance(4999);
        expect(fetcher).toHaveBeenCalledTimes(1);
        await advance(1);
        expect(fetcher).toHaveBeenCalledTimes(2);

        await advance(9999);
        expect(fetcher).toHaveBeenCalledTimes(2);
        await advance(1);
        expect(fetcher).toHaveBeenCalledTimes(3);

        fetcher.mockResolvedValue({ back: true });
        await advance(30000);
        expect(fetcher).toHaveBeenCalledTimes(4);
        expect(result.current.connectionLost).toBe(false);
        expect(result.current.data).toEqual({ back: true });

        await advance(1000);
        expect(fetcher).toHaveBeenCalledTimes(5);
    });

    it("does not poll while paused or when the interval is off", async () => {
        const fetcher = vi.fn().mockResolvedValue({});
        const { result, rerender } = renderHook(({ interval }) => usePolling(fetcher, interval), { initialProps: { interval: 0 } });

        await advance(10000);
        expect(fetcher).toHaveBeenCalledTimes(1);

        rerender({ interval: 1000 });
        await advance(0);
        const afterChange = fetcher.mock.calls.length;

        act(() => result.current.setPaused(true));
        await advance(10000);
        expect(fetcher).toHaveBeenCalledTimes(afterChange);

        act(() => result.current.setPaused(false));
        await advance(0);
        expect(fetcher).toHaveBeenCalledTimes(afterChange + 1);
    });

    it("drops the answer of a fetcher that was replaced", async () => {
        let resolveOld;
        const oldFetcher = vi.fn(() => new Promise((done) => {
            resolveOld = done;
        }));
        const newFetcher = vi.fn().mockResolvedValue({ from: "new" });
        const { result, rerender } = renderHook(({ fetcher }) => usePolling(fetcher, 1000), { initialProps: { fetcher: oldFetcher } });

        await advance(0);
        rerender({ fetcher: newFetcher });
        await advance(0);
        await act(async () => resolveOld({ from: "old" }));

        expect(result.current.data).toEqual({ from: "new" });
    });
});

describe("nextDelay", () => {
    it("uses the interval, then the backoff steps", () => {
        expect(nextDelay(2000, 0)).toBe(2000);
        expect(nextDelay(0, 0)).toBeNull();
        expect(BACKOFF_DELAYS.map((_, index) => nextDelay(2000, index + 1))).toEqual([5000, 10000, 30000, 60000]);
        expect(nextDelay(2000, 12)).toBe(60000);
    });
});
