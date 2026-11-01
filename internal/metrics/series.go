package metrics

import (
	"math"
	"strconv"
)

const (
	sHTTPRps = iota
	sTCPRps
	sHTTP2xxRps
	sHTTP4xxRps
	sHTTP5xxRps
	sTCPGetRps
	sTCPSetRps
	sTCPMgetRps
	sTCPDelRps
	sTCPOtherRps
	sHTTPP50
	sHTTPP95
	sHTTPP99
	sTCPP50
	sTCPP95
	sTCPP99
	sSlowRps
	sUnauthorizedRps
	sForbiddenRps
	sFailedLoginsRps
	sHookErrorsRps
	sLogWarningsRps
	sLogErrorsRps
	sKVKeys
	sKVKeysTTL
	sKVGetsRps
	sKVSetsRps
	sKVDeletesRps
	sKVHitRatio
	sKVExpiredRps
	sDocReadsRps
	sDocWritesRps
	sDocQueriesRps
	sEntities
	sIndexBacklog
	sCacheEntries
	sCacheHitRatio
	sFlushMs
	sFlushFailuresRps
	sRecoveryJSONBytes
	sRecoveryStoreBytes
	sForcedFlushesRps
	sDiskBytes
	sHeapBytes
	sTotalBytes
	sGoroutines
	sGCPauseP99
	sGCCyclesRps
	sCPUPercent
	sOpenFDs
	sHTTPConns
	sTCPConns
	sAdminSessions
	sOpenTransactions
	sOldestTransaction
	sHookExecutionsRps
	sHookP95
	sMongoPingMs
	sMongoErrorsRps
	sMongoPoolInUse
	numSeries
)

var seriesNames = [numSeries]string{
	sHTTPRps:            "http_rps",
	sTCPRps:             "tcp_rps",
	sHTTP2xxRps:         "http_2xx_rps",
	sHTTP4xxRps:         "http_4xx_rps",
	sHTTP5xxRps:         "http_5xx_rps",
	sTCPGetRps:          "tcp_get_rps",
	sTCPSetRps:          "tcp_set_rps",
	sTCPMgetRps:         "tcp_mget_rps",
	sTCPDelRps:          "tcp_del_rps",
	sTCPOtherRps:        "tcp_other_rps",
	sHTTPP50:            "http_p50_ms",
	sHTTPP95:            "http_p95_ms",
	sHTTPP99:            "http_p99_ms",
	sTCPP50:             "tcp_p50_ms",
	sTCPP95:             "tcp_p95_ms",
	sTCPP99:             "tcp_p99_ms",
	sSlowRps:            "slow_rps",
	sUnauthorizedRps:    "unauthorized_rps",
	sForbiddenRps:       "forbidden_rps",
	sFailedLoginsRps:    "failed_logins_rps",
	sHookErrorsRps:      "hook_errors_rps",
	sLogWarningsRps:     "log_warnings_rps",
	sLogErrorsRps:       "log_errors_rps",
	sKVKeys:             "kv_keys",
	sKVKeysTTL:          "kv_keys_ttl",
	sKVGetsRps:          "kv_gets_rps",
	sKVSetsRps:          "kv_sets_rps",
	sKVDeletesRps:       "kv_deletes_rps",
	sKVHitRatio:         "kv_hit_ratio",
	sKVExpiredRps:       "kv_expired_rps",
	sDocReadsRps:        "doc_reads_rps",
	sDocWritesRps:       "doc_writes_rps",
	sDocQueriesRps:      "doc_queries_rps",
	sEntities:           "entities",
	sIndexBacklog:       "index_backlog",
	sCacheEntries:       "cache_entries",
	sCacheHitRatio:      "cache_hit_ratio",
	sFlushMs:            "flush_ms",
	sFlushFailuresRps:   "flush_failures_rps",
	sRecoveryJSONBytes:  "recovery_json_bytes",
	sRecoveryStoreBytes: "recovery_store_bytes",
	sForcedFlushesRps:   "forced_flushes_rps",
	sDiskBytes:          "disk_bytes",
	sHeapBytes:          "heap_bytes",
	sTotalBytes:         "total_bytes",
	sGoroutines:         "goroutines",
	sGCPauseP99:         "gc_pause_p99_ms",
	sGCCyclesRps:        "gc_cycles_rps",
	sCPUPercent:         "cpu_percent",
	sOpenFDs:            "open_fds",
	sHTTPConns:          "http_conns",
	sTCPConns:           "tcp_conns",
	sAdminSessions:      "admin_sessions",
	sOpenTransactions:   "open_transactions",
	sOldestTransaction:  "oldest_transaction_seconds",
	sHookExecutionsRps:  "hook_executions_rps",
	sHookP95:            "hook_p95_ms",
	sMongoPingMs:        "mongo_ping_ms",
	sMongoErrorsRps:     "mongo_errors_rps",
	sMongoPoolInUse:     "mongo_pool_in_use",
}

