package metrics_test

import (
	"encoding/json"
	"errors"
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
)

func setup(t *testing.T, sources metrics.Sources) *configuration.Config {
	t.Helper()

	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Server.HTTP = configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: 8089}
	cfg.Server.TCP = configuration.ServerConfig{Enabled: true, Host: "127.0.0.1", Port: 8088}
	cfg.Security.Authentication = configuration.AuthenticationConfig{Enabled: true, Mode: "user"}
	cfg.Log.SlowRequestMs = 500
	globals.SetConfig(cfg)

	metrics.Reset()
	metrics.SetSources(sources)
	t.Cleanup(metrics.Reset)

	return cfg
}

type seriesJSON struct {
	Range         string                `json:"range"`
	StepSeconds   int                   `json:"step_seconds"`
	SlowRequestMs int                   `json:"slow_request_ms"`
	Time          []int64               `json:"time"`
	Series        map[string][]*float64 `json:"series"`
}

func series(t *testing.T, rangeName string, since int64) seriesJSON {
	t.Helper()

	s, ok := metrics.Series(rangeName, since)
	if !ok {
		t.Fatalf("unknown range %q", rangeName)
	}

	data, err := json.Marshal(s)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}

	var out seriesJSON
	if err := json.Unmarshal(data, &out); err != nil {
		t.Fatalf("unmarshal %s: %v", data, err)
	}

	return out
}

func last(t *testing.T, s seriesJSON, name string) *float64 {
	t.Helper()

	values, ok := s.Series[name]
	if !ok {
		t.Fatalf("missing series %q", name)
	}
	if len(values) == 0 {
		t.Fatalf("series %q is empty", name)
	}

	return values[len(values)-1]
}

func problemCodes(status metrics.Status) []string {
	codes := []string{}
	for _, p := range status.Problems {
		codes = append(codes, p.Code)
	}

	return codes
}

func hasProblem(status metrics.Status, code string) (metrics.Problem, bool) {
	for _, p := range status.Problems {
		if p.Code == code {
			return p, true
		}
	}

	return metrics.Problem{}, false
}

func TestRatesAndPercentilesPerSecond(t *testing.T) {
	setup(t, metrics.Sources{})
	t0 := time.Now()

	metrics.Tick(t0)
	for i := 0; i < 10; i++ {
		metrics.ObserveHTTP("GET", "/api/{entity}", 200, 800*time.Microsecond, metrics.KindDocRead)
	}
	metrics.ObserveHTTP("POST", "/api/{entity}", 500, 2*time.Millisecond, metrics.KindDocWrite)
	metrics.ObserveHTTP("GET", "/kv/{key}", 404, time.Millisecond, metrics.KindKVGet)
	for i := 0; i < 4; i++ {
		metrics.ObserveTCP(metrics.TCPGet, 100*time.Microsecond)
	}
	metrics.ObserveTCP(metrics.TCPOther, 100*time.Microsecond)
	metrics.Tick(t0.Add(time.Second))

	s := series(t, "15m", 0)
	if s.StepSeconds != 1 || len(s.Time) != 1 {
		t.Fatalf("unexpected series %+v", s)
	}

	checks := map[string]float64{
		"http_rps":       12,
		"http_2xx_rps":   10,
		"http_4xx_rps":   1,
		"http_5xx_rps":   1,
		"tcp_rps":        5,
		"tcp_get_rps":    4,
		"tcp_other_rps":  1,
		"kv_gets_rps":    5,
		"doc_reads_rps":  10,
		"doc_writes_rps": 1,
	}
	for name, want := range checks {
		if got := last(t, s, name); got == nil || *got != want {
			t.Fatalf("%s = %v, want %v", name, got, want)
		}
	}

	p50 := last(t, s, "http_p50_ms")
	if p50 == nil || *p50 <= 0.5 || *p50 > 1 {
		t.Fatalf("p50 should fall in the 0.5-1 ms bucket, got %v", p50)
	}

	if p := last(t, s, "hook_p95_ms"); p != nil {
		t.Fatalf("a percentile without samples must be null, got %v", *p)
	}
}

