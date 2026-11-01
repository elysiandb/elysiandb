package metrics

import (
	"fmt"
	"math"
	"runtime"
	"sort"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
)

type Listener struct {
	Enabled bool   `json:"enabled"`
	Address string `json:"address"`
}

type Instance struct {
	Version       string          `json:"version"`
	GoVersion     string          `json:"go_version"`
	Engine        string          `json:"engine"`
	StartedAt     time.Time       `json:"started_at"`
	UptimeSeconds int64           `json:"uptime_seconds"`
	HTTP          Listener        `json:"http"`
	TCP           Listener        `json:"tcp"`
	AuthMode      string          `json:"auth_mode"`
	Features      map[string]bool `json:"features"`
	DataFolder    string          `json:"data_folder"`
}

type Traffic struct {
	RequestsPerSecond     *Number `json:"requests_per_second"`
	HTTPRequestsPerSecond *Number `json:"http_requests_per_second"`
	TCPCommandsPerSecond  *Number `json:"tcp_commands_per_second"`
	ErrorRate             *Number `json:"error_rate"`
	P95Ms                 *Number `json:"p95_ms"`
	SlowRequestsPerMinute *Number `json:"slow_requests_per_minute"`
}

type EntityCount struct {
	Name  string `json:"name"`
	Count int    `json:"count"`
}

type Data struct {
	KVKeys         *Number       `json:"kv_keys"`
	KVKeysWithTTL  *Number       `json:"kv_keys_with_ttl"`
	EntityTypes    int           `json:"entity_types"`
	Entities       int           `json:"entities"`
	DiskBytes      *Number       `json:"disk_bytes"`
	TopEntityTypes []EntityCount `json:"top_entity_types"`
}

type RuntimeInfo struct {
	HeapBytes  *Number `json:"heap_bytes"`
	TotalBytes *Number `json:"total_bytes"`
	Goroutines *Number `json:"goroutines"`
	CPUPercent *Number `json:"cpu_percent"`
}

type Connections struct {
	HTTP                     *Number `json:"http"`
	TCP                      *Number `json:"tcp"`
	AdminSessions            *Number `json:"admin_sessions"`
	OpenTransactions         *Number `json:"open_transactions"`
	OldestTransactionSeconds *Number `json:"oldest_transaction_seconds"`
}

type LogSummary struct {
	Level       string `json:"level"`
	Errors1h    int    `json:"errors_1h"`
	Warnings1h  int    `json:"warnings_1h"`
	Errors24h   int    `json:"errors_24h"`
	Warnings24h int    `json:"warnings_24h"`
}

type Overview struct {
	GeneratedAt time.Time         `json:"generated_at"`
	Instance    Instance          `json:"instance"`
	Status      Status            `json:"status"`
	Traffic     Traffic           `json:"traffic"`
	Data        Data              `json:"data"`
	Persistence PersistenceStatus `json:"persistence"`
	Runtime     RuntimeInfo       `json:"runtime"`
	Connections Connections       `json:"connections"`
	Logs        LogSummary        `json:"logs"`
}

func AuthMode() string {
	cfg := globals.GetConfig()
	if cfg == nil || !cfg.Security.Authentication.Enabled {
		return "disabled"
	}

	return cfg.Security.Authentication.Mode
}

func CurrentInstance() Instance {
	cfg := globals.GetConfig()
	now := time.Now()

	instance := Instance{
		Version:       globals.VERSION,
		GoVersion:     runtime.Version(),
		Engine:        globals.GetEngine(),
		StartedAt:     startedAt,
		UptimeSeconds: int64(now.Sub(startedAt).Seconds()),
		AuthMode:      AuthMode(),
		DataFolder:    cfg.Store.Folder,
		HTTP:          Listener{Enabled: cfg.Server.HTTP.Enabled, Address: fmt.Sprintf("%s:%d", cfg.Server.HTTP.Host, cfg.Server.HTTP.Port)},
		TCP:           Listener{Enabled: cfg.Server.TCP.Enabled, Address: fmt.Sprintf("%s:%d", cfg.Server.TCP.Host, cfg.Server.TCP.Port)},
		Features: map[string]bool{
			"cache":          cfg.Api.Cache.Enabled,
			"hooks":          cfg.Api.Hooks.Enabled,
			"schema":         cfg.Api.Schema.Enabled,
			"crash_recovery": cfg.Store.CrashRecovery.Enabled,
			"admin_ui":       cfg.AdminUI.Enabled,
			"stats":          cfg.Stats.Enabled,
			"prometheus":     cfg.Monitoring.Prometheus.Enabled,
			"access_log":     cfg.Log.AccessLog,
		},
	}

	return instance
}

