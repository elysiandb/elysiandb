import { useCallback, useEffect, useState } from "react";
import { ElysianConfiguration } from "../configuration/ElysianConfiguration";
import { apiFetch } from "../utils/api";
import { Card, ListGroup, Spinner, Alert, Container, Row, Col } from "react-bootstrap";

export default function Configuration() {
    const [data, setData] = useState<ElysianConfiguration | null>(null);
    const [loading, setLoading] = useState(true);
    const [errors, setErrors] = useState<Error | null>(null);

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
        return <Alert variant="danger">Error: {errors.message}</Alert>;
    }

    return (
        <Container className="py-4">
            {data && (
                <Row xs={1} md={2} className="g-4">

                    <Col>
                        <Section title="Store">
                            <ListGroup.Item>Folder: {data.Store.Folder}</ListGroup.Item>
                            <ListGroup.Item>Shards: {data.Store.Shards}</ListGroup.Item>
                            <ListGroup.Item>Flush Interval: {data.Store.FlushIntervalSeconds}s</ListGroup.Item>
                            <ListGroup.Item>Crash Recovery: {String(data.Store.CrashRecovery.Enabled)}</ListGroup.Item>
                            <ListGroup.Item>MaxLogMB: {data.Store.CrashRecovery.MaxLogMB}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title="Server">
                            <ListGroup.Item>HTTP: {data.Server.HTTP.Enabled ? "Enabled" : "Disabled"}</ListGroup.Item>
                            <ListGroup.Item>Host: {data.Server.HTTP.Host}</ListGroup.Item>
                            <ListGroup.Item>Port: {data.Server.HTTP.Port}</ListGroup.Item>
                            <ListGroup.Item>TCP: {data.Server.TCP.Enabled ? "Enabled" : "Disabled"}</ListGroup.Item>
                            <ListGroup.Item>Host: {data.Server.TCP.Host}</ListGroup.Item>
                            <ListGroup.Item>Port: {data.Server.TCP.Port}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title="Logging">
                            <ListGroup.Item>Flush Interval: {data.Log.FlushIntervalSeconds}s</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title="Security">
                            <ListGroup.Item>Authentication Enabled: {String(data.Security.Authentication.Enabled)}</ListGroup.Item>
                            <ListGroup.Item>Mode: {data.Security.Authentication.Mode}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title="Stats">
                            <ListGroup.Item>Enabled: {String(data.Stats.Enabled)}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title="API">
                            <ListGroup.Item>Index Workers: {data.Api.Index.Workers}</ListGroup.Item>
                            <ListGroup.Item>Cache: {data.Api.Cache.Enabled ? "Enabled" : "Disabled"}</ListGroup.Item>
                            <ListGroup.Item>Cleanup Interval: {data.Api.Cache.CleanupIntervalSeconds}s</ListGroup.Item>
                            <ListGroup.Item>Schema: {data.Api.Schema.Enabled ? "Enabled" : "Disabled"}</ListGroup.Item>
                            <ListGroup.Item>Strict: {String(data.Api.Schema.Strict)}</ListGroup.Item>
                        </Section>
                    </Col>

                    <Col>
                        <Section title="Admin UI">
                            <ListGroup.Item>Enabled: {String(data.AdminUI.Enabled)}</ListGroup.Item>
                        </Section>
                    </Col>

                </Row>
            )}
        </Container>
    );
}
