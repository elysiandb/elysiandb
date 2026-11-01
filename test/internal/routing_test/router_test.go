package routing_test

import (
	"bytes"
	"io"
	"net"
	"os"
	"slices"
	"strings"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/routing"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	http_adminui "github.com/taymour/elysiandb/internal/transport/http/adminui"
	"github.com/taymour/elysiandb/internal/transport/http/controller"
	"github.com/valyala/fasthttp"
)

var (
	originalVersion   = routing.Version
	originalGetConfig = controller.GetConfigController
	originalAdminUI   = http_adminui.AdminUIHandler
)

func init() {
	routing.Version = func(h fasthttp.RequestHandler) fasthttp.RequestHandler {
		return func(ctx *fasthttp.RequestCtx) {
			if v := ctx.UserValue("called"); v != nil {
				*(v.(*bool)) = true
			}
			originalVersion(h)(ctx)
		}
	}

	http_adminui.AdminUIHandler = func(ctx *fasthttp.RequestCtx) {
		if v := ctx.UserValue("called"); v != nil {
			*(v.(*bool)) = true
		}
		originalAdminUI(ctx)
	}

	controller.GetConfigController = func(ctx *fasthttp.RequestCtx) {
		if v := ctx.UserValue("called"); v != nil {
			*(v.(*bool)) = true
		}
		originalGetConfig(ctx)
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
	addr := &net.TCPAddr{IP: net.ParseIP("127.0.0.1"), Port: 12345}
	ctx.Init(req, addr, nil)
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
		{"GET", "/config"},
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

func performAs(r *router.Router, sessionID, method, path string) int {
	req := fasthttp.AcquireRequest()
	req.Header.SetMethod(method)
	req.SetRequestURI(path)
	req.Header.SetCookie(security.SessionCookieName, sessionID)
	var ctx fasthttp.RequestCtx
	addr := &net.TCPAddr{IP: net.ParseIP("127.0.0.1"), Port: 12345}
	ctx.Init(req, addr, nil)
	r.Handler(&ctx)

	return ctx.Response.StatusCode()
}

func TestRegisterRoutes_AdminRoutesInUserMode(t *testing.T) {
	initEnv(t, true, true, false)
	cfg := globals.GetConfig()
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"

	r := router.New()
	routing.RegisterRoutes(r)

	ensureUser("bob", security.RoleUser)
	ensureUser("admin", security.RoleAdmin)
	user, _ := security.CreateSession("bob", security.RoleUser, time.Hour)
	admin, _ := security.CreateSession("admin", security.RoleAdmin, time.Hour)

	adminRoutes := [][]string{
		{"GET", "/kv/mget?keys=foo"},
		{"GET", "/kv/foo"},
		{"PUT", "/kv/foo"},
		{"DELETE", "/kv/foo"},
		{"POST", "/save"},
		{"POST", "/reset"},
		{"GET", "/stats"},
		{"GET", "/config"},
		{"GET", "/api/export"},
		{"POST", "/api/import"},
		{"POST", "/api/books/migrate"},
		{"POST", "/api/books/create"},
		{"PUT", "/api/books/schema"},
		{"GET", "/monitoring/overview"},
		{"GET", "/monitoring/series"},
		{"GET", "/monitoring/details"},
		{"GET", "/monitoring/logs"},
		{"GET", "/monitoring/logs/download"},
		{"GET", "/monitoring/logs/level"},
		{"PUT", "/monitoring/logs/level"},
	}

	for _, c := range adminRoutes {
		if status := performAs(r, user.ID, c[0], c[1]); status != fasthttp.StatusForbidden {
			t.Fatalf("expected 403 for a regular user on %s %s, got %d", c[0], c[1], status)
		}
	}

	userRoutes := [][]string{
		{"GET", "/health"},
		{"GET", "/api/books"},
		{"GET", "/api/books/schema"},
	}

	for _, c := range userRoutes {
		if status := performAs(r, user.ID, c[0], c[1]); status == fasthttp.StatusForbidden || status == fasthttp.StatusUnauthorized {
			t.Fatalf("expected access for a regular user on %s %s, got %d", c[0], c[1], status)
		}
	}

	for _, c := range adminRoutes {
		if status := performAs(r, admin.ID, c[0], c[1]); status == fasthttp.StatusForbidden || status == fasthttp.StatusUnauthorized {
			t.Fatalf("expected access for an admin on %s %s, got %d", c[0], c[1], status)
		}
	}
}

func TestRegisterRoutes_AdminUIWarnsWhenAssetsAreMissing(t *testing.T) {
	initEnv(t, false, false, true)

	original := os.Stdout
	reader, writer, err := os.Pipe()
	if err != nil {
		t.Fatal(err)
	}
	os.Stdout = writer

	r := router.New()
	routing.RegisterRoutes(r)

	writer.Close()
	os.Stdout = original
	output, _ := io.ReadAll(reader)

	warned := strings.Contains(string(output), "assets are missing")
	if warned == http_adminui.IsBuilt() {
		t.Fatalf("expected a warning only when the Admin UI assets are missing (built=%v, output=%q)", http_adminui.IsBuilt(), output)
	}

	if !slices.Contains(r.List()["GET"], "/admin/{filepath:*}") {
		t.Fatalf("expected the Admin UI route to be registered, got %v", r.List()["GET"])
	}
}

func TestRegisterRoutes_MetricsOnlyWhenPrometheusIsEnabled(t *testing.T) {
	initEnv(t, false, false, false)

	r := router.New()
	routing.RegisterRoutes(r)
	if slices.Contains(r.List()["GET"], "/metrics") {
		t.Fatalf("/metrics must not be served when Prometheus is disabled")
	}

	globals.GetConfig().Monitoring.Prometheus.Enabled = true
	r = router.New()
	routing.RegisterRoutes(r)
	if !slices.Contains(r.List()["GET"], "/metrics") {
		t.Fatalf("/metrics must be served when Prometheus is enabled, got %v", r.List()["GET"])
	}
}

func TestRegisterRoutes_MonitoringDoesNotHideEntityTypes(t *testing.T) {
	initEnv(t, false, false, false)

	r := router.New()
	routing.RegisterRoutes(r)

	for _, path := range r.List()["GET"] {
		if strings.HasPrefix(path, "/api/logs") || strings.HasPrefix(path, "/api/monitoring") {
			t.Fatalf("%s would hide an entity type", path)
		}
	}
}

func ensureUser(username string, role security.Role) {
	if !security.BasicUserExists(username) {
		_ = security.CreateBasicUser(&security.BasicUser{Username: username, Password: username + "-password", Role: role})
	}
}
