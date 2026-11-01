package e2e

import (
	"strings"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/valyala/fasthttp"
)

func TestLoginIsThrottledAfterRepeatedFailures(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"ops","password":"ops-password","role":"admin"}`)

	client := s.from("203.0.113.10")
	for i := 0; i < 5; i++ {
		client.mustDo(fasthttp.StatusUnauthorized, "POST", "/api/security/login", `{"username":"ops","password":"wrong"}`)
	}

	resp := client.mustDo(fasthttp.StatusTooManyRequests, "POST", "/api/security/login", `{"username":"ops","password":"ops-password"}`)
	if resp.header("Retry-After") == "" {
		t.Fatalf("expected a Retry-After header")
	}

	s.from("203.0.113.11").login("ops", "ops-password")
}

func TestDefaultAdminPasswordIsOnlyAcceptedLocally(t *testing.T) {
	s := startAPIServer(t, nil)

	s.from("203.0.113.20").mustDo(fasthttp.StatusUnauthorized, "POST", "/api/security/login", `{"username":"admin","password":"admin"}`)
	s.mustDo(fasthttp.StatusUnauthorized, "POST", "/api/security/login", `{"username":"admin","password":"admin"}`, "X-Forwarded-For", "203.0.113.21")

	s.login("admin", "admin")
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/admin/password", `{"password":"a-better-password"}`)
	s.mustDo(fasthttp.StatusOK, "GET", "/api/security/me", "")

	s.from("203.0.113.20").login("admin", "a-better-password")
}

func TestTrustedProxyForwardsTheClientAddress(t *testing.T) {
	s := startAPIServer(t, func(cfg *configuration.Config) {
		cfg.Server.HTTP.TrustedProxies = []string{"10.0.0.0/8"}
	})

	proxy := s.from("10.0.0.5")
	proxy.mustDo(fasthttp.StatusUnauthorized, "POST", "/api/security/login", `{"username":"admin","password":"admin"}`, "X-Forwarded-For", "127.0.0.1, 203.0.113.30")
	proxy.headers["X-Forwarded-For"] = "127.0.0.1"
	proxy.login("admin", "admin")
	proxy.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/admin/password", `{"password":"a-better-password"}`)
	delete(proxy.headers, "X-Forwarded-For")

	for i := 0; i < 5; i++ {
		proxy.mustDo(fasthttp.StatusUnauthorized, "POST", "/api/security/login", `{"username":"admin","password":"wrong"}`, "X-Forwarded-For", "203.0.113.31")
	}

	proxy.mustDo(fasthttp.StatusTooManyRequests, "POST", "/api/security/login", `{"username":"admin","password":"a-better-password"}`, "X-Forwarded-For", "203.0.113.31")
	proxy.mustDo(fasthttp.StatusOK, "POST", "/api/security/login", `{"username":"admin","password":"a-better-password"}`, "X-Forwarded-For", "203.0.113.32")
}

func TestSessionsEndWithTheAccount(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"ops","password":"ops-password","role":"admin"}`)

	ops := s.anonymous()
	ops.login("ops", "ops-password")
	ops.mustDo(fasthttp.StatusOK, "GET", "/api/security/me", "")

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/ops/password", `{"password":"another-password"}`)
	ops.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/security/me", "")

	ops.login("ops", "another-password")
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/ops/role", `{"role":"user"}`)
	ops.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/security/user", "")

	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/ops/role", `{"role":"admin"}`)
	ops.login("ops", "another-password")
	s.mustDo(fasthttp.StatusOK, "DELETE", "/api/security/user/ops", "")
	ops.mustDo(fasthttp.StatusUnauthorized, "POST", "/api/security/user", `{"username":"ghost","password":"ghost-password","role":"admin"}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"ops","password":"ops-password","role":"admin"}`)
	ops.mustDo(fasthttp.StatusUnauthorized, "GET", "/api/security/me", "")
}

func TestAccountChangesAreValidated(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/security/user", `{"username":"ops","password":"short","role":"admin"}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/security/user", `{"username":"","password":"ops-password","role":"admin"}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/security/user", `{"username":"a/b","password":"ops-password","role":"admin"}`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/security/user", `{"username":"ops","password":"ops-password","role":"root"}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"ops","password":"ops-password","role":"user"}`)
	s.mustDo(fasthttp.StatusConflict, "POST", "/api/security/user", `{"username":"ops","password":"other-password","role":"admin"}`)

	s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/security/user/ops/password", `{}`)
	s.mustDo(fasthttp.StatusBadRequest, "PUT", "/api/security/user/ops/password", `{"password":"short"}`)
	s.mustDo(fasthttp.StatusOK, "PUT", "/api/security/user/ops/password", `{"password":"ops-password-2"}`)

	s.mustDo(fasthttp.StatusOK, "POST", "/api/security/user", `{"username":"o\"q","password":"quote-password","role":"user"}`)
	users := s.mustDo(fasthttp.StatusOK, "GET", "/api/security/user", "").object(t)["users"].([]any)
	if len(users) != 3 {
		t.Fatalf("expected 3 users, got %v", users)
	}

	user := s.mustDo(fasthttp.StatusOK, "GET", "/api/security/user/ops", "").object(t)
	if user["username"] != "ops" || user["role"] != "user" || len(user) != 2 {
		t.Fatalf("unexpected user %v", user)
	}
}

func TestCrossOriginWritesAreRejected(t *testing.T) {
	s := startAdminSession(t, nil)

	s.mustDo(fasthttp.StatusForbidden, "POST", "/api/book", `{"title":"x"}`, "Origin", "https://elsewhere.example", "Content-Type", "text/plain")
	s.mustDo(fasthttp.StatusForbidden, "POST", "/api/book", `{"title":"x"}`, "Sec-Fetch-Site", "cross-site")
	s.mustDo(fasthttp.StatusForbidden, "POST", "/api/book", `{"title":"x"}`, "Sec-Fetch-Site", "same-site")
	s.mustDo(fasthttp.StatusForbidden, "POST", "/api/book", `{"title":"x"}`, "Origin", "null")

	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"title":"x"}`, "Sec-Fetch-Site", "same-origin", "Origin", "http://test")
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"title":"x"}`, "Origin", "http://test")
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"title":"x"}`)
	s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "", "Sec-Fetch-Site", "cross-site")

	allowed := startAdminSession(t, func(cfg *configuration.Config) {
		cfg.Server.HTTP.CORS.AllowedOrigins = []string{"https://app.example"}
	})
	allowed.mustDo(fasthttp.StatusOK, "POST", "/api/book", `{"title":"x"}`, "Origin", "https://app.example", "Sec-Fetch-Site", "cross-site")
}

