package routing_test

import (
	"bytes"
	"net"
	"testing"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/http/adminui"
	"github.com/valyala/fasthttp"
)

var originalVersion = routing.Version

func init() {
	routing.Version = func(h fasthttp.RequestHandler) fasthttp.RequestHandler {
		return func(ctx *fasthttp.RequestCtx) {
			if v := ctx.UserValue("called"); v != nil {
				flag := v.(*bool)
				*flag = true
			}
			originalVersion(h)(ctx)
		}
	}

	http_adminuiHandler := adminui.AdminUIAuth
	adminui.AdminUIAuth = func(h fasthttp.RequestHandler) fasthttp.RequestHandler {
		return func(ctx *fasthttp.RequestCtx) {
			if v := ctx.UserValue("called"); v != nil {
				flag := v.(*bool)
				*flag = true
			}
			http_adminuiHandler(h)(ctx)
		}
	}
}

func initEnv(t *testing.T, stats, schema, admin bool) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4

	cfg.Security = configuration.SecurityConfig{}
	cfg.Security.Authentication = configuration.AuthenticationConfig{}
	cfg.Security.Authentication.Enabled = false
	cfg.Security.Authentication.Mode = ""

	cfg.Stats.Enabled = stats
	cfg.Api.Schema.Enabled = schema
	cfg.AdminUI.Enabled = admin

	globals.SetConfig(cfg)

	storage.LoadDB()
	storage.LoadJsonDB()
}

func perform(r *router.Router, method, path string) bool {
	req := fasthttp.AcquireRequest()
	req.Header.SetMethod(method)
	req.SetRequestURI(path)

	var ctx fasthttp.RequestCtx

	fakeAddr := &net.TCPAddr{
		IP:   net.ParseIP("127.0.0.1"),
		Port: 12345,
	}
	ctx.Init(req, fakeAddr, nil)

	called := false
	ctx.SetUserValue("called", &called)

	r.Handler(&ctx)

	return *ctx.UserValue("called").(*bool)
}

func TestVersionWrapper(t *testing.T) {
	h := routing.Version(func(ctx *fasthttp.RequestCtx) {
		ctx.SetBody([]byte("ok"))
	})
	req := fasthttp.AcquireRequest()
	req.Header.SetMethod("GET")
	req.SetRequestURI("/x")
	var ctx fasthttp.RequestCtx
	ctx.Init(req, nil, nil)
	h(&ctx)
	if !bytes.Equal(ctx.Response.Body(), []byte("ok")) {
		t.Fatalf("body mismatch")
	}
	if len(ctx.Response.Header.Peek("X-Elysian-Version")) == 0 {
		t.Fatalf("missing version header")
	}
}

func TestRegisterRoutes_NoOptional(t *testing.T) {
	initEnv(t, false, false, false)

	r := router.New()
	routing.RegisterRoutes(r)

	tests := [][]string{
		{"GET", "/health"},
		{"GET", "/kv/mget"},
		{"GET", "/kv/foo"},
		{"PUT", "/kv/foo"},
		{"DELETE", "/kv/foo"},
		{"POST", "/save"},
		{"POST", "/reset"},
		{"GET", "/api/export"},
		{"POST", "/api/import"},
		{"GET", "/api/x"},
		{"POST", "/api/x"},
		{"GET", "/api/x/123"},
		{"PUT", "/api/x/123"},
		{"PUT", "/api/x"},
		{"DELETE", "/api/x/123"},
		{"DELETE", "/api/x"},
		{"GET", "/api/x/count"},
		{"GET", "/api/x/123/exists"},
		{"POST", "/api/x/migrate"},
		{"POST", "/api/tx/begin"},
		{"POST", "/api/tx/t1/rollback"},
		{"POST", "/api/tx/t1/entity/x"},
		{"PUT", "/api/tx/t1/entity/x/1"},
		{"DELETE", "/api/tx/t1/entity/x/1"},
		{"POST", "/api/tx/t1/commit"},
	}

	for _, c := range tests {
		if !perform(r, c[0], c[1]) {
			t.Fatalf("route not reachable: %s %s", c[0], c[1])
		}
	}
}

func TestRegisterRoutes_WithStats(t *testing.T) {
	initEnv(t, true, false, false)
	r := router.New()
	routing.RegisterRoutes(r)
	if !perform(r, "GET", "/stats") {
		t.Fatalf("missing /stats")
	}
}

func TestRegisterRoutes_WithSchema(t *testing.T) {
	initEnv(t, false, true, false)
	r := router.New()
	routing.RegisterRoutes(r)

	if !perform(r, "POST", "/api/books/create") {
		t.Fatalf("missing create")
	}
	if !perform(r, "GET", "/api/books/schema") {
		t.Fatalf("missing get schema")
	}
	if !perform(r, "PUT", "/api/books/schema") {
		t.Fatalf("missing put schema")
	}
}

func TestRegisterRoutes_WithAdmin(t *testing.T) {
	initEnv(t, false, false, true)
	r := router.New()
	routing.RegisterRoutes(r)

	if !perform(r, "GET", "/admin/config") {
		t.Fatalf("missing admin config")
	}
	if !perform(r, "GET", "/admin/anyfile") {
		t.Fatalf("missing admin wildcard")
	}
}
