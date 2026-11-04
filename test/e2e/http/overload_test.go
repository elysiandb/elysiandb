package e2e

import (
	"encoding/json"
	"fmt"
	"net"
	"regexp"
	"strings"
	"sync/atomic"
	"testing"
	"time"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/boot"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

const busyDocs = 5000

const busyQuery = "/api/busy?filter%5Bnote%5D%5Bneq%5D=x&search=nnn&countOnly=true"

func seedBusy(t *testing.T, s *apiServer) {
	t.Helper()

	for start := 0; start < busyDocs; start += 500 {
		list := make([]map[string]any, 0, 500)
		for i := start; i < start+500; i++ {
			list = append(list, map[string]any{"id": fmt.Sprintf("b%05d", i), "note": strings.Repeat("n", 4000), "tags": []any{"n", "m"}})
		}
		body, _ := json.Marshal(list)
		s.mustDo(fasthttp.StatusOK, "POST", "/api/busy", string(body))
	}
}

func holdHeavyLane(t *testing.T) func() {
	t.Helper()

	inside := make(chan struct{})
	release := make(chan struct{})
	done := make(chan struct{})
	var calls atomic.Int64
	guard := &api_storage.Guard{Abandoned: func() bool {
		if calls.Add(1) < 15 {
			time.Sleep(time.Millisecond)
			return false
		}
		if calls.Load() == 15 {
			close(inside)
			<-release
		}
		return false
	}}

	go func() {
		defer close(done)
		api_storage.Plan(api_storage.ListQuery{
			Entity:    "busy",
			Filters:   map[string]map[string]string{"note": {"neq": "x"}},
			Search:    "nnn",
			CountOnly: true,
			Guard:     guard,
		})
	}()

	select {
	case <-inside:
	case <-time.After(5 * time.Second):
		t.Fatalf("the holder never entered the heavy lane")
	}

	var freed atomic.Bool
	free := func() {
		if freed.CompareAndSwap(false, true) {
			close(release)
			<-done
		}
	}
	t.Cleanup(free)

	return free
}

func (s *apiServer) async(method, path string) chan apiResponse {
	out := make(chan apiResponse, 1)
	go func() {
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(method)
		req.SetRequestURI("http://test" + path)
		req.Header.SetCookie(security.SessionCookieName, s.cookie)
		if err := s.client.Do(req, resp); err != nil {
			out <- apiResponse{body: []byte(err.Error())}
			return
		}
		out <- apiResponse{status: resp.StatusCode(), body: append([]byte(nil), resp.Body()...)}
	}()

	return out
}

func waitForHeavyQueue(t *testing.T, n int) {
	t.Helper()

	deadline := time.Now().Add(5 * time.Second)
	for api_storage.HeavyWaiting() != n {
		if time.Now().After(deadline) {
			t.Fatalf("expected %d queued heavy queries, got %d", n, api_storage.HeavyWaiting())
		}
		time.Sleep(time.Millisecond)
	}
}

func TestFullHeavyQueueAnswers503(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) {
		withoutStrictSchema(cfg)
		cfg.Api.Query.Workers = 1
		cfg.Api.Query.MaxQueued = 1
		cfg.Api.Query.QueueTimeoutMs = -1
	})
	seedBusy(t, s)
	free := holdHeavyLane(t)

	queued := s.async("GET", busyQuery)
	waitForHeavyQueue(t, 1)

	busy := s.do("GET", busyQuery, "")
	if busy.status != fasthttp.StatusServiceUnavailable || string(busy.body) != `{"error":"server busy"}` || busy.header("Retry-After") != "1" {
		t.Fatalf("a request beyond the queue answers 503 server busy, got %d %s retry=%q", busy.status, busy.body, busy.header("Retry-After"))
	}

	free()
	select {
	case r := <-queued:
		if r.status != fasthttp.StatusOK || string(r.body) != fmt.Sprintf(`{"count":%d}`, busyDocs) {
			t.Fatalf("the queued request is served once the lane is free, got %d %s", r.status, r.body)
		}
	case <-time.After(10 * time.Second):
		t.Fatalf("the queued request never finished")
	}

	s.mustDo(fasthttp.StatusOK, "GET", busyQuery+"&limit=1", "")

	if !regexp.MustCompile(`(?m)^elysiandb_heavy_rejected_total [1-9]`).Match(metrics.Prometheus()) {
		t.Fatalf("the rejection is counted in the Prometheus export")
	}

	metrics.SetSources(boot.MonitoringSources())
	t.Cleanup(func() { metrics.SetSources(metrics.Sources{}) })

	var details struct {
		Indexes []struct {
			Entity string `json:"entity"`
			Index  string `json:"index"`
			State  string `json:"state"`
		} `json:"indexes"`
	}
	if err := json.Unmarshal(s.mustDo(fasthttp.StatusOK, "GET", "/monitoring/details", "").body, &details); err != nil {
		t.Fatal(err)
	}

	states := map[string]string{}
	for _, idx := range details.Indexes {
		if idx.Entity == "busy" {
			states[idx.Index] = idx.State
		}
	}
	if states["f:note"] == "" || states["search"] == "" {
		t.Fatalf("the monitoring details list the indexes of busy, got %v", states)
	}
}

