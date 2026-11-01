import { describe, expect, it, vi } from "vitest";
import { createFormatters } from "../../src/utils/format.js";
import { translate } from "../../src/i18n/translate.js";
import { mergeSeries, seriesFetcher, sum, values } from "../../src/utils/series.js";
import { logsQuery, readFilters } from "../../src/utils/logs.js";

function formatters(language) {
    return createFormatters(language, (key, params) => translate(language, key, params));
}

describe("format", () => {
    it("formats numbers, units and durations for the language", () => {
        const en = formatters("en");
        const fr = formatters("fr");

        expect(en.count(120394)).toBe("120,394");
        expect(fr.count(120394)).toBe("120\u202f394");
        expect(en.rate(1284)).toBe("1,284/s");
        expect(en.perMinute(2.5)).toBe("2.5/min");
        expect(en.percent(0.0002)).toBe("0.02%");
        expect(en.bytes(1288490188)).toBe("1.2 GB");
        expect(fr.bytes(1288490188)).toBe("1,2\u202fGo");
        expect(en.duration(13500)).toBe("3h 45m");
        expect(en.duration(86400 * 3 + 3600 * 4)).toBe("3d 4h");
        expect(en.duration(42)).toBe("42s");
        expect(en.ms(0.8)).toBe("0.8 ms");
        expect(en.count(null)).toBe("–");
    });
});

describe("series", () => {
    const first = { range: "15m", slow_request_ms: 500, time: [1000, 2000, 3000], series: { http_rps: [1, 2, 3] } };

    it("appends only the newer points and keeps every series aligned", () => {
        const merged = mergeSeries(first, { time: [3000, 4000], series: { http_rps: [3, 4], tcp_rps: [9, 8] } }, 15);

        expect(merged.time).toEqual([1000, 2000, 3000, 4000]);
        expect(merged.series.http_rps).toEqual([1, 2, 3, 4]);
        expect(merged.series.tcp_rps).toEqual([null, null, null, 8]);
        expect(merged.slow_request_ms).toBe(500);
    });

    it("drops the points older than the range", () => {
        const start = 0;
        const minute = 60000;
        const old = { time: [start, start + minute], series: { http_rps: [1, 2] } };
        const merged = mergeSeries(old, { time: [start + 16 * minute], series: { http_rps: [3] } }, 15);

        expect(merged.time).toEqual([start + minute, start + 16 * minute]);
        expect(merged.series.http_rps).toEqual([2, 3]);
    });

    it("asks only for the points after the last one it has", async () => {
        const fetchMock = vi.fn(async (url) => {
            const since = new URL(url).searchParams.get("since");
            const body = since === null ? first : { time: [4000], series: { http_rps: [4] } };
            return new Response(JSON.stringify(body), { status: 200 });
        });
        vi.stubGlobal("fetch", fetchMock);

        const fetchSeries = seriesFetcher("1h");
        await fetchSeries();
        const second = await fetchSeries();

        const urls = fetchMock.mock.calls.map(([url]) => new URL(url));
        expect(urls[0].pathname).toBe("/monitoring/series");
        expect(urls[0].searchParams.get("range")).toBe("1h");
        expect(urls[0].searchParams.has("since")).toBe(false);
        expect(urls[1].searchParams.get("since")).toBe("3000");
        expect(second.series.http_rps).toEqual([1, 2, 3, 4]);
    });

    it("scales and sums series", () => {
        const data = { time: [1, 2], series: { a: [1, null], b: [2, 3] } };

        expect(values(data, "a", 60)).toEqual([60, null]);
        expect(values(data, "missing")).toEqual([null, null]);
        expect(sum(data, ["a", "b"])).toEqual([3, 3]);
    });
});

describe("logs query", () => {
    it("keeps only the filters that are set and turns the period into a start time", () => {
        const filters = readFilters(new URLSearchParams("level=warn&component=http&range=1h&q=timeout"));
        const query = new URLSearchParams(logsQuery(filters, { limit: 200, after: undefined }, 3_600_000 * 2));

        expect(Object.fromEntries(query)).toEqual({
            level: "warn",
            component: "http",
            q: "timeout",
            since: "3600000",
            limit: "200",
        });
    });
});
