import { Card, Button, Form, Spinner } from "react-bootstrap";
import { useEffect, useState } from "react";
import { Editor } from "@monaco-editor/react";

export default function HookEditor({ hook, loading, onSave }) {
    const [optionsOpen, setOptionsOpen] = useState(false);
    const [form, setForm] = useState(null);

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
        onSave(form);
    };

    return (
        <Card className="entity-sidebar">
            <Card.Header className="d-flex justify-content-between align-items-center">
                <span className="text-warning fw-bold">Hook</span>
                <Button variant="primary" onClick={save}>
                    Save
                </Button>
            </Card.Header>

            <Card.Body>
                <Form>
                    <div className="d-flex justify-content-between align-items-center mb-2">
                        <span className="fw-semibold">Options</span>
                        <Button
                            variant="outline-secondary"
                            size="sm"
                            onClick={() => setOptionsOpen(o => !o)}
                        >
                            {optionsOpen ? "Hide" : "Show"}
                        </Button>
                    </div>

                    {optionsOpen && (
                        <>
                            <Form.Group className="mb-3">
                                <Form.Check
                                    type="switch"
                                    id="hook-enabled"
                                    label="Enabled"
                                    checked={form.enabled}
                                    onChange={e => update("enabled", e.target.checked)}
                                />
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Check
                                    type="switch"
                                    id="hook-bypass-acl"
                                    label="Bypass ACL"
                                    checked={form.bypass_acl}
                                    onChange={e => update("bypass_acl", e.target.checked)}
                                />
                                <Form.Text style={{ color: "#9aa0c7" }}>
                                    Allows this hook to bypass ACL checks
                                </Form.Text>
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>Language</Form.Label>
                                <Form.Select
                                    value={form.language}
                                    onChange={e => update("language", e.target.value)}
                                >
                                    <option value="javascript">javascript</option>
                                </Form.Select>
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>Event</Form.Label>
                                <Form.Select
                                    value={form.event}
                                    onChange={e => update("event", e.target.value)}
                                >
                                    <option value="post_read">post_read</option>
                                </Form.Select>
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>Priority</Form.Label>
                                <Form.Control
                                    type="number"
                                    min={1}
                                    max={100}
                                    value={form.priority}
                                    onChange={e => update("priority", Number(e.target.value))}
                                />
                            </Form.Group>

                            <Form.Group className="mb-3">
                                <Form.Label>Name</Form.Label>
                                <Form.Control
                                    type="text"
                                    value={form.name}
                                    onChange={e => update("name", e.target.value)}
                                />
                            </Form.Group>
                        </>
                    )}

                    <Form.Group className="mt-3">
                        <Form.Label>Script</Form.Label>
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