func CurrentOverview() Overview {
	now := time.Now()

	state.mu.RLock()
	slow := state.slow
	prev, cur := state.window(time.Minute)
	cpuPrev, cpuCur := state.window(10 * time.Second)
	var latest *snapshot
	if len(state.recent) > 0 {
		latest = state.recent[len(state.recent)-1]
	}
	tenSeconds := state.resolutions[1].ordered()
	minutes := state.resolutions[2].ordered()
	state.mu.RUnlock()

	overview := Overview{
		GeneratedAt: now,
		Instance:    CurrentInstance(),
		Status:      CurrentStatus(),
		Persistence: Persistence(),
		Data:        Data{TopEntityTypes: []EntityCount{}},
		Logs:        LogSummary{Level: log.CurrentLevel().Level},
	}

	if prev != nil && cur != nil {
		elapsed := cur.at.Sub(prev.at).Seconds()
		httpCount := counterDelta(prev, cur, cHTTP)
		tcpCount := counterDelta(prev, cur, cTCP)

		overview.Traffic = Traffic{
			RequestsPerSecond:     num((httpCount + tcpCount) / elapsed),
			HTTPRequestsPerSecond: num(httpCount / elapsed),
			TCPCommandsPerSecond:  num(tcpCount / elapsed),
			P95Ms:                 num(cur.http.sub(prev.http).add(cur.tcp.sub(prev.tcp)).quantile(0.95)),
			SlowRequestsPerMinute: num(counterDelta(prev, cur, cSlow) / elapsed * 60),
		}

		if httpCount > 0 {
			overview.Traffic.ErrorRate = num(counterDelta(prev, cur, cHTTP5xx) / httpCount)
		}
	}

	if latest != nil {
		overview.Data.KVKeys = num(latest.gauges[sKVKeys])
		overview.Data.KVKeysWithTTL = num(latest.gauges[sKVKeysTTL])
		overview.Data.DiskBytes = num(latest.gauges[sDiskBytes])
		overview.Runtime = RuntimeInfo{
			HeapBytes:  num(latest.gauges[sHeapBytes]),
			TotalBytes: num(latest.gauges[sTotalBytes]),
			Goroutines: num(latest.gauges[sGoroutines]),
		}
		overview.Connections = Connections{
			HTTP:                     num(latest.gauges[sHTTPConns]),
			TCP:                      num(latest.gauges[sTCPConns]),
			AdminSessions:            num(latest.gauges[sAdminSessions]),
			OpenTransactions:         num(latest.gauges[sOpenTransactions]),
			OldestTransactionSeconds: num(latest.gauges[sOldestTransaction]),
		}
	}

	if cpuPrev != nil && cpuCur != nil {
		overview.Runtime.CPUPercent = num(derive(cpuPrev, cpuCur).V[sCPUPercent])
	}

	types := make([]EntityCount, 0, len(slow.entityCounts))
	for name, count := range slow.entityCounts {
		types = append(types, EntityCount{Name: name, Count: count})
		overview.Data.Entities += count
	}
	overview.Data.EntityTypes = len(types)

	sort.Slice(types, func(i, j int) bool {
		if types[i].Count != types[j].Count {
			return types[i].Count > types[j].Count
		}
		return types[i].Name < types[j].Name
	})

	if len(types) > 5 {
		types = types[:5]
	}
	overview.Data.TopEntityTypes = types

	overview.Logs.Errors1h, overview.Logs.Warnings1h = sumLogs(tenSeconds, now.Add(-time.Hour), 10)
	overview.Logs.Errors24h, overview.Logs.Warnings24h = sumLogs(minutes, now.Add(-24*time.Hour), 60)

	return overview
}

func sumLogs(points []Point, since time.Time, stepSeconds float64) (int, int) {
	var errorsCount, warningsCount float64
	limit := since.UnixMilli()
	for _, p := range points {
		if p.T < limit {
			continue
		}
		if v := p.V[sLogErrorsRps]; !math.IsNaN(v) {
			errorsCount += v * stepSeconds
		}
		if v := p.V[sLogWarningsRps]; !math.IsNaN(v) {
			warningsCount += v * stepSeconds
		}
	}

	return int(math.Round(errorsCount)), int(math.Round(warningsCount))
}

type Routes struct {
	SinceStartup []RouteSummary `json:"since_startup"`
	Last15m      []RouteSummary `json:"last_15m"`
}

type Details struct {
	Routes      Routes        `json:"routes"`
	Hooks       []HookSummary `json:"hooks"`
	EntityTypes []EntityCount `json:"entity_types"`
}

func CurrentDetails() Details {
	current := snapshotRoutes()

	state.mu.RLock()
	var baseline map[string]routeSnapshot
	if len(state.routeMarks) > 0 {
		baseline = state.routeMarks[0]
	}
	counts := state.slow.entityCounts
	state.mu.RUnlock()

	details := Details{
		Routes: Routes{
			SinceStartup: topRoutes(current, nil, 10),
			Last15m:      topRoutes(current, baseline, 10),
		},
		Hooks:       hookSummaries(),
		EntityTypes: []EntityCount{},
	}

	for name, count := range counts {
		details.EntityTypes = append(details.EntityTypes, EntityCount{Name: name, Count: count})
	}

	sort.Slice(details.EntityTypes, func(i, j int) bool {
		if details.EntityTypes[i].Count != details.EntityTypes[j].Count {
			return details.EntityTypes[i].Count > details.EntityTypes[j].Count
		}
		return details.EntityTypes[i].Name < details.EntityTypes[j].Name
	})

	return details
}
