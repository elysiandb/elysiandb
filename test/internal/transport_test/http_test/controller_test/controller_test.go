package controller_test

import (
	"encoding/json"
	"strings"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/http/controller"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	cfg.Stats.Enabled = false
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.ResetStore()
}

func newCtx(method, path string, body string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(path)
	req.Header.SetMethod(method)
	if body != "" {
		req.SetBodyString(body)
	}
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	return ctx
}

func TestHealthController(t *testing.T) {
	setup(t)

	ctx := newCtx("GET", "/health", "")
	controller.HealthController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatal("health failed")
	}
}

func TestPutAndGet(t *testing.T) {
	setup(t)

	ctx := newCtx("PUT", "/kv/foo", "bar")
	ctx.SetUserValue("key", "foo")
	controller.PutKeyController(ctx)

	if ctx.Response.StatusCode() != 204 {
		t.Fatal("put failed")
	}

	ctx = newCtx("GET", "/kv/foo", "")
	ctx.SetUserValue("key", "foo")
	controller.GetKeyController(ctx)

	if !strings.Contains(string(ctx.Response.Body()), `"value":"bar"`) {
		t.Fatal("get failed")
	}
}

func TestDeleteKeyController(t *testing.T) {
	setup(t)

	ctx := newCtx("PUT", "/kv/a", "1")
	ctx.SetUserValue("key", "a")
	controller.PutKeyController(ctx)

	ctx = newCtx("DELETE", "/kv/a", "")
	ctx.SetUserValue("key", "a")
	controller.DeleteKeyController(ctx)

	if ctx.Response.StatusCode() != 204 {
		t.Fatal("delete failed")
	}
}

func TestResetController(t *testing.T) {
	setup(t)

	ctx := newCtx("PUT", "/kv/x", "y")
	ctx.SetUserValue("key", "x")
	controller.PutKeyController(ctx)

	ctx = newCtx("POST", "/reset", "")
	controller.ResetController(ctx)

	ctx = newCtx("GET", "/kv/x", "")
	ctx.SetUserValue("key", "x")
	controller.GetKeyController(ctx)

	if !strings.Contains(string(ctx.Response.Body()), `"value":null`) {
		t.Fatal("reset failed")
	}
}

func TestSaveController(t *testing.T) {
	setup(t)

	ctx := newCtx("POST", "/save", "")
	controller.SaveController(ctx)

	if ctx.Response.StatusCode() != 204 {
		t.Fatal("save failed")
	}
}

func TestMultiGetController(t *testing.T) {
	setup(t)

	ctx := newCtx("PUT", "/kv/a", "1")
	ctx.SetUserValue("key", "a")
	controller.PutKeyController(ctx)

	ctx = newCtx("PUT", "/kv/b", "2")
	ctx.SetUserValue("key", "b")
	controller.PutKeyController(ctx)

	ctx = newCtx("GET", "/kv/mget?keys=a,b,c", "")
	controller.MultiGetController(ctx)

	body := string(ctx.Response.Body())
	if !strings.Contains(body, `"key":"a"`) || !strings.Contains(body, `"key":"b"`) || !strings.Contains(body, `"key":"c"`) {
		t.Fatal("mget failed")
	}
}

func TestWildcardGetController(t *testing.T) {
	setup(t)

	ctx := newCtx("PUT", "/kv/a1", "x")
	ctx.SetUserValue("key", "a1")
	controller.PutKeyController(ctx)

	ctx = newCtx("PUT", "/kv/a2", "y")
	ctx.SetUserValue("key", "a2")
	controller.PutKeyController(ctx)

	ctx = newCtx("GET", "/kv/a*", "")
	ctx.SetUserValue("key", "a*")
	controller.GetKeyController(ctx)

	body := string(ctx.Response.Body())
	if !strings.Contains(body, "a1") || !strings.Contains(body, "a2") {
		t.Fatal("wildcard get failed")
	}
}

func TestStatsController(t *testing.T) {
	setup(t)

	ctx := newCtx("GET", "/stats", "")
	controller.StatsController(ctx)

	var out map[string]interface{}
	if err := json.Unmarshal(ctx.Response.Body(), &out); err != nil {
		t.Fatal("invalid stats json")
	}
}

func TestGetConfigController(t *testing.T) {
	setup(t)

	ctx := newCtx("GET", "/config", "")
	controller.GetConfigController(ctx)

	if ctx.Response.StatusCode() != 200 {
		t.Fatalf("expected 200, got %d", ctx.Response.StatusCode())
	}

	body := ctx.Response.Body()
	if len(body) == 0 {
		t.Fatalf("empty body")
	}

	var out map[string]interface{}
	if err := json.Unmarshal(body, &out); err != nil {
		t.Fatalf("invalid json: %v", err)
	}

	if len(out) == 0 {
		t.Fatalf("expected non-empty config object")
	}

	found := false
	for k := range out {
		if strings.Contains(strings.ToLower(k), "store") ||
			strings.Contains(strings.ToLower(k), "folder") ||
			strings.Contains(strings.ToLower(k), "shard") {
			found = true
			break
		}
	}

	if !found {
		t.Fatalf("config json does not contain expected storage-related keys: %v", out)
	}
}
