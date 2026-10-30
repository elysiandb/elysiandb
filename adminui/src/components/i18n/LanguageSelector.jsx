import { Form } from "react-bootstrap";
import { LANGUAGES } from "../../i18n/languages.js";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";

export default function LanguageSelector() {
    const { t, language, setLanguage } = useTranslation();

    return (
        <Form.Select
            size="sm"
            className="language-selector"
            aria-label={t("language.label")}
            value={language}
            onChange={(e) => setLanguage(e.target.value)}
        >
            {LANGUAGES.map(({ code, label }) => (
                <option key={code} value={code} lang={code}>
                    {label}
                </option>
            ))}
        </Form.Select>
    );
}
