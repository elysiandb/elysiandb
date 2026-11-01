import { apiFetch } from "./api";

export const RANGES = {
    "15m": { minutes: 15, defaultInterval: 2000 },
    "1h": { minutes: 60, defaultInterval: 30000 },
    "24h": { minutes: 1440, defaultInterval: 60000 },
};

function padded(values, length) {
    return values ?? new Array(length).fill(null);
}

export function mergeSeries(previous, response, rangeMinutes) {
    const incoming = response?.time ?? [];
    if (!previous) {
        return trimSeries({ ...response, time: [...incoming], series: { ...(response?.series ?? {}) } }, rangeMinutes);
    }

    const last = previous.time.length ? previous.time[previous.time.length - 1] : -Infinity;
    const start = incoming.findIndex((value) => value > last);
    const fresh = start === -1 ? 0 : incoming.length - start;
    const offset = incoming.length - fresh;
    const names = new Set([...Object.keys(previous.series ?? {}), ...Object.keys(response?.series ?? {})]);
    const series = {};

    for (const name of names) {
        const before = padded(previous.series?.[name], previous.time.length);
        const after = padded(response?.series?.[name], incoming.length).slice(offset);
        series[name] = [...before, ...after];
    }

    return trimSeries({
        ...previous,
        ...response,
        time: [...previous.time, ...incoming.slice(offset)],
        series,
    }, rangeMinutes);
}

export function trimSeries(data, rangeMinutes) {
    if (!data.time.length) return data;
    const cutoff = data.time[data.time.length - 1] - rangeMinutes * 60000;
    const start = data.time.findIndex((value) => value >= cutoff);
    if (start <= 0) return data;

    const series = {};
    for (const [name, values] of Object.entries(data.series)) {
        series[name] = values.slice(start);
    }

    return { ...data, time: data.time.slice(start), series };
}

export function seriesFetcher(range) {
    const minutes = RANGES[range]?.minutes ?? 15;
    let latest = null;

    return async () => {
        const since = latest?.time?.length ? latest.time[latest.time.length - 1] : null;
        const query = since === null ? `range=${range}` : `range=${range}&since=${since}`;
        const response = await apiFetch(`/monitoring/series?${query}`);
        latest = mergeSeries(latest, response, minutes);
        return latest;
    };
}

export function values(data, name, scale = 1) {
    const raw = data?.series?.[name];
    if (!raw) return data?.time ? new Array(data.time.length).fill(null) : [];
    if (scale === 1) return raw;
    return raw.map((value) => (value === null || value === undefined ? null : value * scale));
}

export function sum(data, names) {
    const length = data?.time?.length ?? 0;
    const result = new Array(length).fill(null);
    for (const name of names) {
        const raw = data?.series?.[name];
        if (!raw) continue;
        for (let index = 0; index < length; index += 1) {
            const value = raw[index];
            if (value !== null && value !== undefined) {
                result[index] = (result[index] ?? 0) + value;
            }
        }
    }
    return result;
}
