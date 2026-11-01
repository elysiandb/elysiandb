package observe_test

import (
	"errors"
	"log/slog"
	"strings"
	"testing"
	"time"

	"github.com/fasthttp/router"
	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/log"
	"github.com/taymour/elysiandb/internal/metrics"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T, mutate func(cfg *configuration.Config)) fasthttp.RequestHandler {
	t.Helper()

	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Log.SlowRequestMs = 50
	if mutate != nil {
		mutate(cfg)
	}
	globals.SetConfig(cfg)

	if err := log.Configure(log.Options{Level: "debug", BufferLines: 1000}); err != nil {
		t.Fatal(err)
	}
	log.SetOutput(&strings.Builder{}, false)
	metrics.Reset()
	observe.ResetThrottles()

	t.Cleanup(func() {
		log.SetOutput(nil, false)
		_ = log.Configure(log.Options{})
		metrics.Reset()
	})

	r := router.New()
	r.SaveMatchedRoutePath = true
	r.PanicHandler = observe.Panic

	r.GET("/ok/{entity}", func(ctx *fasthttp.RequestCtx) { ctx.SetBodyString("ok") })
	r.GET("/fail", func(ctx *fasthttp.RequestCtx) {
		observe.SetError(ctx, errors.New("disk on fire"))
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
	})
	r.GET("/panic", func(ctx *fasthttp.RequestCtx) { panic("kaboom") })
	r.GET("/slow", func(ctx *fasthttp.RequestCtx) { time.Sleep(60 * time.Millisecond) })
	r.GET("/denied", func(ctx *fasthttp.RequestCtx) { ctx.SetStatusCode(fasthttp.StatusUnauthorized) })
	r.GET("/forbidden", func(ctx *fasthttp.RequestCtx) {
		ctx.SetUserValue("username", "bob")
		ctx.SetStatusCode(fasthttp.StatusForbidden)
	})
	r.GET("/missing", func(ctx *fasthttp.RequestCtx) { ctx.SetStatusCode(fasthttp.StatusNotFound) })
	r.GET("/monitoring/overview", func(ctx *fasthttp.RequestCtx) {})
	r.GET("/api/security/me", func(ctx *fasthttp.RequestCtx) { ctx.SetStatusCode(fasthttp.StatusUnauthorized) })

	return observe.Middleware(r.Handler)
}

func call(h fasthttp.RequestHandler, method, uri string, headers map[string]string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.SetRequestURI(uri)
	req.Header.SetMethod(method)
	for k, v := range headers {
		req.Header.Set(k, v)
	}

	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	h(ctx)

	return ctx
}

func entries(q log.Query) []log.Entry {
	return log.Logs(q).Entries
}

func TestRequestIDIsKeptOrGenerated(t *testing.T) {
	h := setup(t, nil)

	ctx := call(h, "GET", "/ok/books", map[string]string{observe.RequestIDHeader: "client-id_1.2"})
	if got := string(ctx.Response.Header.Peek(observe.RequestIDHeader)); got != "client-id_1.2" {
		t.Fatalf("a valid client request id must be kept, got %q", got)
	}

	for _, invalid := range []string{"has space", strings.Repeat("a", 65), "semi;colon"} {
		ctx = call(h, "GET", "/ok/books", map[string]string{observe.RequestIDHeader: invalid})
		got := string(ctx.Response.Header.Peek(observe.RequestIDHeader))
		if got == "" || got == invalid {
			t.Fatalf("an invalid request id must be replaced, got %q for %q", got, invalid)
		}
	}

	first := string(call(h, "GET", "/ok/books", nil).Response.Header.Peek(observe.RequestIDHeader))
	second := string(call(h, "GET", "/ok/books", nil).Response.Header.Peek(observe.RequestIDHeader))
	if first == "" || first == second {
		t.Fatalf("generated ids must be unique: %q %q", first, second)
	}
}