func TestBlockedHookQueryStopsAtTheHookTimeout(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) {
		withoutStrictSchema(cfg)
		cfg.Api.Query.Workers = 1
		cfg.Api.Query.QueueTimeoutMs = -1
		cfg.Api.Hooks.TimeoutMs = 300
	})
	seedBusy(t, s)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/month", `{"id":"m1","label":"january"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/month", `{"name":"total","event":"post_read","language":"javascript","enabled":true,"script":"function postRead(ctx){ ctx.entity.purchases = ctx.query(\"busy\", {\"tags\": {\"all\": \"n,m\"}}).length }"}`)

	free := holdHeavyLane(t)

	start := time.Now()
	resp := s.mustDo(fasthttp.StatusOK, "GET", "/api/month", "")
	took := time.Since(start)
	if took > 2*time.Second {
		t.Fatalf("a ctx.query blocked behind the heavy lane stops at the hook timeout, took %s", took)
	}
	if strings.Contains(string(resp.body), "purchases") {
		t.Fatalf("the stopped hook leaves the document as it was, got %s after %s", resp.body, took)
	}

	var lastError string
	for _, h := range metrics.CurrentDetails().Hooks {
		if h.Entity == "month" {
			lastError = h.LastError
		}
	}
	if !strings.Contains(lastError, "hook script stopped after 300ms") {
		t.Fatalf("the hook error names the timeout, got %q", lastError)
	}

	free()
	relaxed := *globals.GetConfig()
	relaxed.Api.Hooks.TimeoutMs = 60000
	globals.SetConfig(&relaxed)

	after := s.mustDo(fasthttp.StatusOK, "GET", "/api/month", "").list(t)
	if after[0]["purchases"] != float64(busyDocs) {
		t.Fatalf("once the lane is free the hook query is served, got %v", after)
	}
}

func hookExecutions(entity string) uint64 {
	var n uint64
	for _, h := range metrics.CurrentDetails().Hooks {
		if h.Entity == entity {
			n += h.Executions
		}
	}

	return n
}

func TestAbandonedRequestStopsItsWork(t *testing.T) {
	s, addr := startTCPAdminSession(t, withoutStrictSchema)

	const docs = 300
	list := make([]map[string]any, 0, docs)
	for i := 0; i < docs; i++ {
		list = append(list, map[string]any{"id": fmt.Sprintf("s%03d", i), "n": i})
	}
	body, _ := json.Marshal(list)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/spin", string(body))
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/spin", `{"name":"spin","event":"pre_read","language":"javascript","enabled":true,"script":"function preRead(ctx){ var t = Date.now(); while (Date.now() - t < 10) {} }"}`)

	conn, err := net.Dial("tcp", addr)
	if err != nil {
		t.Fatal(err)
	}
	fmt.Fprintf(conn, "GET /api/spin?filter%%5Bn%%5D%%5Bgte%%5D=0 HTTP/1.1\r\nHost: test\r\nCookie: %s=%s\r\n\r\n", security.SessionCookieName, s.cookie)

	time.Sleep(300 * time.Millisecond)
	if hookExecutions("spin") == 0 {
		t.Fatalf("the request should be running its hooks")
	}
	_ = conn.Close()

	time.Sleep(200 * time.Millisecond)
	stopped := hookExecutions("spin")
	time.Sleep(700 * time.Millisecond)
	if later := hookExecutions("spin"); later != stopped || later >= docs {
		t.Fatalf("the hooks of an abandoned request stop, %d then %d executions of %d", stopped, later, docs)
	}

	page := s.mustDo(fasthttp.StatusOK, "GET", "/api/spin?filter%5Bn%5D%5Blt%5D=2", "").ids(t)
	if strings.Join(page, ",") != "s000,s001" {
		t.Fatalf("a client that waits is served as before, got %v", page)
	}
}
