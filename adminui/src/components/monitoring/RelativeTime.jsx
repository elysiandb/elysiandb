import { useNow } from "../../hooks/monitoring/useNow.js";
import { useFormatters } from "../../hooks/i18n/useFormatters.js";

export default function RelativeTime({ value, title }) {
    const now = useNow(1000);
    const format = useFormatters();

    return <span title={title ?? format.dateTime(value)}>{format.relative(value, now)}</span>;
}
