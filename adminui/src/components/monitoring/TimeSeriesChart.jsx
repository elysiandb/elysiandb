import { useEffect, useMemo, useRef } from "react";
import uPlot from "uplot";
import "uplot/dist/uPlot.min.css";
import { EMPTY_VALUE } from "../../utils/format.js";

const COLORS = {
    accent: ["--accent", "#f7d64a"],
    cyan: ["--bs-cyan", "#0dcaf0"],
    green: ["--bs-green", "#198754"],
    red: ["--bs-red", "#dc3545"],
    purple: ["--bs-purple", "#6f42c1"],
    orange: ["--bs-orange", "#fd7e14"],
    teal: ["--bs-teal", "#20c997"],
    pink: ["--bs-pink", "#d63384"],
    indigo: ["--bs-indigo", "#6610f2"],
};

const PALETTE = ["accent", "cyan", "green", "red", "purple", "orange", "teal", "pink", "indigo"];

function cssVariable(name, fallback) {
    if (typeof document === "undefined") return fallback;
    const value = getComputedStyle(document.documentElement).getPropertyValue(name).trim();
    return value || fallback;
}

function resolveColor(name, index) {
    const [variable, fallback] = COLORS[name] ?? COLORS[PALETTE[index % PALETTE.length]];
    return cssVariable(variable, fallback);
}

function translucent(color) {
    return /^#[0-9a-f]{6}$/i.test(color) ? `${color}33` : color;
}

function dataRange(seriesCount) {
    return (u) => {
        let low = 0;
        let high = 0;
        for (let index = 1; index <= seriesCount; index++) {
            for (const value of u.data[index] ?? []) {
                if (value === null || value === undefined) continue;
                low = Math.min(low, value);
                high = Math.max(high, value);
            }
        }
        return [low, high > 0 ? high * 1.1 : 1];
    };
}

function buildOptions({ width, height, series, threshold, sparkline, syncKey, formatRef, formatTimeRef }) {
    const axisColor = cssVariable("--text-dim", "#a5aac5");
    const gridColor = "rgba(255, 255, 255, 0.07)";
    const formatValue = (u, value) => (value === null || value === undefined ? EMPTY_VALUE : formatRef.current(value));

    const plotted = series.map((item, index) => {
        const stroke = resolveColor(item.color, index);
        return {
            label: item.label,
            stroke,
            width: 1.5,
            fill: sparkline ? translucent(stroke) : undefined,
            dash: item.dash,
            points: { show: false },
            value: formatValue,
        };
    });

    if (threshold) {
        plotted.push({
            label: threshold.label,
            stroke: resolveColor("red", 0),
            width: 1,
            dash: [6, 6],
            points: { show: false },
            value: formatValue,
        });
    }

    const axis = {
        stroke: axisColor,
        grid: { stroke: gridColor, width: 1 },
        ticks: { stroke: gridColor, width: 1 },
    };

    return {
        width,
        height,
        legend: { show: !sparkline },
        cursor: sparkline
            ? { show: false }
            : { drag: { x: false, y: false }, ...(syncKey ? { sync: { key: syncKey } } : {}) },
        select: { show: false },
        scales: { x: { time: true }, y: { range: dataRange(series.length) } },
        axes: sparkline
            ? [{ show: false }, { show: false }]
            : [
                { ...axis, space: 70, values: (u, splits) => splits.map((value) => formatTimeRef.current(value * 1000)) },
                { ...axis, size: 72, values: (u, splits) => splits.map((value) => formatRef.current(value)) },
            ],
        series: [
            { value: (u, value) => (value === null || value === undefined ? EMPTY_VALUE : formatTimeRef.current(value * 1000)) },
            ...plotted,
        ],
    };
}

export default function TimeSeriesChart({
    time,
    series,
    format,
    formatTime,
    height = 180,
    syncKey,
    threshold,
    sparkline = false,
    label,
}) {
    const containerRef = useRef(null);
    const plotRef = useRef(null);
    const formatRef = useRef(format);
    const formatTimeRef = useRef(formatTime);

    const data = useMemo(() => {
        const xs = (time ?? []).map((value) => value / 1000);
        const ys = series.map((item) => item.values ?? []);
        if (threshold) {
            ys.push(xs.map(() => threshold.value));
        }
        return [xs, ...ys];
    }, [time, series, threshold]);

    const latest = useRef({ data, series, threshold, sparkline, height, syncKey });

    useEffect(() => {
        formatRef.current = format;
        formatTimeRef.current = formatTime;
        latest.current = { data, series, threshold, sparkline, height, syncKey };
    });

    const signature = JSON.stringify([
        series.map((item) => [item.label, item.color, item.dash]),
        threshold ? threshold.label : null,
        sparkline,
        height,
        syncKey ?? null,
    ]);

    useEffect(() => {
        const container = containerRef.current;
        if (!container) return undefined;

        const config = latest.current;
        const options = buildOptions({
            width: container.clientWidth || 300,
            height: config.height,
            series: config.series,
            threshold: config.threshold,
            sparkline: config.sparkline,
            syncKey: config.syncKey,
            formatRef,
            formatTimeRef,
        });
        const plot = new uPlot(options, config.data, container);
        plotRef.current = plot;

        let observer = null;
        if (typeof ResizeObserver !== "undefined") {
            observer = new ResizeObserver((entries) => {
                const width = Math.floor(entries[0]?.contentRect?.width ?? 0);
                if (width > 0) {
                    plot.setSize({ width, height: config.height });
                }
            });
            observer.observe(container);
        }

        return () => {
            observer?.disconnect();
            plot.destroy();
            plotRef.current = null;
        };
    }, [signature]);

    useEffect(() => {
        plotRef.current?.setData(data);
    }, [data]);

    return (
        <div
            ref={containerRef}
            className={sparkline ? "monitoring-sparkline" : "monitoring-chart"}
            role="img"
            aria-label={label}
        />
    );
}
