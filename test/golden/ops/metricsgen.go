package main

import (
	"encoding/json"
	"errors"
	"regexp"
	"sort"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/test/golden/goldenio"
)

var runtimeKeys = map[string]bool{
	"heap_bytes": true, "heap_live_bytes": true, "total_bytes": true, "held_bytes": true, "goroutines": true, "gomaxprocs": true,
	"go_version": true, "open_fds": true, "uptime_seconds": true, "uptime": true, "started_at": true, "rss_bytes": true,
	"gc_pause_p99_ms": true, "gc_cycles_rps": true, "gc_cycles": true, "gc_pause_ms": true, "cpu_percent": true, "cpu_user_seconds": true,
	"cpu_system_seconds": true, "cpu_seconds": true, "pid": true, "now": true, "version": true, "startup_ms": true, "generated_at": true, "since": true,
}

func maskJSON(v any) any {
	switch x := v.(type) {
	case map[string]any:
		out := map[string]any{}
		for k, val := range x {
			if runtimeKeys[k] || strings.HasPrefix(k, "cpu_") || strings.HasPrefix(k, "gc_") {
				out[k] = "<MASK>"
				continue
			}
			out[k] = maskJSON(val)
		}
		return out
	case string:
		if rfcTime.MatchString(x) {
			return "<T>"
		}
		return x
	case []any:
		out := make([]any, len(x))
		for i, e := range x {
			out[i] = maskJSON(e)
		}
		return out
	}
	return v
}

func maskedJSON(v any) (string, error) {
	b, err := json.Marshal(v)
	if err != nil {
		return "", err
	}
	var generic any
	if err := json.Unmarshal(b, &generic); err != nil {
		return "", err
	}
	m, err := json.Marshal(maskJSON(generic))
	return string(m), err
}

var rfcTime = regexp.MustCompile(`^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}`)

var promRuntime = regexp.MustCompile(`(?m)^(elysiandb_(?:goroutines|gomaxprocs|heap_bytes|heap_live_bytes|memory_bytes|total_bytes|held_bytes|rss_bytes|open_fds|cpu_[a-z_]+|gc_[a-z_]+|uptime_seconds|start_time_seconds|process_[a-z_]+|build_info)(?:\{[^}]*\})?) .*$`)

func genMetrics(out string) error {
	globals.SetConfig(&configuration.Config{Stats: configuration.StatsConfig{Enabled: true}})
	metrics.Reset()
	metrics.SetSources(metrics.Sources{
		DataFolder:           func() string { return "/data" },
		KVKeys:               func() (uint64, uint64) { return 10, 2 },
		EntityCounts:         func() map[string]int { return map[string]int{"book": 30, "author": 8} },
		IndexBacklog:         func() int { return 3 },
		CacheEntries:         func() int { return 12 },
		RecoverySizes:        func() (int64, int64) { return 1 << 20, 4 << 20 },
		AdminSessions:        func() int { return 1 },
		Transactions:         func() (int, time.Duration) { return 2, 1500 * time.Millisecond },
		HTTPConnections:      func() int { return 5 },
		DefaultAdminPassword: func() bool { return true },
		HeavyWaiting:         func() int { return 1 },
	})
	start := time.Unix(1800000000, 0).UTC()
	for s := 0; s < 130; s++ {
		now := start.Add(time.Duration(s) * time.Second)
		for i := 0; i < s%7; i++ {
			metrics.ObserveHTTP("GET", "/api/{entity}", 200, time.Duration(1+i*3)*time.Millisecond, "list")
		}
		if s%5 == 0 {
			metrics.ObserveHTTP("POST", "/api/{entity}", 400, 2*time.Millisecond, "write")
			metrics.ObserveTCP("GET", 300*time.Microsecond)
			metrics.KVHit()
			metrics.KVMiss()
			metrics.CacheHit()
		}
		if s%11 == 0 {
			metrics.ObserveHook("h1", "book", "post_read", 4*time.Millisecond, nil)
			metrics.ObserveHookQuery("h1", "book", "post_read", 20, s%22 == 0, time.Millisecond)
			metrics.SlowRequest()
			metrics.FailedLogin("203.0.113.5")
		}
		if s == 60 {
			metrics.ObserveHook("h2", "book", "pre_read", time.Millisecond, errors.New("boom"))
			metrics.RecordFlush("checkpoint", 40*time.Millisecond, true, nil)
			metrics.HeavyQuery(15 * time.Millisecond)
			metrics.HeavyRejected()
			metrics.RequestAbandoned()
		}
		metrics.Tick(now)
	}
	var l goldenio.Lines
	prom := promRuntime.ReplaceAllString(string(metrics.Prometheus()), "$1 <MASK>")
	if err := goldenio.WriteFile(out, "metrics/prometheus.prom", []byte(prom)); err != nil {
		return err
	}
	for _, r := range []string{"15m", "1h", "24h", "7d", ""} {
		for _, since := range []int64{0, start.Add(100 * time.Second).UnixMilli(), start.Add(200 * time.Second).UnixMilli()} {
			resp, ok := metrics.Series(r, since)
			s, err := maskedJSON(resp)
			if err != nil {
				return err
			}
			l.Add(map[string]any{"fn": "Series", "range": r, "since": since, "ok": ok, "body": s})
		}
	}
	names := metrics.SeriesNames()
	sort.Strings(names)
	l.Add(map[string]any{"fn": "SeriesNames", "names": names})
	for name, v := range map[string]any{"overview": metrics.CurrentOverview(), "details": metrics.CurrentDetails(), "status": metrics.CurrentStatus(), "instance": metrics.CurrentInstance()} {
		s, err := maskedJSON(v)
		if err != nil {
			return err
		}
		if err := goldenio.WriteFile(out, "metrics/"+name+".json", []byte(s+"\n")); err != nil {
			return err
		}
	}
	metrics.Reset()
	return l.Write(out, "metrics/series.jsonl")
}
