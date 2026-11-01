package metrics

import (
	"sort"
	"sync"
	"sync/atomic"
	"time"
)

const (
	cHTTP = iota
	cHTTP2xx
	cHTTP3xx
	cHTTP4xx
	cHTTP5xx
	cUnauthorized
	cForbidden
	cSlow
	cTCP
	cTCPGet
	cTCPSet
	cTCPMget
	cTCPDel
	cTCPOther
	cTCPConnections
	cFailedLogins
	cHookExecutions
	cHookErrors
	cKVGets
	cKVSets
	cKVDeletes
	cKVHits
	cKVMisses
	cKVExpired
	cDocReads
	cDocWrites
	cDocQueries
	cCacheHits
	cCacheMisses
	cFlushes
	cFlushNanos
	cFlushFailures
	cForcedFlushes
	cMongoErrors
	cRecoveryFailures
	numCounters
)

const (
	TCPGet   = "GET"
	TCPSet   = "SET"
	TCPMget  = "MGET"
	TCPDel   = "DEL"
	TCPOther = "OTHER"
)

const (
	KindKVGet    = "kv_get"
	KindKVSet    = "kv_set"
	KindKVDelete = "kv_delete"
	KindDocRead  = "doc_read"
	KindDocWrite = "doc_write"
	KindDocQuery = "doc_query"
)

var (
	counters      [numCounters]atomic.Uint64
	httpLatency   Histogram
	tcpLatency    Histogram
	hookLatency   Histogram
	tcpOpen       atomic.Int64
	startedAt     = time.Now()
	routeStats    sync.Map
	hookStats     sync.Map
	authFailures  = newAddressCounter()
	persistence   = &persistenceState{}
	recoveryState = &timedError{}
	hookFailure   = &timedError{}
	replayed      atomic.Int64
)

func StartedAt() time.Time {
	return startedAt
}

func add(i int, n uint64) {
	counters[i].Add(n)
}

func ObserveHTTP(method, route string, status int, d time.Duration, kind string) {
	add(cHTTP, 1)

	switch {
	case status >= 500:
		add(cHTTP5xx, 1)
	case status >= 400:
		add(cHTTP4xx, 1)
		if status == 401 {
			add(cUnauthorized, 1)
		}
		if status == 403 {
			add(cForbidden, 1)
		}
	case status >= 300:
		add(cHTTP3xx, 1)
	default:
		add(cHTTP2xx, 1)
	}

	switch kind {
	case KindKVGet:
		add(cKVGets, 1)
	case KindKVSet:
		add(cKVSets, 1)
	case KindKVDelete:
		add(cKVDeletes, 1)
	case KindDocRead:
		add(cDocReads, 1)
	case KindDocWrite:
		add(cDocWrites, 1)
	case KindDocQuery:
		add(cDocQueries, 1)
	}

	httpLatency.Observe(d)
	routeFor(method, route).observe(status, d)
}

func ObserveTCP(command string, d time.Duration) {
	add(cTCP, 1)

	switch command {
	case TCPGet:
		add(cTCPGet, 1)
		add(cKVGets, 1)
	case TCPMget:
		add(cTCPMget, 1)
		add(cKVGets, 1)
	case TCPSet:
		add(cTCPSet, 1)
		add(cKVSets, 1)
	case TCPDel:
		add(cTCPDel, 1)
		add(cKVDeletes, 1)
	default:
		add(cTCPOther, 1)
	}

	tcpLatency.Observe(d)
}

func SlowRequest() {
	add(cSlow, 1)
}

func TCPConnectionOpened() {
	add(cTCPConnections, 1)
	tcpOpen.Add(1)
}

func TCPConnectionClosed() {
	tcpOpen.Add(-1)
}

func FailedLogin(addr string) {
	add(cFailedLogins, 1)
	authFailures.add(addr)
}

func AuthFailure(addr string) {
	authFailures.add(addr)
}

func KVHit() {
	add(cKVHits, 1)
}

func KVMiss() {
	add(cKVMisses, 1)
}

func KVExpired(n int) {
	if n > 0 {
		add(cKVExpired, uint64(n))
	}
}

func CacheHit() {
	add(cCacheHits, 1)
}

func CacheMiss() {
	add(cCacheMisses, 1)
}

func ForcedFlush() {
	add(cForcedFlushes, 1)
}

func MongoError() {
	add(cMongoErrors, 1)
}

func RecoveryWriteFailed(err error) {
	add(cRecoveryFailures, 1)
	recoveryState.set(err.Error(), "")
}

