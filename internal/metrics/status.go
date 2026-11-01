package metrics

import (
	"net"
	"sort"
	"strings"
	"sync"
	"time"

	"github.com/taymour/elysiandb/internal/globals"
)

const (
	SeverityCritical = "critical"
	SeverityWarning  = "warning"
	SeverityInfo     = "info"

	StatusHealthy  = "healthy"
	StatusWarning  = "warning"
	StatusCritical = "critical"
)

const (
	recentFailureWindow   = 5 * time.Minute
	minimumWindowRequests = 20
	errorRateThreshold    = 0.01
	authFailureThreshold  = 20
	indexBacklogDelay     = time.Minute
	oldTransactionAge     = 10 * time.Minute
	uncleanShutdownNotice = time.Hour
)

type Problem struct {
	Code     string         `json:"code"`
	Severity string         `json:"severity"`
	Since    time.Time      `json:"since"`
	Details  map[string]any `json:"details,omitempty"`
}

type Status struct {
	Level    string    `json:"level"`
	Problems []Problem `json:"problems"`
}

type problemTracker struct {
	mu     sync.Mutex
	active map[string]Problem
}

func newProblemTracker() *problemTracker {
	return &problemTracker{active: map[string]Problem{}}
}

func (t *problemTracker) update(now time.Time, found []Problem) {
	t.mu.Lock()
	defer t.mu.Unlock()

	next := map[string]Problem{}
	for _, p := range found {
		if prev, ok := t.active[p.Code]; ok {
			p.Since = prev.Since
		} else if p.Since.IsZero() {
			p.Since = now
		}
		next[p.Code] = p
	}

	t.active = next
}

func (t *problemTracker) status() Status {
	t.mu.Lock()
	defer t.mu.Unlock()

	status := Status{Level: StatusHealthy, Problems: []Problem{}}
	for _, p := range t.active {
		status.Problems = append(status.Problems, p)
		switch p.Severity {
		case SeverityCritical:
			status.Level = StatusCritical
		case SeverityWarning:
			if status.Level == StatusHealthy {
				status.Level = StatusWarning
			}
		}
	}

	order := map[string]int{SeverityCritical: 0, SeverityWarning: 1, SeverityInfo: 2}
	sort.Slice(status.Problems, func(i, j int) bool {
		a, b := status.Problems[i], status.Problems[j]
		if order[a.Severity] != order[b.Severity] {
			return order[a.Severity] < order[b.Severity]
		}
		return a.Code < b.Code
	})

	return status
}

func CurrentStatus() Status {
	return state.problems.status()
}

func (s *sampler) evaluate(now time.Time) []Problem {
	cfg := globals.GetConfig()
	problems := []Problem{}

	if since := flushFailingSince(); !since.IsZero() {
		problems = append(problems, Problem{Code: "flush_failing", Severity: SeverityCritical, Since: since})
	}

	if at, _ := recoveryState.get(); !at.IsZero() && now.Sub(at) < recentFailureWindow {
		problems = append(problems, Problem{Code: "recovery_write_failing", Severity: SeverityCritical})
	}

	s.mu.RLock()
	slow := s.slow
	backlogFrom := s.backlogFrom
	prev, cur := s.window(recentFailureWindow)
	sources := s.sources
	s.mu.RUnlock()

	if sources.MongoPing != nil && slow.mongoErr != nil {
		problems = append(problems, Problem{Code: "mongodb_unreachable", Severity: SeverityCritical})
	}

	if prev != nil && cur != nil {
		requests := counterDelta(prev, cur, cHTTP)
		failures := counterDelta(prev, cur, cHTTP5xx)
		if requests >= minimumWindowRequests && failures/requests > errorRateThreshold {
			problems = append(problems, Problem{Code: "error_rate_high", Severity: SeverityWarning, Details: map[string]any{"error_rate": round(failures / requests)}})
		}

		if cfg != nil && cfg.Log.SlowRequestMs > 0 {
			latency := cur.http.sub(prev.http).add(cur.tcp.sub(prev.tcp))
			if latency.total() >= minimumWindowRequests {
				if p95 := latency.quantile(0.95); p95 > float64(cfg.Log.SlowRequestMs) {
					problems = append(problems, Problem{Code: "latency_high", Severity: SeverityWarning, Details: map[string]any{"p95_ms": round(p95)}})
				}
			}
		}
	}

	if at, hookID := hookFailure.get(); !at.IsZero() && now.Sub(at) < recentFailureWindow {
		problems = append(problems, Problem{Code: "hook_failing", Severity: SeverityWarning, Details: map[string]any{"hook_id": hookID}})
	}

	if addr, count := authFailures.worst(); count > authFailureThreshold {
		problems = append(problems, Problem{Code: "auth_failures", Severity: SeverityWarning, Details: map[string]any{"remote_addr": addr, "per_minute": count}})
	}

	if !backlogFrom.IsZero() && now.Sub(backlogFrom) > indexBacklogDelay {
		problems = append(problems, Problem{Code: "index_backlog", Severity: SeverityWarning, Since: backlogFrom})
	}

	if cur != nil {
		if oldest := cur.gauges[sOldestTransaction]; oldest > oldTransactionAge.Seconds() {
			problems = append(problems, Problem{Code: "old_transaction", Severity: SeverityWarning, Details: map[string]any{"seconds": int(oldest)}})
		}
	}

	if slow.defaultAdminPw {
		problems = append(problems, Problem{Code: "default_admin_password", Severity: SeverityWarning})
	}

	if cfg != nil && AuthenticationOffOnPublicAddress(cfg.Security.Authentication.Enabled, publicListeners()) {
		problems = append(problems, Problem{Code: "auth_disabled_public", Severity: SeverityWarning})
	}

	if entries := replayed.Load(); entries > 0 && now.Sub(startedAt) < uncleanShutdownNotice {
		problems = append(problems, Problem{Code: "unclean_shutdown", Severity: SeverityInfo, Since: startedAt, Details: map[string]any{"entries": entries}})
	}

	return problems
}

func counterDelta(prev, cur *snapshot, c int) float64 {
	if cur.counters[c] < prev.counters[c] {
		return 0
	}

	return float64(cur.counters[c] - prev.counters[c])
}

func publicListeners() []string {
	cfg := globals.GetConfig()
	if cfg == nil {
		return nil
	}

	hosts := []string{}
	if cfg.Server.HTTP.Enabled {
		hosts = append(hosts, cfg.Server.HTTP.Host)
	}
	if cfg.Server.TCP.Enabled {
		hosts = append(hosts, cfg.Server.TCP.Host)
	}

	return hosts
}

func AuthenticationOffOnPublicAddress(authEnabled bool, hosts []string) bool {
	if authEnabled {
		return false
	}

	for _, host := range hosts {
		if !IsLoopbackHost(host) {
			return true
		}
	}

	return false
}

func IsLoopbackHost(host string) bool {
	host = strings.Trim(strings.TrimSpace(host), "[]")
	if strings.EqualFold(host, "localhost") {
		return true
	}

	ip := net.ParseIP(host)

	return ip != nil && ip.IsLoopback()
}