func TestSeriesSinceAndResolutions(t *testing.T) {
	setup(t, metrics.Sources{})
	t0 := time.Now().Add(-20 * time.Second)

	for i := 0; i <= 12; i++ {
		metrics.Tick(t0.Add(time.Duration(i) * time.Second))
	}

	all := series(t, "15m", 0)
	if len(all.Time) != 12 {
		t.Fatalf("expected 12 one-second points, got %d", len(all.Time))
	}

	since := series(t, "15m", all.Time[9])
	if len(since.Time) != 2 || since.Time[0] != all.Time[10] {
		t.Fatalf("since should return the newer points only: %v", since.Time)
	}

	hour := series(t, "1h", 0)
	if hour.StepSeconds != 10 || len(hour.Time) != 1 {
		t.Fatalf("expected one 10-second point, got %+v", hour)
	}

	if day := series(t, "24h", 0); day.StepSeconds != 60 || len(day.Time) != 0 {
		t.Fatalf("expected no minute point yet, got %+v", day)
	}

	if _, ok := metrics.Series("2h", 0); ok {
		t.Fatalf("unknown ranges must be refused")
	}

	if len(all.Series) != len(metrics.SeriesNames()) {
		t.Fatalf("every series must be returned")
	}
}

func TestGaugesComeFromSources(t *testing.T) {
	setup(t, metrics.Sources{
		KVKeys:          func() (uint64, uint64) { return 42, 7 },
		EntityCounts:    func() map[string]int { return map[string]int{"books": 3, "authors": 2} },
		IndexBacklog:    func() int { return 5 },
		CacheEntries:    func() int { return 9 },
		RecoverySizes:   func() (int64, int64) { return 100, 200 },
		AdminSessions:   func() int { return 2 },
		Transactions:    func() (int, time.Duration) { return 1, 30 * time.Second },
		HTTPConnections: func() int { return 4 },
	})
	t0 := time.Now()

	metrics.TCPConnectionOpened()
	metrics.TCPConnectionOpened()
	metrics.TCPConnectionClosed()

	metrics.Tick(t0)
	metrics.Tick(t0.Add(time.Second))

	s := series(t, "15m", 0)
	checks := map[string]float64{
		"kv_keys":                    42,
		"kv_keys_ttl":                7,
		"entities":                   5,
		"index_backlog":              5,
		"cache_entries":              9,
		"recovery_json_bytes":        100,
		"recovery_store_bytes":       200,
		"admin_sessions":             2,
		"open_transactions":          1,
		"oldest_transaction_seconds": 30,
		"http_conns":                 4,
		"tcp_conns":                  1,
	}
	for name, want := range checks {
		if got := last(t, s, name); got == nil || *got != want {
			t.Fatalf("%s = %v, want %v", name, got, want)
		}
	}

	if got := last(t, s, "heap_bytes"); got == nil || *got <= 0 {
		t.Fatalf("heap_bytes should come from the Go runtime, got %v", got)
	}
}

func TestFlushStatusAndProblem(t *testing.T) {
	setup(t, metrics.Sources{})
	now := time.Now()

	metrics.RecordFlush("store", 3*time.Millisecond, true, nil)
	metrics.RecordFlush("json", time.Millisecond, true, errors.New("disk full"))
	metrics.Tick(now)

	p := metrics.Persistence()
	if p.LastFlushOK || p.LastError != "disk full" || p.FlushFailures != 1 || p.LastFlushAt == nil {
		t.Fatalf("unexpected persistence %+v", p)
	}

	status := metrics.CurrentStatus()
	if status.Level != metrics.StatusCritical {
		t.Fatalf("a failing flush is critical: %+v", status)
	}
	if _, ok := hasProblem(status, "flush_failing"); !ok {
		t.Fatalf("missing flush_failing in %v", problemCodes(status))
	}

	metrics.RecordFlush("store", time.Millisecond, false, nil)
	metrics.Tick(now.Add(time.Second))
	if _, ok := hasProblem(metrics.CurrentStatus(), "flush_failing"); !ok {
		t.Fatalf("the json target still fails")
	}

	metrics.RecordFlush("json", time.Millisecond, true, nil)
	metrics.Tick(now.Add(2 * time.Second))

	if p := metrics.Persistence(); !p.LastFlushOK || p.LastError != "" || p.LastSuccessAt == nil {
		t.Fatalf("persistence should be ok again: %+v", p)
	}
	if _, ok := hasProblem(metrics.CurrentStatus(), "flush_failing"); ok {
		t.Fatalf("flush_failing should be cleared")
	}
}