func RecoveryReplayed(entries int) {
	replayed.Add(int64(entries))
}

func ObserveHook(id, entity, phase string, d time.Duration, err error) {
	add(cHookExecutions, 1)
	hookLatency.Observe(d)

	v, _ := hookStats.LoadOrStore(id, &hookStat{id: id, entity: entity, phase: phase})
	hs := v.(*hookStat)
	hs.executions.Add(1)
	hs.latency.Observe(d)
	hs.totalNanos.Add(uint64(d))

	if err != nil {
		add(cHookErrors, 1)
		hs.errors.Add(1)
		hs.mu.Lock()
		hs.lastError = err.Error()
		hs.lastErrorAt = time.Now()
		hs.mu.Unlock()
		hookFailure.set(err.Error(), id)
	}
}

type persistenceState struct {
	mu            sync.Mutex
	targets       map[string]bool
	lastFlushAt   time.Time
	lastSuccessAt time.Time
	lastDuration  time.Duration
	lastError     string
	failingSince  time.Time
}

func RecordFlush(target string, d time.Duration, wrote bool, err error) {
	if wrote || err != nil {
		add(cFlushes, 1)
		add(cFlushNanos, uint64(d))
	}

	if err != nil {
		add(cFlushFailures, 1)
	}

	p := persistence
	p.mu.Lock()
	defer p.mu.Unlock()

	if p.targets == nil {
		p.targets = map[string]bool{}
	}

	now := time.Now()
	p.lastFlushAt = now
	if wrote || err != nil {
		p.lastDuration = d
	}

	p.targets[target] = err == nil
	if err != nil {
		p.lastError = err.Error()
		if p.failingSince.IsZero() {
			p.failingSince = now
		}
		return
	}

	for _, ok := range p.targets {
		if !ok {
			return
		}
	}

	p.lastError = ""
	p.failingSince = time.Time{}
	p.lastSuccessAt = now
}

type PersistenceStatus struct {
	LastFlushAt   *time.Time `json:"last_flush_at"`
	LastFlushMs   float64    `json:"last_flush_ms"`
	LastFlushOK   bool       `json:"last_flush_ok"`
	LastError     string     `json:"last_flush_error"`
	FlushFailures uint64     `json:"flush_failures"`
	LastSuccessAt *time.Time `json:"last_success_at"`
}

func Persistence() PersistenceStatus {
	p := persistence
	p.mu.Lock()
	defer p.mu.Unlock()

	status := PersistenceStatus{
		LastFlushMs:   round(float64(p.lastDuration) / float64(time.Millisecond)),
		LastFlushOK:   p.failingSince.IsZero(),
		LastError:     p.lastError,
		FlushFailures: counters[cFlushFailures].Load(),
	}

	if !p.lastFlushAt.IsZero() {
		t := p.lastFlushAt
		status.LastFlushAt = &t
	}

	if !p.lastSuccessAt.IsZero() {
		t := p.lastSuccessAt
		status.LastSuccessAt = &t
	}

	return status
}

func flushFailingSince() time.Time {
	persistence.mu.Lock()
	defer persistence.mu.Unlock()

	return persistence.failingSince
}

type timedError struct {
	mu      sync.Mutex
	at      time.Time
	message string
	key     string
}

func (t *timedError) set(message, key string) {
	t.mu.Lock()
	t.at = time.Now()
	t.message = message
	t.key = key
	t.mu.Unlock()
}

func (t *timedError) get() (time.Time, string) {
	t.mu.Lock()
	defer t.mu.Unlock()

	return t.at, t.key
}

type routeStat struct {
	method  string
	route   string
	count   atomic.Uint64
	errors  atomic.Uint64
	latency Histogram
}

func routeFor(method, route string) *routeStat {
	key := method + " " + route
	if v, ok := routeStats.Load(key); ok {
		return v.(*routeStat)
	}

	v, _ := routeStats.LoadOrStore(key, &routeStat{method: method, route: route})

	return v.(*routeStat)
}

func (r *routeStat) observe(status int, d time.Duration) {
	r.count.Add(1)
	if status >= 500 {
		r.errors.Add(1)
	}
	r.latency.Observe(d)
}

type routeSnapshot struct {
	method string
	route  string
	count  uint64
	errors uint64
	hist   hist
}

