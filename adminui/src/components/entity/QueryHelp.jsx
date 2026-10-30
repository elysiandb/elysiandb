import { useTranslation } from "../../hooks/i18n/useTranslation.js";

const QUERY_HELP = [
    {
        id: "filters",
        code: {
            filters: {
                and: [
                    { status: { eq: "published" } },
                    {
                        or: [
                            { title: { eq: "*Go*" } },
                            { excerpt: { eq: "*distributed*" } }
                        ]
                    }
                ]
            }
        }
    },
    {
        id: "sort",
        code: {
            sorts: {
                publishedAt: "desc"
            }
        }
    },
    {
        id: "pagination",
        code: {
            offset: 0,
            limit: 50
        }
    }
];

export default function QueryHelp() {
    const { t } = useTranslation();

    return (
        <div className="query-help">
            {QUERY_HELP.map(section => (
                <div key={section.id} className="query-help-section">
                    <div className="query-help-title">{t(`browse.help.${section.id}`)}</div>
                    <pre className="query-help-code">
                        {JSON.stringify(section.code, null, 2)}
                    </pre>
                </div>
            ))}
        </div>
    );
}
