import { Card } from "react-bootstrap";
import TimeSeriesChart from "./TimeSeriesChart.jsx";

export default function KpiCard({ title, value, detail, time, values, format, formatTime, color, tone }) {
    return (
        <Card className="h-100 monitoring-kpi" data-tone={tone ?? "neutral"}>
            <Card.Body>
                <div className="monitoring-kpi-title">{title}</div>
                <div className="monitoring-kpi-value">{value}</div>
                {detail && <div className="monitoring-kpi-detail">{detail}</div>}
                {values && (
                    <TimeSeriesChart
                        time={time}
                        series={[{ label: title, values, color }]}
                        format={format}
                        formatTime={formatTime}
                        height={40}
                        sparkline
                        label={title}
                    />
                )}
            </Card.Body>
        </Card>
    );
}
