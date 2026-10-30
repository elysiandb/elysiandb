import { useTranslation } from "../hooks/i18n/useTranslation.js";

export default function Home() {
    const { t } = useTranslation();

    return <div>
        <h1>{t("home.title")}</h1>
    </div>
}
