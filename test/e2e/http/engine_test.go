package e2e

import (
	"encoding/json"
	"fmt"
	"strings"
	"sync"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/valyala/fasthttp"
)

func TestRequestDeadlineAnswers503(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) {
		cfg.Api.Schema.Strict = false
		cfg.Api.Query.TimeoutMs = 1
		cfg.Api.Query.Workers = 1
	})

	list := make([]map[string]any, 0, 20000)
	for i := 0; i < 20000; i++ {
		list = append(list, map[string]any{"id": fmt.Sprintf("d%05d", i), "note": strings.Repeat("n", 40), "k": i % 7})
	}
	body, _ := json.Marshal(list)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/big", string(body))

	expired := false
	for i := 0; i < 5 && !expired; i++ {
		resp := s.do("GET", "/api/big?filter%5Bnote%5D%5Bneq%5D=x&search=nnn", "")
		if resp.status == fasthttp.StatusServiceUnavailable {
			expired = true
			if string(resp.body) != `{"error":"request deadline exceeded"}` {
				t.Fatalf("unexpected body %s", resp.body)
			}
		}
	}

	if !expired {
		t.Fatalf("a scan longer than api.query.timeoutMs should answer 503")
	}

	page := s.mustDo(fasthttp.StatusOK, "GET", "/api/big?limit=3&offset=100", "").ids(t)
	if strings.Join(page, ",") != "d00100,d00101,d00102" {
		t.Fatalf("index-only plans are not slowed down, got %v", page)
	}
}

func TestCachedListsFollowIncludedDocuments(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/writer", `{"id":"w1","name":"Herbert"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/novel", `{"id":"n1","title":"Dune","writer":{"@entity":"writer","id":"w1"}}`)

	first := s.mustDo(fasthttp.StatusOK, "GET", "/api/novel?includes=writer", "")
	if !strings.Contains(string(first.body), "Herbert") {
		t.Fatalf("expected the included writer, got %s", first.body)
	}
	if hit := s.mustDo(fasthttp.StatusOK, "GET", "/api/novel?includes=writer", ""); hit.header("X-Elysian-Cache") != "HIT" {
		t.Fatalf("the second read should come from the cache")
	}

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/writer/w1", `{"name":"Frank Herbert"}`)

	after := s.mustDo(fasthttp.StatusOK, "GET", "/api/novel?includes=writer", "")
	if after.header("X-Elysian-Cache") != "MISS" || !strings.Contains(string(after.body), "Frank Herbert") {
		t.Fatalf("a cached response that includes a changed document is invalidated, got %s %s", after.header("X-Elysian-Cache"), after.body)
	}
}

func TestStatsCountEveryRequest(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) { cfg.Api.Schema.Strict = false })

	before := s.mustDo(fasthttp.StatusOK, "GET", "/stats", "").object(t)
	for i := 0; i < 5; i++ {
		s.mustDo(fasthttp.StatusOK, "GET", "/api/nothing", "")
	}
	s.mustDo(fasthttp.StatusNoContent, "PUT", "/kv/k", "v")
	after := s.mustDo(fasthttp.StatusOK, "GET", "/stats", "").object(t)

	var b, a int
	fmt.Sscanf(before["total_requests"].(string), "%d", &b)
	fmt.Sscanf(after["total_requests"].(string), "%d", &a)
	if a-b != 7 {
		t.Fatalf("expected 7 more requests (5 lists, 1 KV write, 1 stats), got %d", a-b)
	}

	if after["keys_count"] != "1" {
		t.Fatalf("keys_count counts the KV keys only, got %v", after["keys_count"])
	}
}

func TestLargeListsAreStreamedByteForByte(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)

	list := make([]map[string]any, 0, 4000)
	for i := 0; i < 4000; i++ {
		list = append(list, map[string]any{"id": fmt.Sprintf("s%04d", i), "text": "<&>" + strings.Repeat("t", 260), "n": float64(i) / 3})
	}
	body, _ := json.Marshal(list)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/stream", string(body))

	resp := s.mustDo(fasthttp.StatusOK, "GET", "/api/stream", "")
	if len(resp.body) < 1<<20 {
		t.Fatalf("the response should be larger than the streaming threshold, got %d bytes", len(resp.body))
	}

	var decoded []map[string]any
	if err := json.Unmarshal(resp.body, &decoded); err != nil {
		t.Fatal(err)
	}

	again, _ := json.Marshal(decoded)
	if string(again) != string(resp.body) {
		t.Fatalf("a streamed list must be byte for byte what json.Marshal gives")
	}

	if len(decoded) != 4000 || decoded[3999]["id"] != "s3999" {
		t.Fatalf("unexpected streamed list of %d documents", len(decoded))
	}
}

func TestConcurrentWritersAndReadersOnOneEntity(t *testing.T) {
	s := startAdminSession(t, withoutStrictSchema)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/par", `{"id":"seed","rank":-1}`)
	s.mustDo(fasthttp.StatusNoContent, "DELETE", "/api/par/seed", "")

	var wg sync.WaitGroup
	for g := 0; g < 8; g++ {
		wg.Add(1)
		go func(g int) {
			defer wg.Done()
			batch := make([]map[string]any, 0, 50)
			for i := 0; i < 50; i++ {
				batch = append(batch, map[string]any{"id": fmt.Sprintf("g%d-%02d", g, i), "rank": float64(g*100 + i)})
			}
			body, _ := json.Marshal(batch)
			s.mustDo(fasthttp.StatusOK, "POST", "/api/par", string(body))
		}(g)

		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := 0; i < 10; i++ {
				s.mustDo(fasthttp.StatusOK, "GET", "/api/par?sort%5Brank%5D=desc&limit=5", "")
				time.Sleep(time.Millisecond)
			}
		}()
	}
	wg.Wait()

	count := s.mustDo(fasthttp.StatusOK, "GET", "/api/par/count", "").object(t)
	if count["count"] != 400.0 {
		t.Fatalf("expected 400 documents, got %v", count["count"])
	}

	if sorted := s.mustDo(fasthttp.StatusOK, "GET", "/api/par?sort%5Brank%5D=asc", "").ids(t); len(sorted) != 400 || sorted[0] != "g0-00" {
		t.Fatalf("the sorted list holds every document, got %d", len(sorted))
	}
}
