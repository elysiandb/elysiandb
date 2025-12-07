// EntityFieldEditor.jsx
import {Button, Form} from "react-bootstrap";
import FieldEditor from "./EntityFieldEditor.jsx";

export default function EntityFieldEditor({ fieldKey, field, onChange, depth }) {
    const update = (patch) => {
        onChange(fieldKey, { ...field, ...patch });
    };

    const addChild = () => {
        const newKey = "newField_" + Math.random().toString(36).slice(2, 6);
        const newField = {
            name: newKey,
            required: false,
            type: "string"
        };

        update({
            fields: {
                [newKey]: newField,
                ...(field.fields || {})
            }
        });
    };

    const padding = depth * 18;

    return (
        <div className="entity-field" style={{ marginLeft: padding }}>
            <div className="entity-field-header">
                <span className="entity-field-branch" />
                <span className="entity-field-key">{fieldKey}</span>
            </div>

            <div className="entity-field-row">
                <button
                    type="button"
                    className="entity-delete-field"
                    onClick={() => onChange(fieldKey, null, "delete")}
                >
                    –
                </button>
                <Form.Control
                    className="entity-field-input"
                    type="text"
                    value={field.name}
                    onChange={(e) => update({ name: e.target.value })}
                />

                <Form.Select
                    className="entity-field-type"
                    value={field.type}
                    onChange={(e) => update({ type: e.target.value })}
                >
                    <option value="string">string</option>
                    <option value="number">number</option>
                    <option value="boolean">boolean</option>
                    <option value="object">object</option>
                    <option value="array">array</option>
                </Form.Select>

                <Form.Check
                    className="entity-field-required"
                    type="checkbox"
                    checked={field.required}
                    onChange={(e) => update({ required: e.target.checked })}
                />

                <button
                    type="button"
                    className="entity-add-child"
                    onClick={addChild}
                >
                    +
                </button>
            </div>

            {field.fields && (
                <div className="entity-field-children mt-3">
                    {Object.keys(field.fields).map((sub) => (
                        <FieldEditor
                            key={sub}
                            fieldKey={sub}
                            field={field.fields[sub]}
                            onChange={(key, updated, action) =>
                                update({
                                    fields:
                                        action === "delete"
                                            ? (() => {
                                                const f = { ...field.fields };
                                                delete f[key];
                                                return f;
                                            })()
                                            : action === "rename"
                                                ? (() => {
                                                    const f = { ...field.fields };
                                                    delete f[key];
                                                    f[updated.name] = updated;
                                                    return f;
                                                })()
                                                : { ...field.fields, [key]: updated }
                                })
                            }
                            depth={depth + 1}
                        />
                    ))}
                </div>
            )}
        </div>
    );
}
