import { Card, Button, Form, Spinner, Alert } from "react-bootstrap";
import { useEffect, useMemo, useState } from "react";
import { Editor } from "@monaco-editor/react";
import { useTranslation } from "../../hooks/i18n/useTranslation.js";

export default function HookEditor({ hook, loading, onSave }) {
    const [optionsOpen, setOptionsOpen] = useState(false);
    const [form, setForm] = useState(null);
    const { t } = useTranslation();

    useEffect(() => {
        if (!hook) return;
        setForm({
            id: hook.id,
            entity: hook.entity,
            name: hook.name ?? "",
            event: hook.event ?? "post_read",
            priority: hook.priority ?? 1,
            language: hook.language ?? "javascript",
            script: hook.script ?? "",
            bypass_acl: hook.bypass_acl ?? false,
            enabled: hook.enabled ?? true,
        });
    }, [hook]);

    const error = useMemo(() => {
        if (!form?.script?.trim()) return null;
        if (form.event === "post_read" && !form.script.trim().startsWith("function postRead")) {
            return t("hooks.mustStartWith", { event: "post_read", signature: "function postRead" });
        }
        if (form.event === "pre_read" && !form.script.trim().startsWith("function preRead")) {
            return t("hooks.mustStartWith", { event: "pre_read", signature: "function preRead" });
        }
        return null;
    }, [form, t]);

    if (loading || !form) {
        return (
            <Card className="entity-sidebar">
                <div className="py-4 text-center">
                    <Spinner animation="border" />
                </div>
            </Card>
        );
    }

    const update = (key, value) => {
        setForm(prev => ({ ...prev, [key]: value }));
    };

    const save = () => {
        if (error) return;
        onSave(form);
    };

    return (
        <Card className="entity-sidebar">
            <Card.Header className="d-flex justify-content-between align-items-center">
                <span className="text-warning fw-bold">{t("hooks.title")}</span>
                <Button variant="primary" onClick={save} disabled={!!error}>
                    {t("common.save")}
                </Button>
            </Card.Header>

            <Card.Body>
                <Form>
                    <div className="d-flex justify-content-between align-items-center mb-2">
                        <span className="fw-semibold">{t("hooks.options")}</span>
                        <Button
                            variant="outline-secondary"
                            size="sm"
                            onClick={() => setOptionsOpen(o => !o)}
                        >
                            {optionsOpen ? t("common.hide") : t("common.show")}
                        </Button>
                    </div>

                    {optionsOpen && (
                        <>
                            <Form.Group className="mb-3">
                                <Form.Check
                                    type="switch"
                                    id="hook-enabled"
                                    label={t("common.enabled")}
                                    checked={form.enabled}
                                    onChange={e => update("enabled", e.target.checked)}
                                />
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Check
                                    type="switch"
                                    id="hook-bypass-acl"
                                    label={t("hooks.bypassAcl")}
                                    checked={form.bypass_acl}
                                    onChange={e => update("bypass_acl", e.target.checked)}
                                />
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>{t("hooks.language")}</Form.Label>
                                <Form.Select
                                    value={form.language}
                                    onChange={e => update("language", e.target.value)}
                                >
                                    <option value="javascript">javascript</option>
                                </Form.Select>
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>{t("common.event")}</Form.Label>
                                <Form.Select
                                    value={form.event}
                                    onChange={e => update("event", e.target.value)}
                                >
                                    <option value="post_read">post_read</option>
                                    <option value="pre_read">pre_read</option>
                                </Form.Select>
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>{t("hooks.priority")}</Form.Label>
                                <Form.Control
                                    type="number"
                                    min={1}
                                    max={100}
                                    value={form.priority}
                                    onChange={e => update("priority", Number(e.target.value))}
                                />
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>{t("common.name")}</Form.Label>
                                <Form.Control
                                    type="text"
                                    value={form.name}
                                    onChange={e => update("name", e.target.value)}
                                />
                            </Form.Group>
                        </>
                    )}

                    {error && (
                        <Alert variant="danger" className="mt-3">
                            {error}
                        </Alert>
                    )}

                    <Form.Group className="mt-3">
                        <Form.Label>{t("hooks.script")}</Form.Label>
                        <div style={{ height: 260 }}>
                            <Editor
                                language="javascript"
                                value={form.script}
                                onChange={v => update("script", v ?? "")}
                                theme="elysian"
                                beforeMount={monaco => {
                                    monaco.editor.defineTheme("elysian", {
                                        base: "vs-dark",
                                        inherit: true,
                                        rules: [],
                                        colors: {
                                            "editor.background": "#151829",
                                            "editor.foreground": "#e6e6f0",
                                            "editorCursor.foreground": "#f7d64a",
                                            "editor.lineHighlightBackground": "#1b1f3a",
                                            "editor.selectionBackground": "#f7d64a33",
                                        },
                                    });
                                }}
                                options={{
                                    minimap: { enabled: false },
                                    fontSize: 13,
                                    lineNumbers: "on",
                                    scrollBeyondLastLine: false,
                                    wordWrap: "on",
                                    automaticLayout: true,
                                    tabSize: 2,
                                }}
                            />
                        </div>
                    </Form.Group>
                </Form>
            </Card.Body>
        </Card>
    );
}
