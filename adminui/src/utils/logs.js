export const LOG_LEVELS = {
    debug: { label: "logs.levels.debug", variant: "secondary" },
    info: { label: "logs.levels.info", variant: "info" },
    warn: { label: "logs.levels.warn", variant: "warning" },
    error: { label: "logs.levels.error", variant: "danger" },
};

export const LOG_RANGES = {
    "15m": { minutes: 15, label: "logs.ranges.last15m" },
    "1h": { minutes: 60, label: "logs.ranges.last1h" },
    "24h": { minutes: 1440, label: "logs.ranges.last24h" },
};

export const FILTER_KEYS = ["level", "component", "q", "user", "request_id", "range"];

export const TEXT_FILTER_KEYS = ["q", "user", "request_id"];

export function levelOf(entry) {
    return LOG_LEVELS[String(entry?.level ?? "").toLowerCase()] ?? LOG_LEVELS.info;
}

export function readFilters(searchParams) {
    return Object.fromEntries(FILTER_KEYS.map((key) => [key, searchParams.get(key) ?? ""]));
}

export function filtersKey(filters) {
    return FILTER_KEYS.map((key) => `${key}=${filters[key] ?? ""}`).join("&");
}

export function logsQuery(filters, extra = {}, now = Date.now()) {
    const params = new URLSearchParams();

    for (const key of ["level", "component", "q", "user", "request_id"]) {
        if (filters[key]) {
            params.set(key, filters[key]);
        }
    }

    const range = LOG_RANGES[filters.range];
    if (range) {
        params.set("since", String(now - range.minutes * 60000));
    }

    for (const [key, value] of Object.entries(extra)) {
        if (value !== undefined && value !== null && value !== "") {
            params.set(key, String(value));
        }
    }

    return params.toString();
}

export function attributeText(value) {
    if (value === null || value === undefined) return "";
    if (typeof value === "object") return JSON.stringify(value);
    return String(value);
}
