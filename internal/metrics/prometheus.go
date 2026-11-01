package metrics

import (
	"bytes"
	"math"
	"sort"
	"strconv"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/log"
)

type promCounter struct {
	name    string
	help    string
	counter int
	labels  string
}

var promCounters = []promCounter{
	{"elysiandb_http_requests_total", "HTTP requests by status class.", cHTTP2xx, `status_class="2xx"`},
	{"elysiandb_http_requests_total", "", cHTTP3xx, `status_class="3xx"`},
	{"elysiandb_http_requests_total", "", cHTTP4xx, `status_class="4xx"`},
	{"elysiandb_http_requests_total", "", cHTTP5xx, `status_class="5xx"`},
	{"elysiandb_tcp_commands_total", "TCP commands by command.", cTCPGet, `command="GET"`},
	{"elysiandb_tcp_commands_total", "", cTCPSet, `command="SET"`},
	{"elysiandb_tcp_commands_total", "", cTCPMget, `command="MGET"`},
	{"elysiandb_tcp_commands_total", "", cTCPDel, `command="DEL"`},
	{"elysiandb_tcp_commands_total", "", cTCPOther, `command="OTHER"`},
	{"elysiandb_tcp_connections_total", "TCP connections accepted.", cTCPConnections, ""},
	{"elysiandb_slow_requests_total", "HTTP requests and TCP commands slower than log.slowRequestMs.", cSlow, ""},
	{"elysiandb_unauthorized_requests_total", "HTTP requests answered 401.", cUnauthorized, ""},
	{"elysiandb_forbidden_requests_total", "HTTP requests answered 403.", cForbidden, ""},
	{"elysiandb_failed_logins_total", "Failed logins.", cFailedLogins, ""},
	{"elysiandb_hook_executions_total", "Hook script executions.", cHookExecutions, ""},
	{"elysiandb_hook_errors_total", "Hook script errors.", cHookErrors, ""},
	{"elysiandb_kv_operations_total", "KV operations by kind.", cKVGets, `op="get"`},
	{"elysiandb_kv_operations_total", "", cKVSets, `op="set"`},
	{"elysiandb_kv_operations_total", "", cKVDeletes, `op="delete"`},
	{"elysiandb_kv_lookups_total", "KV reads by result.", cKVHits, `result="hit"`},
	{"elysiandb_kv_lookups_total", "", cKVMisses, `result="miss"`},
	{"elysiandb_kv_expired_keys_total", "Keys removed by expiration.", cKVExpired, ""},
	{"elysiandb_document_operations_total", "REST API document operations by kind.", cDocReads, `op="read"`},
	{"elysiandb_document_operations_total", "", cDocWrites, `op="write"`},
	{"elysiandb_document_operations_total", "", cDocQueries, `op="query"`},
	{"elysiandb_api_cache_lookups_total", "API cache lookups by result.", cCacheHits, `result="hit"`},
	{"elysiandb_api_cache_lookups_total", "", cCacheMisses, `result="miss"`},
	{"elysiandb_flushes_total", "Flushes to disk that wrote data.", cFlushes, ""},
	{"elysiandb_flush_failures_total", "Failed flushes to disk.", cFlushFailures, ""},
	{"elysiandb_forced_flushes_total", "Flushes forced by the recovery log size limit.", cForcedFlushes, ""},
	{"elysiandb_recovery_write_failures_total", "Failed recovery log writes.", cRecoveryFailures, ""},
	{"elysiandb_mongodb_errors_total", "Failed MongoDB operations.", cMongoErrors, ""},
}

var promGauges = []struct {
	name   string
	help   string
	series int
}{
	{"elysiandb_kv_keys", "Keys in the KV store.", sKVKeys},
	{"elysiandb_kv_keys_with_ttl", "Keys with a TTL.", sKVKeysTTL},
	{"elysiandb_index_backlog", "Fields waiting for a lazy index rebuild.", sIndexBacklog},
	{"elysiandb_api_cache_entries", "API cache entries.", sCacheEntries},
	{"elysiandb_recovery_log_bytes_json", "Size of the JSON recovery log.", sRecoveryJSONBytes},
	{"elysiandb_recovery_log_bytes_store", "Size of the store recovery log.", sRecoveryStoreBytes},
	{"elysiandb_data_folder_bytes", "Size of the data folder.", sDiskBytes},
	{"elysiandb_heap_bytes", "Go heap in use.", sHeapBytes},
	{"elysiandb_memory_bytes", "Memory mapped by the Go runtime.", sTotalBytes},
	{"elysiandb_goroutines", "Goroutines.", sGoroutines},
	{"elysiandb_open_fds", "Open file descriptors.", sOpenFDs},
	{"elysiandb_http_connections", "Open HTTP connections.", sHTTPConns},
	{"elysiandb_tcp_connections", "Open TCP connections.", sTCPConns},
	{"elysiandb_admin_sessions", "Active Admin UI sessions.", sAdminSessions},
	{"elysiandb_open_transactions", "Open transactions.", sOpenTransactions},
	{"elysiandb_oldest_transaction_seconds", "Age of the oldest open transaction.", sOldestTransaction},
	{"elysiandb_mongodb_ping_seconds", "MongoDB ping latency.", sMongoPingMs},
	{"elysiandb_mongodb_pool_in_use", "MongoDB pool connections in use.", sMongoPoolInUse},
}

