import { useCallback, useEffect, useRef, useState } from "react";

export const BACKOFF_DELAYS = [5000, 10000, 30000, 60000];

const EMPTY_STATE = { data: undefined, error: null, lastUpdated: null, connectionLost: false, loading: true };

function documentIsHidden() {
    return typeof document !== "undefined" && document.visibilityState === "hidden";
}

export function nextDelay(intervalMs, failures) {
    if (failures > 0) {
        return BACKOFF_DELAYS[Math.min(failures, BACKOFF_DELAYS.length) - 1];
    }
    return intervalMs > 0 ? intervalMs : null;
}

export function usePolling(fetcher, intervalMs) {
    const [state, setState] = useState({ ...EMPTY_STATE, key: null });
    const [paused, setPaused] = useState(false);
    const activeFetcher = useRef(null);
    const inFlight = useRef(null);
    const failures = useRef(0);
    const timer = useRef(null);
    const settings = useRef({ intervalMs, paused });
    const runRef = useRef(() => {});

    const clearTimer = useCallback(() => {
        if (timer.current !== null) {
            clearTimeout(timer.current);
            timer.current = null;
        }
    }, []);

    const schedule = useCallback(() => {
        clearTimer();
        if (settings.current.paused || documentIsHidden()) return;
        const delay = nextDelay(settings.current.intervalMs, failures.current);
        if (delay === null) return;
        timer.current = setTimeout(() => runRef.current(), delay);
    }, [clearTimer]);

    const run = useCallback(async () => {
        if (activeFetcher.current !== fetcher || inFlight.current === fetcher) return;
        clearTimer();
        inFlight.current = fetcher;

        try {
            const data = await fetcher();
            if (activeFetcher.current !== fetcher) return;
            failures.current = 0;
            setState({ key: fetcher, data, error: null, lastUpdated: Date.now(), connectionLost: false, loading: false });
        } catch (error) {
            if (activeFetcher.current !== fetcher) return;
            failures.current += 1;
            setState((previous) => ({
                ...(previous.key === fetcher ? previous : EMPTY_STATE),
                key: fetcher,
                error,
                connectionLost: true,
                loading: false,
            }));
        } finally {
            if (inFlight.current === fetcher) {
                inFlight.current = null;
            }
            if (activeFetcher.current === fetcher) {
                schedule();
            }
        }
    }, [fetcher, clearTimer, schedule]);

    useEffect(() => {
        runRef.current = run;
        activeFetcher.current = fetcher;
        failures.current = 0;
        if (!settings.current.paused && !documentIsHidden()) {
            run();
        }

        return () => {
            clearTimer();
            if (activeFetcher.current === fetcher) {
                activeFetcher.current = null;
            }
        };
    }, [fetcher, run, clearTimer]);

    useEffect(() => {
        settings.current = { intervalMs, paused };
        if (paused) {
            clearTimer();
            return;
        }
        if (inFlight.current === null) {
            runRef.current();
        }
    }, [intervalMs, paused, clearTimer]);

    useEffect(() => {
        const onVisibilityChange = () => {
            if (documentIsHidden()) {
                clearTimer();
                return;
            }
            if (!settings.current.paused) {
                runRef.current();
            }
        };

        document.addEventListener("visibilitychange", onVisibilityChange);
        return () => document.removeEventListener("visibilitychange", onVisibilityChange);
    }, [clearTimer]);

    const refresh = useCallback(() => {
        runRef.current();
    }, []);

    const current = state.key === fetcher ? state : EMPTY_STATE;

    return {
        data: current.data,
        error: current.error,
        lastUpdated: current.lastUpdated,
        connectionLost: current.connectionLost,
        loading: current.loading,
        paused,
        setPaused,
        refresh,
    };
}
