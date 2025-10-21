package e2e

import (
	"encoding/json"
	"testing"
	"time"

	"github.com/valyala/fasthttp"
)

type getEntry struct {
	Key   string  `json:"key"`
	Value *string `json:"value"`
}

type multiGetEntry struct {
	Key   string  `json:"key"`
	Value *string `json:"value"`
}

func toMap(arr []multiGetEntry) map[string]*string {
	m := make(map[string]*string, len(arr))
	for _, e := range arr {
		m[e.Key] = e.Value
	}
	return m
}

func mustBodyJSON[T any](t *testing.T, b []byte, out *T) {
	t.Helper()
	if err := json.Unmarshal(b, out); err != nil {
		t.Fatalf("invalid JSON: %v (body=%q)", err, b)
	}
}

func TestMultiplePUTAndMGET_WithWildcard(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	for k, v := range map[string]string{
		"foo": "bar",
		"baz": "bat",
	} {
		req.Reset()
		resp.Reset()
		req.Header.SetMethod(fasthttp.MethodPut)
		req.SetRequestURI("http://test/kv/" + k)
		req.SetBodyString(v)
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("PUT %s failed: %v", k, err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
			t.Fatalf("expected 204, got %d", sc)
		}
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/mget?keys=foo,baz,qux,ba*")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	var arr []multiGetEntry
	mustBodyJSON(t, resp.Body(), &arr)
	got := toMap(arr)

	if len(got) != 3 {
		t.Fatalf("expected 3 distinct results, got %d (%v)", len(got), arr)
	}
	if got["foo"] == nil || *got["foo"] != "bar" {
		t.Fatalf("foo mismatch: %+v", got["foo"])
	}
	if got["baz"] == nil || *got["baz"] != "bat" {
		t.Fatalf("baz mismatch: %+v", got["baz"])
	}
	if v, ok := got["qux"]; !ok || v != nil {
		t.Fatalf("qux should be present with nil value, got: %+v", v)
	}
}

func TestPUTAndGET(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodPut)
	req.SetRequestURI("http://test/kv/foo")
	req.SetBodyString("bar")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("PUT failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/foo")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	var e getEntry
	mustBodyJSON(t, resp.Body(), &e)
	if e.Key != "foo" || e.Value == nil || *e.Value != "bar" {
		t.Fatalf("unexpected getEntry: %+v", e)
	}
}

func TestPUTWithTTLExpires(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodPut)
	req.SetRequestURI("http://test/kv/ephemeral?ttl=1")
	req.SetBodyString("tmp")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("put failed: %v", err)
	}
	if resp.StatusCode() != 204 {
		t.Fatalf("expected 204, got %d", resp.StatusCode())
	}

	time.Sleep(1100 * time.Millisecond)

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/ephemeral")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("get failed: %v", err)
	}
	if resp.StatusCode() != 404 {
		t.Fatalf("expected 404 after TTL, got %d", resp.StatusCode())
	}
	var e getEntry
	mustBodyJSON(t, resp.Body(), &e)
	if e.Key != "ephemeral" || e.Value != nil {
		t.Fatalf("unexpected 404 body: %+v", e)
	}
}

func TestGETNotFound(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/notfound")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET failed: %v", err)
	}
	if resp.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404 for missing key, got %d", resp.StatusCode())
	}
	var e getEntry
	mustBodyJSON(t, resp.Body(), &e)
	if e.Key != "notfound" || e.Value != nil {
		t.Fatalf("unexpected 404 body: %+v", e)
	}
}

func TestPutAndSave(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodPut)
	req.SetRequestURI("http://test/kv/foo")
	req.SetBodyString("bar")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("PUT failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodPost)
	req.SetRequestURI("http://test/save")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("POST failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/foo")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}
	var e getEntry
	mustBodyJSON(t, resp.Body(), &e)
	if e.Key != "foo" || e.Value == nil || *e.Value != "bar" {
		t.Fatalf("unexpected getEntry: %+v", e)
	}
}

func TestPutAndReset(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodPut)
	req.SetRequestURI("http://test/kv/foo2")
	req.SetBodyString("bar")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("PUT failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodPost)
	req.SetRequestURI("http://test/reset")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("POST failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/foo2")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET failed: %v", err)
	}
	if resp.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404 for missing key, got %d", resp.StatusCode())
	}
	var e getEntry
	mustBodyJSON(t, resp.Body(), &e)
	if e.Key != "foo2" || e.Value != nil {
		t.Fatalf("unexpected 404 body: %+v", e)
	}
}