func TestServerErrorIsLoggedOnceWithItsError(t *testing.T) {
	h := setup(t, nil)

	ctx := call(h, "GET", "/fail", map[string]string{observe.RequestIDHeader: "req-9"})

	got := entries(log.Query{MinLevel: slog.LevelError})
	if len(got) != 1 {
		t.Fatalf("expected one error line, got %+v", got)
	}

	e := got[0]
	if e.Message != "request failed" || e.Component != "http" || e.Attrs["err"] != "disk on fire" || e.Attrs["route"] != "/fail" || e.Attrs["status"] != int64(500) || e.Attrs["request_id"] != "req-9" {
		t.Fatalf("unexpected entry %+v", e)
	}

	if ctx.Response.StatusCode() != 500 {
		t.Fatalf("unexpected status %d", ctx.Response.StatusCode())
	}
}

func TestPanicIsRecoveredAndLoggedOnce(t *testing.T) {
	h := setup(t, nil)

	ctx := call(h, "GET", "/panic", nil)
	if ctx.Response.StatusCode() != 500 {
		t.Fatalf("a panic must answer 500, got %d", ctx.Response.StatusCode())
	}

	got := entries(log.Query{MinLevel: slog.LevelError})
	if len(got) != 1 || got[0].Message != "handler panic" || got[0].Attrs["panic"] != "kaboom" {
		t.Fatalf("expected one handler panic line, got %+v", got)
	}

	if stack, _ := got[0].Attrs["stack"].(string); !strings.Contains(stack, "goroutine") {
		t.Fatalf("the stack must be logged")
	}
}

func TestUnauthorizedIsAWarningOnlyWithCredentials(t *testing.T) {
	h := setup(t, nil)

	call(h, "GET", "/denied", nil)
	if got := entries(log.Query{MinLevel: slog.LevelWarn}); len(got) != 0 {
		t.Fatalf("a request without credentials is not a security warning: %+v", got)
	}

	call(h, "GET", "/denied", map[string]string{"Authorization": "Bearer nope"})
	call(h, "GET", "/denied", map[string]string{"Authorization": "Bearer nope"})

	got := entries(log.Query{MinLevel: slog.LevelWarn})
	if len(got) != 1 || got[0].Message != "unauthorized request" || got[0].Component != "security" {
		t.Fatalf("expected one throttled warning, got %+v", got)
	}

	call(h, "GET", "/api/security/me", map[string]string{"Cookie": security.SessionCookieName + "=expired"})
	if got := entries(log.Query{MinLevel: slog.LevelWarn}); len(got) != 1 {
		t.Fatalf("the Admin UI session check must stay quiet: %+v", got)
	}
}

func TestForbiddenIsAWarningWithTheUser(t *testing.T) {
	h := setup(t, nil)

	call(h, "GET", "/forbidden", nil)

	got := entries(log.Query{MinLevel: slog.LevelWarn})
	if len(got) != 1 || got[0].Message != "forbidden request" || got[0].Attrs["user"] != "bob" {
		t.Fatalf("unexpected entries %+v", got)
	}
}

func TestClientErrorsAreDebug(t *testing.T) {
	h := setup(t, nil)

	call(h, "GET", "/missing", nil)
	call(h, "GET", "/nowhere", nil)

	if got := entries(log.Query{MinLevel: slog.LevelInfo}); len(got) != 0 {
		t.Fatalf("4xx answers are not logged above debug: %+v", got)
	}

	got := entries(log.Query{MinLevel: slog.LevelDebug, Text: "client error"})
	if len(got) != 2 {
		t.Fatalf("expected two debug lines, got %+v", got)
	}

	if got[0].Attrs["route"] != "unmatched" {
		t.Fatalf("an unknown path is reported as unmatched, got %+v", got[0])
	}
}

func TestSlowRequestIsAWarning(t *testing.T) {
	h := setup(t, nil)

	call(h, "GET", "/slow", nil)

	got := entries(log.Query{MinLevel: slog.LevelWarn})
	if len(got) != 1 || got[0].Message != "slow request" || got[0].Attrs["route"] != "/slow" {
		t.Fatalf("unexpected entries %+v", got)
	}
}

