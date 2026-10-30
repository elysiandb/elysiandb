import {Card, Form, Button, Alert, Row, Col} from "react-bootstrap";
import { useEffect, useState } from "react";
import { useEntityTypes } from "../../hooks/entity/useEntityTypes.js";
import { useEntityTypeActions } from "../../hooks/entity/useEntityTypeActions.js";
import FieldEditor from "./EntityFieldEditor.jsx";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";

export default function EntityType({ entityType, onDropSuccess }) {
    const { loadOne, entityTypeData } = useEntityTypes();
    const { updateEntitySchema, dropEntityType, loading, error } = useEntityTypeActions();
    const [localData, setLocalData] = useState(null);
    const { t } = useTranslation();

    const refresh = () => {
        loadOne(entityType);
    };

    useEffect(() => {
        refresh();
    }, [entityType]);

    useEffect(() => {
        setLocalData(entityTypeData);
    }, [entityTypeData]);

    if (!localData) return null;

    const updateField = (key, updated, rename) => {
        if (!rename) {
            setLocalData({
                ...localData,
                fields: {
                    ...localData.fields,
                    [key]: updated
                }
            });
        } else if (rename === "delete") {
            const cloned = { ...localData.fields };
            delete cloned[key];
            setLocalData({ ...localData, fields: cloned });
        } else {
            const cloned = { ...localData.fields };
            delete cloned[key];
            cloned[updated.name] = updated;
            setLocalData({ ...localData, fields: cloned });
        }
    };

    const onSave = async () => {
        await updateEntitySchema(localData.id, localData.fields);
        refresh();
    };

    const onDrop = async () => {
        if (!confirm(t("schema.confirmDrop", { entity: localData.id }))) return;

        await dropEntityType(localData.id);

        if (onDropSuccess) {
            onDropSuccess();
        }
    };

    return (
        <Card className="p-4">
            <h4 className="text-warning mb-4">
                {localData.id} ({localData._manual ? t("schema.manual") : t("schema.autoManaged")})
            </h4>

            {error && (
                <Alert variant="danger" className="mb-3">{error}</Alert>
            )}

            <Row className="mb-4">
                <Col>
                    <div className="entity-toolbar">
                        <Button
                            variant="outline-light"
                            onClick={() => {
                                const newKey = "newField_" + Math.random().toString(36).slice(2, 6);

                                setLocalData({
                                    ...localData,
                                    fields: {
                                        [newKey]: {
                                            name: newKey,
                                            required: false,
                                            type: "string"
                                        },
                                        ...localData.fields
                                    }
                                });
                            }}
                        >
                            {t("schema.addField")}
                        </Button>
                        <Button
                            variant="warning"
                            disabled={loading === localData.id}
                            onClick={onSave}
                        >
                            {loading === localData.id ? t("common.updating") : t("schema.update")}
                        </Button>
                        <Button
                            variant="danger"
                            disabled={loading === localData.id}
                            onClick={onDrop}
                        >
                            {loading === localData.id ? t("common.updating") : t("schema.drop")}
                        </Button>
                    </div>
                </Col>
            </Row>

            <Form className="entity-tree">
                {Object.keys(localData.fields).map((key) => (
                    <FieldEditor
                        key={key}
                        fieldKey={key}
                        field={localData.fields[key]}
                        onChange={updateField}
                        depth={0}
                    />
                ))}
            </Form>
        </Card>
    );
}
