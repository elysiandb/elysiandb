import { Card } from "react-bootstrap";
import TimeSeriesChart from "./TimeSeriesChart.jsx";

export default function ChartCard({ title, time, series, format, formatTime, height, syncKey, threshold, children }) {
    return (
        <Card className="h-100 monitoring-card">
            <Card.Header>{title}</Card.Header>
            <Card.Body className="monitoring-card-body">
                <TimeSeriesChart
                    time={time}
                    series={series}
                    format={format}
                    formatTime={formatTime}
                    height={height}
                    syncKey={syncKey}
                    threshold={threshold}
                    label={title}
                />
                {children}
            </Card.Body>
        </Card>
    );
}