func TestSlowRequestCanBeDisabled(t *testing.T) {
	h := setup(t, func(cfg *configuration.Config) { cfg.Log.SlowRequestMs = 0 })

	call(h, "GET", "/slow", nil)

	if got := entries(log.Query{MinLevel: slog.LevelWarn}); len(got) != 0 {
		t.Fatalf("slowRequestMs: 0 disables the warning: %+v", got)
	}
}

func TestAccessLog(t *testing.T) {
	h := setup(t, func(cfg *configuration.Config) { cfg.Log.AccessLog = true })

	call(h, "GET", "/ok/books", nil)
	call(h, "GET", "/monitoring/overview", nil)

	got := entries(log.Query{MinLevel: slog.LevelInfo})
	if len(got) != 1 || got[0].Message != "request" || got[0].Attrs["route"] != "/ok/{entity}" || got[0].Attrs["status"] != int64(200) {
		t.Fatalf("expected one access line for the API request only, got %+v", got)
	}
}

func TestNoAccessLogByDefault(t *testing.T) {
	h := setup(t, nil)

	call(h, "GET", "/ok/books", nil)

	if got := entries(log.Query{MinLevel: slog.LevelInfo}); len(got) != 0 {
		t.Fatalf("no info line per request by default: %+v", got)
	}
}

func TestMonitoringRequestsAreNotMeasured(t *testing.T) {
	h := setup(t, nil)
	t0 := time.Now()

	metrics.Tick(t0)
	call(h, "GET", "/monitoring/overview", nil)
	call(h, "GET", "/ok/books", nil)
	metrics.Tick(t0.Add(time.Second))

	routes := metrics.CurrentDetails().Routes.SinceStartup
	if len(routes) != 1 || routes[0].Route != "/ok/{entity}" {
		t.Fatalf("monitoring requests must not be counted: %+v", routes)
	}
}

func TestKind(t *testing.T) {
	cases := []struct {
		method, route, want string
	}{
		{"GET", "/kv/{key}", metrics.KindKVGet},
		{"GET", "/kv/mget", metrics.KindKVGet},
		{"PUT", "/kv/{key}", metrics.KindKVSet},
		{"DELETE", "/kv/{key}", metrics.KindKVDelete},
		{"POST", "/api/query", metrics.KindDocQuery},
		{"GET", "/api/{entity}", metrics.KindDocRead},
		{"GET", "/api/{entity}/{id}", metrics.KindDocRead},
		{"POST", "/api/{entity}", metrics.KindDocWrite},
		{"DELETE", "/api/{entity}/{id}", metrics.KindDocWrite},
		{"POST", "/api/tx/{txId}/commit", metrics.KindDocWrite},
		{"GET", "/health", ""},
		{"GET", "/api/backup", ""},
	}

	for _, c := range cases {
		if got := observe.Kind(c.method, c.route); got != c.want {
			t.Fatalf("Kind(%s %s) = %q, want %q", c.method, c.route, got, c.want)
		}
	}
}

func TestExcluded(t *testing.T) {
	for path, want := range map[string]bool{
		"/health":              true,
		"/metrics":             true,
		"/admin":               true,
		"/admin/assets/app.js": true,
		"/monitoring/logs":     true,
		"/administrators":      false,
		"/api/monitoring":      false,
		"/api/books":           false,
	} {
		if got := observe.Excluded([]byte(path)); got != want {
			t.Fatalf("Excluded(%q) = %v", path, got)
		}
	}
}

func TestAuditCarriesActorAndRequest(t *testing.T) {
	setup(t, nil)

	req := fasthttp.AcquireRequest()
	req.SetRequestURI("/x")
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	ctx.SetUserValue("username", "alice")

	observe.Audit(ctx, "schema changed", "entity", "books")

	got := entries(log.Query{Component: "audit"})
	if len(got) != 1 || got[0].Message != "schema changed" || got[0].Attrs["actor"] != "alice" || got[0].Attrs["source"] != "http" || got[0].Attrs["entity"] != "books" {
		t.Fatalf("unexpected audit entry %+v", got)
	}
}