func TestErrorRateAndLatencyProblems(t *testing.T) {
	cfg := setup(t, metrics.Sources{})
	cfg.Log.SlowRequestMs = 10
	t0 := time.Now()

	metrics.Tick(t0)
	for i := 0; i < 30; i++ {
		status := 200
		if i < 3 {
			status = 500
		}
		metrics.ObserveHTTP("GET", "/api/{entity}", status, 50*time.Millisecond, "")
	}
	metrics.Tick(t0.Add(time.Second))

	status := metrics.CurrentStatus()
	rate, ok := hasProblem(status, "error_rate_high")
	if !ok || rate.Details["error_rate"] != 0.1 {
		t.Fatalf("expected error_rate_high at 0.1, got %+v", status)
	}

	latency, ok := hasProblem(status, "latency_high")
	if !ok || latency.Details["p95_ms"].(float64) <= 10 {
		t.Fatalf("expected latency_high, got %+v", status)
	}
}

func TestFewRequestsDoNotRaiseErrorRate(t *testing.T) {
	setup(t, metrics.Sources{})
	t0 := time.Now()

	metrics.Tick(t0)
	metrics.ObserveHTTP("GET", "/api/{entity}", 500, time.Millisecond, "")
	metrics.Tick(t0.Add(time.Second))

	if _, ok := hasProblem(metrics.CurrentStatus(), "error_rate_high"); ok {
		t.Fatalf("one failed request out of one is not enough to raise the problem")
	}
}

func TestSecurityAndConfigurationProblems(t *testing.T) {
	cfg := setup(t, metrics.Sources{DefaultAdminPassword: func() bool { return true }})
	cfg.Security.Authentication.Enabled = false
	cfg.Server.HTTP.Host = "0.0.0.0"
	now := time.Now()

	for i := 0; i < 21; i++ {
		metrics.FailedLogin("10.0.0.7")
	}
	metrics.Tick(now)

	status := metrics.CurrentStatus()
	for _, code := range []string{"default_admin_password", "auth_disabled_public", "auth_failures"} {
		if _, ok := hasProblem(status, code); !ok {
			t.Fatalf("missing %s in %v", code, problemCodes(status))
		}
	}

	if p, _ := hasProblem(status, "auth_failures"); p.Details["remote_addr"] != "10.0.0.7" {
		t.Fatalf("auth_failures should name the address: %+v", p)
	}

	if status.Level != metrics.StatusWarning {
		t.Fatalf("expected warning, got %s", status.Level)
	}
}

func TestHookRecoveryReplayTransactionAndBacklogProblems(t *testing.T) {
	backlog := 3
	setup(t, metrics.Sources{
		IndexBacklog: func() int { return backlog },
		Transactions: func() (int, time.Duration) { return 1, 11 * time.Minute },
	})
	now := time.Now()

	metrics.ObserveHook("h1", "books", "post_read", time.Millisecond, errors.New("ReferenceError"))
	metrics.RecoveryWriteFailed(errors.New("no space left"))
	metrics.RecoveryReplayed(12)

	metrics.Tick(now)
	metrics.Tick(now.Add(61 * time.Second))

	status := metrics.CurrentStatus()
	for _, code := range []string{"hook_failing", "recovery_write_failing", "unclean_shutdown", "old_transaction", "index_backlog"} {
		if _, ok := hasProblem(status, code); !ok {
			t.Fatalf("missing %s in %v", code, problemCodes(status))
		}
	}

	if p, _ := hasProblem(status, "hook_failing"); p.Details["hook_id"] != "h1" {
		t.Fatalf("hook_failing should name the hook: %+v", p)
	}
	if p, _ := hasProblem(status, "unclean_shutdown"); p.Severity != metrics.SeverityInfo || p.Details["entries"] != int64(12) {
		t.Fatalf("unexpected unclean_shutdown %+v", p)
	}
	if status.Problems[0].Severity != metrics.SeverityCritical {
		t.Fatalf("critical problems come first: %+v", status.Problems)
	}

	backlog = 0
	metrics.Tick(now.Add(62 * time.Second))
	if _, ok := hasProblem(metrics.CurrentStatus(), "index_backlog"); ok {
		t.Fatalf("index_backlog should clear when the backlog is empty")
	}
}