func TestStats(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	type StatsDTO struct {
		KeysCount           string `json:"keys_count"`
		ExpirationKeysCount string `json:"expiration_keys_count"`
		UptimeSeconds       string `json:"uptime_seconds"`
		TotalRequests       string `json:"total_requests"`
		Hits                string `json:"hits"`
		Misses              string `json:"misses"`
	}

	{
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodPut)
		req.SetRequestURI("http://test/kv/foo")
		req.SetBodyString("bar")
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("PUT failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
			t.Fatalf("expected 204, got %d", sc)
		}
	}

	{
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodGet)
		req.SetRequestURI("http://test/kv/foo")
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("GET hit failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
			t.Fatalf("expected 200, got %d", sc)
		}
	}

	{
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodGet)
		req.SetRequestURI("http://test/kv/missing")
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("GET miss failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusNotFound {
			t.Fatalf("expected 404, got %d", sc)
		}
	}

	atoi := func(s string) uint64 {
		var n uint64
		for i := 0; i < len(s); i++ {
			c := s[i]
			if c < '0' || c > '9' {
				t.Fatalf("expected numeric string, got %q", s)
			}
			n = n*10 + uint64(c-'0')
		}
		return n
	}

	getStats := func() StatsDTO {
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodGet)
		req.SetRequestURI("http://test/stats")

		if err := client.Do(req, resp); err != nil {
			t.Fatalf("GET /stats failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
			t.Fatalf("expected 200 from /stats, got %d", sc)
		}

		var dto StatsDTO
		mustBodyJSON(t, resp.Body(), &dto)

		_ = atoi(dto.KeysCount)
		_ = atoi(dto.ExpirationKeysCount)
		_ = atoi(dto.UptimeSeconds)
		_ = atoi(dto.TotalRequests)
		_ = atoi(dto.Hits)
		_ = atoi(dto.Misses)

		return dto
	}

	s1 := getStats()

	if kc := atoi(s1.KeysCount); kc != 1 {
		t.Fatalf("keys_count = %d, want 1", kc)
	}
	if ec := atoi(s1.ExpirationKeysCount); ec != 0 {
		t.Fatalf("expiration_keys_count = %d, want 0", ec)
	}
	_ = atoi(s1.UptimeSeconds)

	h1 := atoi(s1.Hits)
	m1 := atoi(s1.Misses)
	tr1 := atoi(s1.TotalRequests)

	if h1 < 1 {
		t.Fatalf("hits = %d, want >= 1 (after GET on existing key)", h1)
	}
	if m1 < 1 {
		t.Fatalf("misses = %d, want >= 1 (after GET on missing key)", m1)
	}
	if tr1 < h1+m1 {
		t.Fatalf("total_requests = %d, want >= hits+misses (%d)", tr1, h1+m1)
	}
}

func TestGETWildcard_Matches(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	for k, v := range map[string]string{
		"user:1": "alice",
		"user:2": "bob",
		"prod:1": "sku1",
	} {
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		req.Header.SetMethod(fasthttp.MethodPut)
		req.SetRequestURI("http://test/kv/" + k)
		req.SetBodyString(v)
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("PUT %s failed: %v", k, err)
		}
		fasthttp.ReleaseRequest(req)
		fasthttp.ReleaseResponse(resp)
	}

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/user:%2A")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET wildcard failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	var arr []multiGetEntry
	mustBodyJSON(t, resp.Body(), &arr)
	got := toMap(arr)

	if len(got) != 2 {
		t.Fatalf("expected 2 matches, got %d (%v)", len(got), arr)
	}
	if got["user:1"] == nil || *got["user:1"] != "alice" {
		t.Fatalf("user:1 mismatch: %+v", got["user:1"])
	}
	if got["user:2"] == nil || *got["user:2"] != "bob" {
		t.Fatalf("user:2 mismatch: %+v", got["user:2"])
	}
}

func TestGETWildcard_NoMatches_ReturnsPatternWithNil(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/unknown:%2A")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET wildcard failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	var arr []multiGetEntry
	mustBodyJSON(t, resp.Body(), &arr)

	if len(arr) != 1 || arr[0].Key != "unknown:*" || arr[0].Value != nil {
		t.Fatalf("expected single {key=pattern,value=nil}, got: %+v", arr)
	}
}

func TestDELETEWildcard_HTTP(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	seed := map[string]string{
		"user:1": "alice",
		"user:2": "bob",
		"prod:1": "sku1",
	}
	for k, v := range seed {
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		req.Header.SetMethod(fasthttp.MethodPut)
		req.SetRequestURI("http://test/kv/" + k)
		req.SetBodyString(v)
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("PUT %s failed: %v", k, err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
			t.Fatalf("PUT %s expected 204, got %d", k, sc)
		}
		fasthttp.ReleaseRequest(req)
		fasthttp.ReleaseResponse(resp)
	}

	{
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodDelete)
		req.SetRequestURI("http://test/kv/user:%2A")
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("DELETE wildcard failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
			t.Fatalf("DELETE wildcard expected 204, got %d", sc)
		}
	}

	{
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodGet)
		req.SetRequestURI("http://test/kv/user:%2A")
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("GET wildcard after delete failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
			t.Fatalf("expected 200, got %d", sc)
		}
		var arr []multiGetEntry
		mustBodyJSON(t, resp.Body(), &arr)
		if len(arr) != 1 || arr[0].Key != "user:*" || arr[0].Value != nil {
			t.Fatalf("expected single {key=pattern,value=nil}, got: %+v", arr)
		}
	}

	{
		req := fasthttp.AcquireRequest()
		resp := fasthttp.AcquireResponse()
		defer fasthttp.ReleaseRequest(req)
		defer fasthttp.ReleaseResponse(resp)

		req.Header.SetMethod(fasthttp.MethodGet)
		req.SetRequestURI("http://test/kv/mget?keys=user:1,user:2,prod:1")
		if err := client.Do(req, resp); err != nil {
			t.Fatalf("MGET after delete failed: %v", err)
		}
		if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
			t.Fatalf("expected 200, got %d", sc)
		}

		var arr []multiGetEntry
		mustBodyJSON(t, resp.Body(), &arr)
		got := toMap(arr)

		if v, ok := got["user:1"]; !ok || v != nil {
			t.Fatalf("user:1 should be nil after delete, got: %+v", v)
		}
		if v, ok := got["user:2"]; !ok || v != nil {
			t.Fatalf("user:2 should be nil after delete, got: %+v", v)
		}
		if got["prod:1"] == nil || *got["prod:1"] != "sku1" {
			t.Fatalf("prod:1 should still exist with value sku1, got: %+v", got["prod:1"])
		}
	}
}

