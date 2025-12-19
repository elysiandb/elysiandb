export default function JsonView({ data }) {
    if (data === null) {
        return <span className="json-null">null</span>;
    }

    if (typeof data !== "object") {
        return <span className="json-value">{String(data)}</span>;
    }

    return (
        <div className="json-object">
            {Object.entries(data).map(([key, value]) => (
                <div key={key} className="json-entry">
                    <div className="json-keyline">
                        <span className="json-key">{key}</span>

                        {typeof value !== "object" || value === null ? (
                            <>
                                <span className="json-sep">:</span>
                                <JsonView data={value} />
                            </>
                        ) : null}
                    </div>

                    {typeof value === "object" && value !== null ? (
                        <div className="json-children">
                            <JsonView data={value} />
                        </div>
                    ) : null}
                </div>
            ))}
        </div>
    );
}