func TestResponsesCarrySecurityHeaders(t *testing.T) {
	s := startAdminSession(t, func(cfg *configuration.Config) {
		cfg.AdminUI.Enabled = true
	})

	api := s.mustDo(fasthttp.StatusOK, "GET", "/api/book", "")
	expected := map[string]string{
		"X-Content-Type-Options":  "nosniff",
		"X-Frame-Options":         "DENY",
		"Referrer-Policy":         "no-referrer",
		"Cache-Control":           "no-store",
		"Content-Security-Policy": "default-src 'none'; frame-ancestors 'none'",
	}
	for name, value := range expected {
		if got := api.header(name); got != value {
			t.Fatalf("%s: expected %q, got %q", name, value, got)
		}
	}

	ui := s.do("GET", "/admin/", "")
	if csp := ui.header("Content-Security-Policy"); !strings.Contains(csp, "script-src 'self'") || !strings.Contains(csp, "frame-ancestors 'none'") {
		t.Fatalf("unexpected Admin UI policy %q", csp)
	}
}

func TestImportKeepsInternalEntitiesUnchanged(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/book/create", `{"fields":{"title":{"type":"string","required":true}}}`)

	s.mustDo(fasthttp.StatusForbidden, "POST", "/api/import", `{"book":[{"id":"b1","title":"x","owner":{"@entity":"_elysiandb_core_user","id":"mallory","username":"mallory","password":"x","role":"admin"}}]}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/import", `{"_elysiandb_core_schema":[],"book":[{"id":"b1","title":"x"}]}`)

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/book", `{"id":"b2"}`)

	users := s.mustDo(fasthttp.StatusOK, "GET", "/api/security/user", "").object(t)["users"].([]any)
	if len(users) != 1 {
		t.Fatalf("expected the accounts to stay unchanged, got %v", users)
	}
}

func TestHooksWorkOnACopyOfTheDocument(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/note", `{"id":"n1","meta":{"n":1}}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/note", `{"name":"inc","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { ctx.entity.meta.n = ctx.entity.meta.n + 1; ctx.entity.accounts = ctx.query(\"_elysiandb_core_user\", {}).length }"}`)

	for i := 0; i < 3; i++ {
		note := s.mustDo(fasthttp.StatusOK, "GET", "/api/note/n1", "").object(t)
		meta, _ := note["meta"].(map[string]any)
		if meta["n"] != float64(2) || note["accounts"] != float64(0) {
			t.Fatalf("unexpected hook result %v", note)
		}
	}
}

func TestEndlessHookScriptIsStopped(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/loop", `{"id":"l1"}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/hook/loop", `{"name":"spin","event":"post_read","language":"javascript","priority":5,"enabled":true,"script":"function postRead(ctx) { while (true) {} }"}`)

	started := time.Now()
	s.mustDo(fasthttp.StatusOK, "GET", "/api/loop/l1", "")
	if elapsed := time.Since(started); elapsed > 10*time.Second {
		t.Fatalf("expected the script to be stopped, took %s", elapsed)
	}
}

func TestUnusualRequestsKeepTheServerRunning(t *testing.T) {
	s := startAdminSession(t, nil)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/cyc", `{"id":"x","self":{"@entity":"cyc","id":"x"},"items":[{"@entity":"cyc","id":"x"}]}`)

	s.mustDo(fasthttp.StatusOK, "GET", "/api/cyc/x?includes=all", "")
	s.mustDo(fasthttp.StatusOK, "GET", "/api/cyc?includes=all", "")
	s.mustDo(fasthttp.StatusOK, "GET", "/api/cyc/x?includes=self.self.self", "")

	s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"cyc","offset":-1}`)
	s.mustDo(fasthttp.StatusOK, "POST", "/api/query", `{"entity":"cyc","offset":1,"limit":9223372036854775807}`)

	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/cyc", `[null]`)
	s.mustDo(fasthttp.StatusBadRequest, "POST", "/api/cyc", `[{"id":"y"},null]`)

	s.mustDo(fasthttp.StatusOK, "GET", "/api/cyc/x", "")
}