func TestProblemKeepsItsStartTime(t *testing.T) {
	setup(t, metrics.Sources{DefaultAdminPassword: func() bool { return true }})
	t0 := time.Now()

	metrics.Tick(t0)
	first, _ := hasProblem(metrics.CurrentStatus(), "default_admin_password")
	metrics.Tick(t0.Add(time.Second))
	second, _ := hasProblem(metrics.CurrentStatus(), "default_admin_password")

	if first.Since.IsZero() || !first.Since.Equal(second.Since) {
		t.Fatalf("since must stay the same: %v %v", first.Since, second.Since)
	}
}

func TestOverview(t *testing.T) {
	setup(t, metrics.Sources{
		KVKeys: func() (uint64, uint64) { return 10, 1 },
		EntityCounts: func() map[string]int {
			return map[string]int{"a": 1, "b": 6, "c": 5, "d": 4, "e": 3, "f": 2}
		},
	})
	t0 := time.Now()

	metrics.Tick(t0)
	for i := 0; i < 9; i++ {
		metrics.ObserveHTTP("GET", "/api/{entity}", 200, time.Millisecond, "")
	}
	metrics.ObserveHTTP("GET", "/api/{entity}", 503, time.Millisecond, "")
	metrics.Tick(t0.Add(time.Second))

	o := metrics.CurrentOverview()

	if o.Instance.AuthMode != "user" || o.Instance.Engine != "internal" || !o.Instance.HTTP.Enabled || o.Instance.HTTP.Address != "127.0.0.1:8089" {
		t.Fatalf("unexpected instance %+v", o.Instance)
	}
	if o.Traffic.HTTPRequestsPerSecond == nil || float64(*o.Traffic.HTTPRequestsPerSecond) != 10 {
		t.Fatalf("unexpected traffic %+v", o.Traffic)
	}
	if o.Traffic.ErrorRate == nil || float64(*o.Traffic.ErrorRate) != 0.1 {
		t.Fatalf("unexpected error rate %v", o.Traffic.ErrorRate)
	}
	if o.Data.Entities != 21 || o.Data.EntityTypes != 6 || len(o.Data.TopEntityTypes) != 5 || o.Data.TopEntityTypes[0].Name != "b" {
		t.Fatalf("unexpected data %+v", o.Data)
	}
	if o.Data.KVKeys == nil || float64(*o.Data.KVKeys) != 10 {
		t.Fatalf("unexpected kv keys %v", o.Data.KVKeys)
	}

	data, err := json.Marshal(o)
	if err != nil {
		t.Fatalf("marshal: %v", err)
	}
	for _, key := range []string{`"instance"`, `"status"`, `"traffic"`, `"data"`, `"persistence"`, `"runtime"`, `"connections"`, `"logs"`} {
		if !strings.Contains(string(data), key) {
			t.Fatalf("missing %s in %s", key, data)
		}
	}
}

func TestOverviewCountsLogLines(t *testing.T) {
	setup(t, metrics.Sources{})
	_ = log.Configure(log.Options{})
	log.SetOutput(&strings.Builder{}, false)
	t.Cleanup(func() { log.SetOutput(nil, false) })

	t0 := time.Now().Add(-15 * time.Second)
	metrics.Tick(t0)
	log.Warn("w1")
	log.Warn("w2")
	log.Error("e1")
	for i := 1; i <= 11; i++ {
		metrics.Tick(t0.Add(time.Duration(i) * time.Second))
	}

	o := metrics.CurrentOverview()
	if o.Logs.Warnings1h != 2 || o.Logs.Errors1h != 1 || o.Logs.Level != "info" {
		t.Fatalf("unexpected log summary %+v", o.Logs)
	}
}