func snapshotRoutes() map[string]routeSnapshot {
	out := map[string]routeSnapshot{}
	routeStats.Range(func(k, v any) bool {
		r := v.(*routeStat)
		out[k.(string)] = routeSnapshot{
			method: r.method,
			route:  r.route,
			count:  r.count.Load(),
			errors: r.errors.Load(),
			hist:   r.latency.snapshot(),
		}
		return true
	})

	return out
}

type RouteSummary struct {
	Method    string  `json:"method"`
	Route     string  `json:"route"`
	Count     uint64  `json:"count"`
	Errors    uint64  `json:"errors"`
	ErrorRate float64 `json:"error_rate"`
	P95Ms     *Number `json:"p95_ms"`
}

func topRoutes(current, previous map[string]routeSnapshot, limit int) []RouteSummary {
	out := []RouteSummary{}
	for key, cur := range current {
		prev := previous[key]
		count := cur.count - min(prev.count, cur.count)
		if count == 0 {
			continue
		}

		errors := cur.errors - min(prev.errors, cur.errors)
		out = append(out, RouteSummary{
			Method:    cur.method,
			Route:     cur.route,
			Count:     count,
			Errors:    errors,
			ErrorRate: round(float64(errors) / float64(count)),
			P95Ms:     num(cur.hist.sub(prev.hist).quantile(0.95)),
		})
	}

	sort.Slice(out, func(i, j int) bool {
		if out[i].Count != out[j].Count {
			return out[i].Count > out[j].Count
		}
		return out[i].Method+out[i].Route < out[j].Method+out[j].Route
	})

	if len(out) > limit {
		out = out[:limit]
	}

	return out
}

type hookStat struct {
	id          string
	entity      string
	phase       string
	executions  atomic.Uint64
	errors      atomic.Uint64
	totalNanos  atomic.Uint64
	latency     Histogram
	mu          sync.Mutex
	lastError   string
	lastErrorAt time.Time
}

type HookSummary struct {
	HookID      string     `json:"hook_id"`
	Entity      string     `json:"entity"`
	Phase       string     `json:"phase"`
	Executions  uint64     `json:"executions"`
	Errors      uint64     `json:"errors"`
	AvgMs       *Number    `json:"avg_ms"`
	P95Ms       *Number    `json:"p95_ms"`
	LastError   string     `json:"last_error"`
	LastErrorAt *time.Time `json:"last_error_at"`
}

func hookSummaries() []HookSummary {
	out := []HookSummary{}
	hookStats.Range(func(_, v any) bool {
		h := v.(*hookStat)
		executions := h.executions.Load()
		summary := HookSummary{
			HookID:     h.id,
			Entity:     h.entity,
			Phase:      h.phase,
			Executions: executions,
			Errors:     h.errors.Load(),
			P95Ms:      num(h.latency.snapshot().quantile(0.95)),
		}

		if executions > 0 {
			summary.AvgMs = num(float64(h.totalNanos.Load()) / float64(executions) / float64(time.Millisecond))
		}

		h.mu.Lock()
		summary.LastError = h.lastError
		if !h.lastErrorAt.IsZero() {
			t := h.lastErrorAt
			summary.LastErrorAt = &t
		}
		h.mu.Unlock()

		out = append(out, summary)
		return true
	})

	sort.Slice(out, func(i, j int) bool {
		return out[i].Executions > out[j].Executions
	})

	return out
}

const addressCounterMax = 10_000

type addressCounter struct {
	mu       sync.Mutex
	current  map[string]int
	previous map[string]int
	minute   int64
}

func newAddressCounter() *addressCounter {
	return &addressCounter{current: map[string]int{}, previous: map[string]int{}}
}

func (a *addressCounter) rotate(now time.Time) {
	minute := now.Unix() / 60
	if minute == a.minute {
		return
	}

	if minute == a.minute+1 {
		a.previous = a.current
	} else {
		a.previous = map[string]int{}
	}

	a.current = map[string]int{}
	a.minute = minute
}

func (a *addressCounter) add(addr string) {
	a.mu.Lock()
	defer a.mu.Unlock()

	a.rotate(time.Now())
	if _, ok := a.current[addr]; !ok && len(a.current) >= addressCounterMax {
		return
	}

	a.current[addr]++
}

func (a *addressCounter) worst() (string, int) {
	a.mu.Lock()
	defer a.mu.Unlock()

	a.rotate(time.Now())

	worstAddr, worstCount := "", 0
	for _, m := range []map[string]int{a.current, a.previous} {
		for addr, count := range m {
			if count > worstCount || (count == worstCount && addr < worstAddr) {
				worstAddr, worstCount = addr, count
			}
		}
	}

	return worstAddr, worstCount
}