func Prometheus() []byte {
	var b bytes.Buffer

	described := map[string]bool{}
	for _, c := range promCounters {
		if !described[c.name] {
			described[c.name] = true
			writeHeader(&b, c.name, c.help, "counter")
		}
		writeSample(&b, c.name, c.labels, float64(counters[c.counter].Load()))
	}

	warnings, errorsCount := log.Counts()
	writeHeader(&b, "elysiandb_log_lines_total", "Warning and error log lines.", "counter")
	writeSample(&b, "elysiandb_log_lines_total", `level="warn"`, float64(warnings))
	writeSample(&b, "elysiandb_log_lines_total", `level="error"`, float64(errorsCount))

	state.mu.RLock()
	var latest *snapshot
	if len(state.recent) > 0 {
		latest = state.recent[len(state.recent)-1]
	}
	counts := state.slow.entityCounts
	state.mu.RUnlock()

	if latest != nil {
		for _, g := range promGauges {
			v, ok := latest.gauges[g.series]
			if !ok || math.IsNaN(v) {
				continue
			}
			if g.series == sMongoPingMs {
				v /= 1000
			}
			writeHeader(&b, g.name, g.help, "gauge")
			writeSample(&b, g.name, "", v)
		}
	}

	names := make([]string, 0, len(counts))
	for name := range counts {
		names = append(names, name)
	}
	sort.Strings(names)
	writeHeader(&b, "elysiandb_entities", "Documents per entity type.", "gauge")
	for _, name := range names {
		writeSample(&b, "elysiandb_entities", `entity_type="`+escapeLabel(name)+`"`, float64(counts[name]))
	}

	writeHeader(&b, "elysiandb_uptime_seconds", "Seconds since the server started.", "gauge")
	writeSample(&b, "elysiandb_uptime_seconds", "", time.Since(startedAt).Seconds())

	writeHistogram(&b, "elysiandb_http_request_duration_seconds", "HTTP request duration.", &httpLatency)
	writeHistogram(&b, "elysiandb_tcp_command_duration_seconds", "TCP command duration.", &tcpLatency)
	writeHistogram(&b, "elysiandb_hook_duration_seconds", "Hook script duration.", &hookLatency)

	return b.Bytes()
}

func writeHeader(b *bytes.Buffer, name, help, kind string) {
	if help != "" {
		b.WriteString("# HELP " + name + " " + help + "\n")
	}
	b.WriteString("# TYPE " + name + " " + kind + "\n")
}

func writeSample(b *bytes.Buffer, name, labels string, v float64) {
	b.WriteString(name)
	if labels != "" {
		b.WriteString("{" + labels + "}")
	}
	b.WriteByte(' ')
	b.WriteString(strconv.FormatFloat(v, 'g', -1, 64))
	b.WriteByte('\n')
}

func writeHistogram(b *bytes.Buffer, name, help string, source *Histogram) {
	writeHeader(b, name, help, "histogram")

	h := source.snapshot()
	var cumulative uint64
	for i, bound := range bucketBoundsMs {
		cumulative += h[i]
		writeSample(b, name+"_bucket", `le="`+strconv.FormatFloat(bound/1000, 'g', -1, 64)+`"`, float64(cumulative))
	}

	cumulative += h[numBuckets-1]
	writeSample(b, name+"_bucket", `le="+Inf"`, float64(cumulative))
	writeSample(b, name+"_sum", "", source.sumSeconds())
	writeSample(b, name+"_count", "", float64(cumulative))
}

func escapeLabel(s string) string {
	return strings.NewReplacer(`\`, `\\`, `"`, `\"`, "\n", `\n`).Replace(s)
}
