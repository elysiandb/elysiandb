package monitoring_test

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/transport/http/monitoring"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T, bufferLines int) *configuration.Config {
	t.Helper()

	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Log.SlowRequestMs = 250
	globals.SetConfig(cfg)

	if err := log.Configure(log.Options{Level: "info", BufferLines: bufferLines}); err != nil {
		t.Fatal(err)
	}
	log.SetOutput(&strings.Builder{}, false)
	metrics.Reset()

	t.Cleanup(func() {
		log.SetOutput(nil, false)
		_ = log.Configure(log.Options{})
		metrics.Reset()
	})

	return cfg
}

func call(h fasthttp.RequestHandler, method, uri, body string, headers map[string]string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(uri)
	req.Header.SetMethod(method)
	for k, v := range headers {
		req.Header.Set(k, v)
	}
	if body != "" {
		req.SetBodyString(body)
	}

	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	h(ctx)

	return ctx
}

func decode(t *testing.T, ctx *fasthttp.RequestCtx, v any) {
	t.Helper()

	if err := json.Unmarshal(ctx.Response.Body(), v); err != nil {
		t.Fatalf("invalid JSON %q: %v", ctx.Response.Body(), err)
	}
}

func TestOverviewController(t *testing.T) {
	setup(t, 10)
	metrics.Tick(time.Now())

	ctx := call(monitoring.OverviewController, "GET", "/monitoring/overview", "", nil)
	if ctx.Response.StatusCode() != 200 || !strings.HasPrefix(string(ctx.Response.Header.ContentType()), "application/json") {
		t.Fatalf("unexpected answer %d %s", ctx.Response.StatusCode(), ctx.Response.Header.ContentType())
	}

	var body map[string]any
	decode(t, ctx, &body)
	for _, key := range []string{"generated_at", "instance", "status", "traffic", "data", "persistence", "runtime", "connections", "logs"} {
		if _, ok := body[key]; !ok {
			t.Fatalf("missing %s in %v", key, body)
		}
	}
}

func TestSeriesController(t *testing.T) {
	setup(t, 10)
	t0 := time.Now()
	metrics.Tick(t0)
	metrics.Tick(t0.Add(time.Second))
	metrics.Tick(t0.Add(2 * time.Second))

	ctx := call(monitoring.SeriesController, "GET", "/monitoring/series", "", nil)
	var body struct {
		Range         string                `json:"range"`
		SlowRequestMs int                   `json:"slow_request_ms"`
		Time          []int64               `json:"time"`
		Series        map[string][]*float64 `json:"series"`
	}
	decode(t, ctx, &body)

	if body.Range != "15m" || body.SlowRequestMs != 250 || len(body.Time) != 2 || len(body.Series["http_rps"]) != 2 {
		t.Fatalf("unexpected series %+v", body)
	}

	since := call(monitoring.SeriesController, "GET", "/monitoring/series?range=15m&since="+itoa(body.Time[0]), "", nil)
	decode(t, since, &body)
	if len(body.Time) != 1 {
		t.Fatalf("since should keep one point, got %d", len(body.Time))
	}

	for _, uri := range []string{"/monitoring/series?range=2h", "/monitoring/series?since=yesterday"} {
		if ctx := call(monitoring.SeriesController, "GET", uri, "", nil); ctx.Response.StatusCode() != 400 {
			t.Fatalf("%s should be refused, got %d", uri, ctx.Response.StatusCode())
		}
	}
}

func TestDetailsController(t *testing.T) {
	setup(t, 10)
	metrics.ObserveHTTP("GET", "/api/{entity}", 200, time.Millisecond, "")

	ctx := call(monitoring.DetailsController, "GET", "/monitoring/details", "", nil)
	var body map[string]any
	decode(t, ctx, &body)

	if _, ok := body["routes"]; !ok {
		t.Fatalf("missing routes in %v", body)
	}
}

func TestLogsControllerFilters(t *testing.T) {
	setup(t, 100)

	log.For("http").Error("request failed", "route", "/api/{entity}", "request_id", "abc")
	log.For("audit").Info("user created", "actor", "alice", "user", "bob")
	log.For("security").Warn("login failed", "user", "eve")

	var page log.Page
	decode(t, call(monitoring.LogsController, "GET", "/monitoring/logs?level=warn", "", nil), &page)
	if len(page.Entries) != 2 || page.Entries[0].Message != "login failed" {
		t.Fatalf("unexpected warn page %+v", page.Entries)
	}

	decode(t, call(monitoring.LogsController, "GET", "/monitoring/logs?component=audit&limit=5", "", nil), &page)
	if len(page.Entries) != 1 || page.Entries[0].Attrs["actor"] != "alice" {
		t.Fatalf("unexpected audit page %+v", page.Entries)
	}

	decode(t, call(monitoring.LogsController, "GET", "/monitoring/logs?request_id=abc", "", nil), &page)
	if len(page.Entries) != 1 {
		t.Fatalf("unexpected request id page %+v", page.Entries)
	}

	decode(t, call(monitoring.LogsController, "GET", "/monitoring/logs?limit=1", "", nil), &page)
	if len(page.Entries) != 1 || !page.HasMore || len(page.Components) != 3 {
		t.Fatalf("unexpected limited page %+v", page)
	}

	latest := page.LatestSeq
	log.Info("newer")
	decode(t, call(monitoring.LogsController, "GET", "/monitoring/logs?after="+itoa(int64(latest)), "", nil), &page)
	if len(page.Entries) != 1 || page.Entries[0].Message != "newer" {
		t.Fatalf("after should return the new line only: %+v", page.Entries)
	}

	for _, uri := range []string{"/monitoring/logs?level=loud", "/monitoring/logs?after=-1", "/monitoring/logs?since=now", "/monitoring/logs?limit=x"} {
		if ctx := call(monitoring.LogsController, "GET", uri, "", nil); ctx.Response.StatusCode() != 400 {
			t.Fatalf("%s should be refused, got %d", uri, ctx.Response.StatusCode())
		}
	}
}

