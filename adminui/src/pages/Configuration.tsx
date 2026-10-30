import { useCallback, useEffect, useState } from "react";
import { ElysianConfiguration } from "../configuration/ElysianConfiguration";
import { apiFetch } from "../utils/api";
import { Card, ListGroup, Spinner, Alert, Container, Row, Col } from "react-bootstrap";
import { useTranslation } from "../hooks/i18n/useTranslation.js";

export default function Configuration() {
    const [data, setData] = useState<ElysianConfiguration | null>(null);
    const [loading, setLoading] = useState(true);
    const [errors, setErrors] = useState<Error | null>(null);
    const { t } = useTranslation();

    const entry = (field: string, value: unknown) =>
        t("configuration.entry", { label: t(`configuration.fields.${field}`), value });

    const status = (enabled: boolean) => t(enabled ? "common.enabled" : "common.disabled");

    const fetchData = useCallback(() => {
        apiFetch<ElysianConfiguration>("/config")
            .then((response) => {
                setData(response);
                setLoading(false);
            })
            .catch((error) => {
                setErrors(error);
                setLoading(false);
            });
    }, []);

    useEffect(() => {
        fetchData();
    }, [fetchData]);

    const Section = ({ title, children }: any) => (
        <Card className="h-100">
            <Card.Header>{title}</Card.Header>
            <ListGroup variant="flush">{children}</ListGroup>
        </Card>
    );

    if (loading) {
        return (
            <Container className="py-5 text-center">
                <Spinner animation="border" />
            </Container>
        );
    }

    if (errors) {
        return <Alert variant="danger">{t("common.error", { message: errors.message })}</Alert>;
    }

    return (
        <Container className="py-4">
            {data && (
                <Row xs={1} md={2} className="g-4">

                    <Col>
                        <Section title={t("configuration.sections.store")}>
                            <ListGroup.Item>{entry("folder", data.Store.Folder)}</ListGroup.Item>
                            <ListGroup.Item>{entry("shards", data.Store.Shards)}</ListGroup.Item>
                            <ListGroup.Item>{entry("flushInterval", `${data.Store.FlushIntervalSeconds}s`)}</ListGroup.Item>
                            <ListGroup.Item>{entry("crashRecovery", String(data.Store.CrashRecovery.Enabled))}</ListGroup.Item>
                            <ListGroup.Item>{entry("maxLogMB", data.Store.CrashRecovery.MaxLogMB)}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title={t("configuration.sections.server")}>
                            <ListGroup.Item>{entry("http", status(data.Server.HTTP.Enabled))}</ListGroup.Item>
                            <ListGroup.Item>{entry("host", data.Server.HTTP.Host)}</ListGroup.Item>
                            <ListGroup.Item>{entry("port", data.Server.HTTP.Port)}</ListGroup.Item>
                            <ListGroup.Item>{entry("tcp", status(data.Server.TCP.Enabled))}</ListGroup.Item>
                            <ListGroup.Item>{entry("host", data.Server.TCP.Host)}</ListGroup.Item>
                            <ListGroup.Item>{entry("port", data.Server.TCP.Port)}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title={t("configuration.sections.logging")}>
                            <ListGroup.Item>{entry("flushInterval", `${data.Log.FlushIntervalSeconds}s`)}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title={t("configuration.sections.security")}>
                            <ListGroup.Item>{entry("authenticationEnabled", String(data.Security.Authentication.Enabled))}</ListGroup.Item>
                            <ListGroup.Item>{entry("mode", data.Security.Authentication.Mode)}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title={t("configuration.sections.stats")}>
                            <ListGroup.Item>{entry("enabled", String(data.Stats.Enabled))}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title={t("configuration.sections.api")}>
                            <ListGroup.Item>{entry("indexWorkers", data.Api.Index.Workers)}</ListGroup.Item>
                            <ListGroup.Item>{entry("cache", status(data.Api.Cache.Enabled))}</ListGroup.Item>
                            <ListGroup.Item>{entry("cleanupInterval", `${data.Api.Cache.CleanupIntervalSeconds}s`)}</ListGroup.Item>
                            <ListGroup.Item>{entry("schema", status(data.Api.Schema.Enabled))}</ListGroup.Item>
                            <ListGroup.Item>{entry("strict", String(data.Api.Schema.Strict))}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title={t("configuration.sections.adminUI")}>
                            <ListGroup.Item>{entry("enabled", String(data.AdminUI.Enabled))}</ListGroup.Item>
                        </Section>
                    </Col>

                </Row>
            )}
        </Container>
    );
}