func TestDELETESingle_HTTP(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	req.Header.SetMethod(fasthttp.MethodPut)
	req.SetRequestURI("http://test/kv/tmpdel")
	req.SetBodyString("gone")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("PUT failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodDelete)
	req.SetRequestURI("http://test/kv/tmpdel")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("DELETE failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusNoContent {
		t.Fatalf("expected 204, got %d", sc)
	}

	req.Reset()
	resp.Reset()
	req.Header.SetMethod(fasthttp.MethodGet)
	req.SetRequestURI("http://test/kv/tmpdel")
	if err := client.Do(req, resp); err != nil {
		t.Fatalf("GET failed: %v", err)
	}
	if resp.StatusCode() != fasthttp.StatusNotFound {
		t.Fatalf("expected 404 for deleted key, got %d", resp.StatusCode())
	}
	var e getEntry
	mustBodyJSON(t, resp.Body(), &e)
	if e.Key != "tmpdel" || e.Value != nil {
		t.Fatalf("unexpected 404 body: %+v", e)
	}
}

func TestCreateListOfEntities(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	req := fasthttp.AcquireRequest()
	resp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(req)
	defer fasthttp.ReleaseResponse(resp)

	entities := []map[string]interface{}{
		{"name": "Alice", "age": 30},
		{"name": "Bob", "age": 25},
	}

	body, _ := json.Marshal(entities)
	req.Header.SetMethod(fasthttp.MethodPost)
	req.SetRequestURI("http://test/api/users")
	req.SetBody(body)

	if err := client.Do(req, resp); err != nil {
		t.Fatalf("POST list failed: %v", err)
	}
	if sc := resp.StatusCode(); sc != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", sc)
	}

	var created []map[string]interface{}
	mustBodyJSON(t, resp.Body(), &created)
	if len(created) != 2 {
		t.Fatalf("expected 2 entities, got %d", len(created))
	}
	for _, e := range created {
		if _, ok := e["id"].(string); !ok {
			t.Fatalf("expected id field in %+v", e)
		}
	}
}

func TestUpdateListOfEntities(t *testing.T) {
	client, stop := startTestServer(t)
	defer stop()

	createReq := fasthttp.AcquireRequest()
	createResp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(createReq)
	defer fasthttp.ReleaseResponse(createResp)

	entities := []map[string]interface{}{
		{"id": "u1", "name": "Alice", "age": 30},
		{"id": "u2", "name": "Bob", "age": 25},
	}
	createBody, _ := json.Marshal(entities)
	createReq.Header.SetMethod(fasthttp.MethodPost)
	createReq.SetRequestURI("http://test/api/users")
	createReq.SetBody(createBody)

	if err := client.Do(createReq, createResp); err != nil {
		t.Fatalf("POST create list failed: %v", err)
	}
	if createResp.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", createResp.StatusCode())
	}

	updates := []map[string]interface{}{
		{"id": "u1", "age": 35},
		{"id": "u2", "name": "Bobby"},
	}
	updateBody, _ := json.Marshal(updates)
	updateReq := fasthttp.AcquireRequest()
	updateResp := fasthttp.AcquireResponse()
	defer fasthttp.ReleaseRequest(updateReq)
	defer fasthttp.ReleaseResponse(updateResp)

	updateReq.Header.SetMethod(fasthttp.MethodPut)
	updateReq.SetRequestURI("http://test/api/users")
	updateReq.SetBody(updateBody)

	if err := client.Do(updateReq, updateResp); err != nil {
		t.Fatalf("PUT update list failed: %v", err)
	}
	if updateResp.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200, got %d", updateResp.StatusCode())
	}

	var updated []map[string]interface{}
	mustBodyJSON(t, updateResp.Body(), &updated)
	if len(updated) != 2 {
		t.Fatalf("expected 2 updated entities, got %d", len(updated))
	}

	for _, e := range updated {
		if e["id"] == "u1" && e["age"].(float64) != 35 {
			t.Fatalf("expected age=35 for u1, got %+v", e)
		}
		if e["id"] == "u2" && e["name"].(string) != "Bobby" {
			t.Fatalf("expected name=Bobby for u2, got %+v", e)
		}
	}
}