func SeriesNames() []string {
	return seriesNames[:]
}

type Point struct {
	T int64
	V [numSeries]float64
}

var rateSeries = map[int]int{
	sHTTPRps:           cHTTP,
	sTCPRps:            cTCP,
	sHTTP2xxRps:        cHTTP2xx,
	sHTTP4xxRps:        cHTTP4xx,
	sHTTP5xxRps:        cHTTP5xx,
	sTCPGetRps:         cTCPGet,
	sTCPSetRps:         cTCPSet,
	sTCPMgetRps:        cTCPMget,
	sTCPDelRps:         cTCPDel,
	sTCPOtherRps:       cTCPOther,
	sSlowRps:           cSlow,
	sUnauthorizedRps:   cUnauthorized,
	sForbiddenRps:      cForbidden,
	sFailedLoginsRps:   cFailedLogins,
	sHookErrorsRps:     cHookErrors,
	sKVGetsRps:         cKVGets,
	sKVSetsRps:         cKVSets,
	sKVDeletesRps:      cKVDeletes,
	sKVExpiredRps:      cKVExpired,
	sDocReadsRps:       cDocReads,
	sDocWritesRps:      cDocWrites,
	sDocQueriesRps:     cDocQueries,
	sFlushFailuresRps:  cFlushFailures,
	sForcedFlushesRps:  cForcedFlushes,
	sHookExecutionsRps: cHookExecutions,
	sMongoErrorsRps:    cMongoErrors,
}

func derive(prev, cur *snapshot) Point {
	p := Point{T: cur.at.UnixMilli()}
	for i := range p.V {
		p.V[i] = math.NaN()
	}

	elapsed := cur.at.Sub(prev.at).Seconds()
	if elapsed <= 0 {
		return p
	}

	delta := func(c int) float64 {
		if cur.counters[c] < prev.counters[c] {
			return 0
		}
		return float64(cur.counters[c] - prev.counters[c])
	}

	for s, c := range rateSeries {
		p.V[s] = delta(c) / elapsed
	}

	if cur.logWarnings >= prev.logWarnings {
		p.V[sLogWarningsRps] = float64(cur.logWarnings-prev.logWarnings) / elapsed
	}
	if cur.logErrors >= prev.logErrors {
		p.V[sLogErrorsRps] = float64(cur.logErrors-prev.logErrors) / elapsed
	}

	httpHist := cur.http.sub(prev.http)
	p.V[sHTTPP50] = httpHist.quantile(0.50)
	p.V[sHTTPP95] = httpHist.quantile(0.95)
	p.V[sHTTPP99] = httpHist.quantile(0.99)

	tcpHist := cur.tcp.sub(prev.tcp)
	p.V[sTCPP50] = tcpHist.quantile(0.50)
	p.V[sTCPP95] = tcpHist.quantile(0.95)
	p.V[sTCPP99] = tcpHist.quantile(0.99)

	p.V[sHookP95] = cur.hook.sub(prev.hook).quantile(0.95)

	if hits, misses := delta(cKVHits), delta(cKVMisses); hits+misses > 0 {
		p.V[sKVHitRatio] = hits / (hits + misses)
	}

	if hits, misses := delta(cCacheHits), delta(cCacheMisses); hits+misses > 0 {
		p.V[sCacheHitRatio] = hits / (hits + misses)
	}

	if flushes := delta(cFlushes); flushes > 0 {
		p.V[sFlushMs] = delta(cFlushNanos) / flushes / 1e6
	}

	if cur.gcCycles >= prev.gcCycles {
		p.V[sGCCyclesRps] = float64(cur.gcCycles-prev.gcCycles) / elapsed
	}

	if len(cur.gcPauses) > 0 && len(cur.gcPauses) == len(prev.gcPauses) {
		deltaPauses := make([]uint64, len(cur.gcPauses))
		for i := range cur.gcPauses {
			if cur.gcPauses[i] >= prev.gcPauses[i] {
				deltaPauses[i] = cur.gcPauses[i] - prev.gcPauses[i]
			}
		}
		p.V[sGCPauseP99] = floatHistQuantile(deltaPauses, gcPauseBuckets, 0.99) * 1000
	}

	if cur.cpuSeconds >= 0 && prev.cpuSeconds >= 0 && cur.cpuSeconds >= prev.cpuSeconds && cur.procs > 0 {
		p.V[sCPUPercent] = (cur.cpuSeconds - prev.cpuSeconds) / elapsed / float64(cur.procs) * 100
	}

	for s, v := range cur.gauges {
		p.V[s] = v
	}

	return p
}

type Number float64

func (n Number) MarshalJSON() ([]byte, error) {
	return strconv.AppendFloat(nil, float64(n), 'f', -1, 64), nil
}

func num(f float64) *Number {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return nil
	}

	n := Number(round(f))

	return &n
}

func round(f float64) float64 {
	if math.IsNaN(f) || math.IsInf(f, 0) {
		return f
	}

	return math.Round(f*1000) / 1000
}
