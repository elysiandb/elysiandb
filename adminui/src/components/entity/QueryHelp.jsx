const QUERY_HELP = [
    {
        title: "Filters",
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
        title: "Sort",
        code: {
            sorts: {
                publishedAt: "desc"
            }
        }
    },
    {
        title: "Pagination",
        code: {
            offset: 0,
            limit: 50
        }
    }
];

export default function QueryHelp() {
    return (
        <div className="query-help">
            {QUERY_HELP.map(section => (
                <div key={section.title} className="query-help-section">
                    <div className="query-help-title">{section.title}</div>
                    <pre className="query-help-code">
                        {JSON.stringify(section.code, null, 2)}
                    </pre>
                </div>
            ))}
        </div>
    );
}
