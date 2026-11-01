import { useMemo } from "react";
import { createFormatters } from "../../utils/format.js";
import { useTranslation } from "./useTranslation.js";

export function useFormatters() {
    const { t, language } = useTranslation();

    return useMemo(() => createFormatters(language, t), [language, t]);
}