func TestDetails(t *testing.T) {
	setup(t, metrics.Sources{EntityCounts: func() map[string]int { return map[string]int{"books": 2, "authors": 9} }})
	now := time.Now()

	for i := 0; i < 3; i++ {
		metrics.ObserveHTTP("GET", "/api/{entity}", 200, time.Millisecond, "")
	}
	metrics.ObserveHTTP("POST", "/api/{entity}", 500, time.Millisecond, "")
	metrics.ObserveHook("h1", "books", "pre_read", 2*time.Millisecond, nil)
	metrics.ObserveHook("h1", "books", "pre_read", 4*time.Millisecond, errors.New("boom"))
	metrics.Tick(now)

	d := metrics.CurrentDetails()

	routes := d.Routes.SinceStartup
	if len(routes) != 2 || routes[0].Route != "/api/{entity}" || routes[0].Method != "GET" || routes[0].Count != 3 {
		t.Fatalf("unexpected routes %+v", routes)
	}
	if routes[1].Errors != 1 || routes[1].ErrorRate != 1 {
		t.Fatalf("unexpected error counts %+v", routes[1])
	}

	if len(d.Hooks) != 1 || d.Hooks[0].Executions != 2 || d.Hooks[0].Errors != 1 || d.Hooks[0].LastError != "boom" || d.Hooks[0].Phase != "pre_read" {
		t.Fatalf("unexpected hooks %+v", d.Hooks)
	}
	if d.Hooks[0].AvgMs == nil || float64(*d.Hooks[0].AvgMs) != 3 {
		t.Fatalf("unexpected average %v", d.Hooks[0].AvgMs)
	}

	if len(d.EntityTypes) != 2 || d.EntityTypes[0].Name != "authors" {
		t.Fatalf("unexpected entity types %+v", d.EntityTypes)
	}
}

func TestPrometheus(t *testing.T) {
	setup(t, metrics.Sources{EntityCounts: func() map[string]int { return map[string]int{`we"ird`: 1} }})
	now := time.Now()

	metrics.ObserveHTTP("GET", "/api/{entity}", 200, time.Millisecond, "")
	metrics.ObserveHTTP("GET", "/api/{entity}", 500, 30*time.Second, "")
	metrics.ObserveTCP(metrics.TCPSet, time.Millisecond)
	metrics.Tick(now)

	out := string(metrics.Prometheus())

	for _, want := range []string{
		"# TYPE elysiandb_http_requests_total counter",
		`elysiandb_http_requests_total{status_class="2xx"} 1`,
		`elysiandb_http_requests_total{status_class="5xx"} 1`,
		`elysiandb_tcp_commands_total{command="SET"} 1`,
		"# TYPE elysiandb_http_request_duration_seconds histogram",
		`elysiandb_http_request_duration_seconds_bucket{le="+Inf"} 2`,
		"elysiandb_http_request_duration_seconds_count 2",
		`elysiandb_http_request_duration_seconds_bucket{le="10"} 1`,
		`elysiandb_entities{entity_type="we\"ird"} 1`,
		"# TYPE elysiandb_uptime_seconds gauge",
	} {
		if !strings.Contains(out, want) {
			t.Fatalf("missing %q in:\n%s", want, out)
		}
	}

	if strings.Count(out, "# TYPE elysiandb_http_requests_total") != 1 {
		t.Fatalf("each metric must be declared once")
	}
}

func TestAuthenticationOffOnPublicAddress(t *testing.T) {
	cases := []struct {
		enabled bool
		hosts   []string
		want    bool
	}{
		{false, []string{"0.0.0.0"}, true},
		{false, []string{"127.0.0.1", "::1", "localhost"}, false},
		{false, []string{"127.0.0.1", "192.168.1.4"}, true},
		{true, []string{"0.0.0.0"}, false},
		{false, nil, false},
	}

	for _, c := range cases {
		if got := metrics.AuthenticationOffOnPublicAddress(c.enabled, c.hosts); got != c.want {
			t.Fatalf("AuthenticationOffOnPublicAddress(%v, %v) = %v", c.enabled, c.hosts, got)
		}
	}

	if !metrics.IsLoopbackHost("[::1]") || metrics.IsLoopbackHost("") {
		t.Fatalf("unexpected loopback detection")
	}
}
