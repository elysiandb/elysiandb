import { useCallback, useState } from "react";

export const REFRESH_INTERVALS = [0, 2000, 5000, 10000, 30000, 60000];

const STORAGE_PREFIX = "refresh.";

export function readStoredInterval(key, fallback) {
    try {
        const stored = localStorage.getItem(STORAGE_PREFIX + key);
        const value = stored === null ? NaN : Number(stored);
        return REFRESH_INTERVALS.includes(value) ? value : fallback;
    } catch {
        return fallback;
    }
}

export function writeStoredInterval(key, value) {
    try {
        localStorage.setItem(STORAGE_PREFIX + key, String(value));
    } catch {
        return;
    }
}

export function useStoredInterval(key, fallback) {
    const [state, setState] = useState(() => ({ key, value: readStoredInterval(key, fallback) }));

    const value = state.key === key ? state.value : readStoredInterval(key, fallback);

    const setValue = useCallback((next) => {
        writeStoredInterval(key, next);
        setState({ key, value: next });
    }, [key]);

    return [value, setValue];
}