func TestLogsControllerWithoutBuffer(t *testing.T) {
	setup(t, 0)

	for _, h := range []fasthttp.RequestHandler{monitoring.LogsController, monitoring.LogsDownloadController} {
		if ctx := call(h, "GET", "/monitoring/logs", "", nil); ctx.Response.StatusCode() != 404 {
			t.Fatalf("expected 404 without a buffer, got %d", ctx.Response.StatusCode())
		}
	}
}

func TestLogsDownload(t *testing.T) {
	setup(t, 2000)

	for i := 0; i < 1500; i++ {
		log.Info("line", "i", i)
	}

	ctx := call(monitoring.LogsDownloadController, "GET", "/monitoring/logs/download", "", nil)
	if !strings.Contains(string(ctx.Response.Header.Peek("Content-Disposition")), "attachment") {
		t.Fatalf("missing attachment header")
	}

	lines := strings.Split(strings.TrimSpace(string(ctx.Response.Body())), "\n")
	if len(lines) != 1500 {
		t.Fatalf("the download must hold every matching line, got %d", len(lines))
	}

	var first log.Entry
	if err := json.Unmarshal([]byte(lines[0]), &first); err != nil || first.Attrs["i"] != float64(1499) {
		t.Fatalf("unexpected first line %q %v", lines[0], err)
	}
}

func TestLogLevelControllers(t *testing.T) {
	setup(t, 100)

	var info log.LevelInfo
	decode(t, call(monitoring.GetLogLevelController, "GET", "/monitoring/logs/level", "", nil), &info)
	if info.Level != "info" || info.Configured != "info" || info.Until != nil {
		t.Fatalf("unexpected level %+v", info)
	}

	decode(t, call(monitoring.PutLogLevelController, "PUT", "/monitoring/logs/level", `{"level":"debug","minutes":15}`, nil), &info)
	if info.Level != "debug" || info.Until == nil || time.Until(*info.Until) < 14*time.Minute {
		t.Fatalf("unexpected level %+v", info)
	}

	audit := log.Logs(log.Query{Component: "audit"}).Entries
	if len(audit) != 1 || audit[0].Message != "log level changed" || audit[0].Attrs["level"] != "debug" {
		t.Fatalf("the change must be audited: %+v", audit)
	}

	decode(t, call(monitoring.PutLogLevelController, "PUT", "/monitoring/logs/level", `{"level":"info"}`, nil), &info)
	if info.Level != "info" || info.Until != nil {
		t.Fatalf("the configured level resets the override: %+v", info)
	}

	for _, body := range []string{`{"level":"debug"}`, `{"level":"debug","minutes":2000}`, `{"level":"loud","minutes":5}`, `{"minutes":5}`, `nope`} {
		if ctx := call(monitoring.PutLogLevelController, "PUT", "/monitoring/logs/level", body, nil); ctx.Response.StatusCode() != 400 {
			t.Fatalf("%s should be refused, got %d", body, ctx.Response.StatusCode())
		}
	}
}

func TestPrometheusTokenAuth(t *testing.T) {
	cfg := setup(t, 10)
	cfg.Monitoring.Prometheus.Token = "s3cret"

	adminCalled := false
	adminAuth := func(next fasthttp.RequestHandler) fasthttp.RequestHandler {
		return func(ctx *fasthttp.RequestCtx) {
			adminCalled = true
			next(ctx)
		}
	}
	h := monitoring.PrometheusAuth(adminAuth, monitoring.PrometheusController)

	if ctx := call(h, "GET", "/metrics", "", nil); ctx.Response.StatusCode() != 401 {
		t.Fatalf("a missing token must be refused, got %d", ctx.Response.StatusCode())
	}
	if ctx := call(h, "GET", "/metrics", "", map[string]string{"Authorization": "Bearer wrong"}); ctx.Response.StatusCode() != 401 {
		t.Fatalf("a wrong token must be refused, got %d", ctx.Response.StatusCode())
	}

	ctx := call(h, "GET", "/metrics", "", map[string]string{"Authorization": "Bearer s3cret"})
	if ctx.Response.StatusCode() != 200 || !strings.Contains(string(ctx.Response.Body()), "elysiandb_uptime_seconds") {
		t.Fatalf("unexpected answer %d %s", ctx.Response.StatusCode(), ctx.Response.Body())
	}
	if adminCalled {
		t.Fatalf("with a token the admin credentials are not used")
	}

	cfg.Monitoring.Prometheus.Token = ""
	call(h, "GET", "/metrics", "", nil)
	if !adminCalled {
		t.Fatalf("without a token the admin credentials are required")
	}
}

func itoa(v int64) string {
	b, _ := json.Marshal(v)

	return string(b)
}
