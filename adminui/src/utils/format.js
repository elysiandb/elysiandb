export const EMPTY_VALUE = "–";

const BYTE_UNITS = ["byte", "kilobyte", "megabyte", "gigabyte", "terabyte"];
const DURATION_UNITS = [["day", 86400], ["hour", 3600], ["minute", 60], ["second", 1]];
const RELATIVE_UNITS = [["day", 86400], ["hour", 3600], ["minute", 60], ["second", 1]];

export function isNumber(value) {
    return typeof value === "number" && Number.isFinite(value);
}

function fractionDigits(value) {
    const abs = Math.abs(value);
    if (abs === 0 || abs >= 100) return 0;
    if (abs >= 10) return 1;
    return 2;
}

function toDate(input) {
    if (input === null || input === undefined || input === "") return null;
    const date = input instanceof Date ? input : new Date(input);
    return Number.isNaN(date.getTime()) ? null : date;
}

export function createFormatters(language, t) {
    const cache = new Map();

    const cached = (key, create) => {
        if (!cache.has(key)) {
            cache.set(key, create());
        }
        return cache.get(key);
    };

    const numberFormat = (options) =>
        cached(`number:${JSON.stringify(options)}`, () => new Intl.NumberFormat(language, options));

    const dateFormat = (options) =>
        cached(`date:${JSON.stringify(options)}`, () => new Intl.DateTimeFormat(language, options));

    const unit = (value, name, display = "short") => isNumber(value)
        ? numberFormat({ style: "unit", unit: name, unitDisplay: display, maximumFractionDigits: fractionDigits(value) }).format(value)
        : EMPTY_VALUE;

    const number = (value) => isNumber(value)
        ? numberFormat({ maximumFractionDigits: fractionDigits(value) }).format(value)
        : EMPTY_VALUE;

    const count = (value) => isNumber(value)
        ? numberFormat({ maximumFractionDigits: 0 }).format(value)
        : EMPTY_VALUE;

    const percent = (ratio) => isNumber(ratio)
        ? numberFormat({ style: "percent", maximumFractionDigits: fractionDigits(ratio * 100) }).format(ratio)
        : EMPTY_VALUE;

    const bytes = (value) => {
        if (!isNumber(value)) return EMPTY_VALUE;
        let scaled = value;
        let index = 0;
        while (Math.abs(scaled) >= 1024 && index < BYTE_UNITS.length - 1) {
            scaled /= 1024;
            index += 1;
        }
        return unit(scaled, BYTE_UNITS[index]);
    };

    const duration = (seconds) => {
        if (!isNumber(seconds)) return EMPTY_VALUE;
        let rest = Math.max(0, Math.round(seconds));
        const start = DURATION_UNITS.findIndex(([, size]) => rest >= size);
        if (start === -1) {
            return unit(0, "second", "narrow");
        }
        const parts = [];
        for (const [name, size] of DURATION_UNITS.slice(start, start + 2)) {
            const amount = Math.floor(rest / size);
            rest -= amount * size;
            if (amount > 0 || parts.length === 0) {
                parts.push(numberFormat({ style: "unit", unit: name, unitDisplay: "narrow" }).format(amount));
            }
        }
        return parts.join(" ");
    };

    const interval = (milliseconds) => milliseconds >= 60000
        ? unit(milliseconds / 60000, "minute", "narrow")
        : unit(milliseconds / 1000, "second", "narrow");

    const time = (input, { seconds = true, milliseconds = false } = {}) => {
        const date = toDate(input);
        if (!date) return EMPTY_VALUE;
        const options = { hour: "2-digit", minute: "2-digit", hourCycle: "h23" };
        if (seconds) options.second = "2-digit";
        if (milliseconds) options.fractionalSecondDigits = 3;
        return dateFormat(options).format(date);
    };

    const dateTime = (input) => {
        const date = toDate(input);
        return date ? dateFormat({ dateStyle: "medium", timeStyle: "medium" }).format(date) : EMPTY_VALUE;
    };

    const relative = (input, now) => {
        const date = toDate(input);
        if (!date || !isNumber(now)) return EMPTY_VALUE;
        const diff = Math.round((date.getTime() - now) / 1000);
        const format = cached("relative", () => new Intl.RelativeTimeFormat(language, { numeric: "auto" }));
        for (const [name, size] of RELATIVE_UNITS) {
            if (Math.abs(diff) >= size || name === "second") {
                return format.format(Math.trunc(diff / size), name);
            }
        }
        return EMPTY_VALUE;
    };

    const rate = (value) => isNumber(value) ? t("units.perSecond", { value: number(value) }) : EMPTY_VALUE;

    const perMinute = (value) => isNumber(value) ? t("units.perMinute", { value: number(value) }) : EMPTY_VALUE;

    return {
        number,
        count,
        percent,
        bytes,
        duration,
        interval,
        ms: (value) => unit(value, "millisecond"),
        seconds: (value) => duration(value),
        time,
        dateTime,
        relative,
        rate,
        perMinute,
        range: (minutes) => minutes >= 60 ? unit(minutes / 60, "hour") : unit(minutes, "minute"),
    };
}
