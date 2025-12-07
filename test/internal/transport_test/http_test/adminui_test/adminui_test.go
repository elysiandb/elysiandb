package adminui_test

import (
	"bytes"
	"encoding/json"
	"testing"
	"time"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/http/adminui"
	"github.com/valyala/fasthttp"
)

func setup(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Store.Folder = t.TempDir()
	cfg.Store.Shards = 4
	globals.SetConfig(cfg)
	storage.LoadDB()
	storage.LoadJsonDB()
}

func newCtx(method, path, body string) *fasthttp.RequestCtx {
	req := fasthttp.AcquireRequest()
	req.Header.SetMethod(method)
	req.SetRequestURI(path)
	if body != "" {
		req.SetBody([]byte(body))
	}
	ctx := &fasthttp.RequestCtx{}
	ctx.Init(req, nil, nil)
	return ctx
}

func TestAdminAuth_NoCookie(t *testing.T) {
	setup(t)
	called := false
	h := adminui.AdminAuth(func(ctx *fasthttp.RequestCtx) { called = true })
	ctx := newCtx("GET", "/admin", "")
	h(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401")
	}
	if called {
		t.Fatalf("should not call next")
	}
}

func TestAdminAuth_InvalidSession(t *testing.T) {
	setup(t)
	called := false
	h := adminui.AdminAuth(func(ctx *fasthttp.RequestCtx) { called = true })
	ctx := newCtx("GET", "/admin", "")
	ctx.Request.Header.SetCookie(security.SessionCookieName, "bad")
	h(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401")
	}
	if called {
		t.Fatalf("should not call next")
	}
}

func TestAdminAuth_ForbiddenForNonAdmin(t *testing.T) {
	setup(t)
	u := &security.BasicUser{Username: "u", Password: "x", Role: security.RoleUser}
	security.CreateBasicUser(u)
	s, _ := security.CreateSession("u", security.RoleUser, time.Hour)
	called := false
	h := adminui.AdminAuth(func(ctx *fasthttp.RequestCtx) { called = true })
	ctx := newCtx("GET", "/admin", "")
	ctx.Request.Header.SetCookie(security.SessionCookieName, s.ID)
	h(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusForbidden {
		t.Fatalf("expected 403")
	}
	if called {
		t.Fatalf("should not call next")
	}
}

func TestAdminAuth_OK(t *testing.T) {
	setup(t)
	u := &security.BasicUser{Username: "a", Password: "x", Role: security.RoleAdmin}
	security.CreateBasicUser(u)
	s, _ := security.CreateSession("a", security.RoleAdmin, time.Hour)
	called := false
	h := adminui.AdminAuth(func(ctx *fasthttp.RequestCtx) { called = true })
	ctx := newCtx("GET", "/admin", "")
	ctx.Request.Header.SetCookie(security.SessionCookieName, s.ID)
	h(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("unexpected status %d", ctx.Response.StatusCode())
	}
	if !called {
		t.Fatalf("expected next to be called")
	}
}

func TestLoginController_InvalidJSON(t *testing.T) {
	setup(t)
	ctx := newCtx("POST", "/login", `{invalid`)
	adminui.LoginController(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusBadRequest {
		t.Fatalf("expected 400")
	}
}

func TestLoginController_InvalidCredentials(t *testing.T) {
	setup(t)
	ctx := newCtx("POST", "/login", `{"username":"x","password":"y"}`)
	adminui.LoginController(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401")
	}
}

func TestLoginController_OK(t *testing.T) {
	setup(t)
	u := &security.BasicUser{Username: "admin", Password: "x", Role: security.RoleAdmin}
	security.CreateBasicUser(u)
	ctx := newCtx("POST", "/login", `{"username":"admin","password":"x"}`)
	adminui.LoginController(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
	if len(ctx.Response.Header.PeekCookie(security.SessionCookieName)) == 0 {
		t.Fatalf("cookie not set")
	}
}

func TestLogoutController(t *testing.T) {
	setup(t)

	s, _ := security.CreateSession("a", security.RoleAdmin, time.Hour)

	ctx := newCtx("POST", "/logout", "")
	ctx.Request.Header.SetCookie(security.SessionCookieName, s.ID)

	adminui.LogoutController(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusNoContent {
		t.Fatalf("expected 204")
	}

	sc := ctx.Response.Header.Peek("Set-Cookie")
	if sc == nil {
		t.Fatalf("Set-Cookie missing")
	}

	if !bytes.Contains(bytes.ToLower(sc), []byte("expires=")) {
		t.Fatalf("expected expires= in Set-Cookie, got %s", sc)
	}

	if !bytes.Contains(sc, []byte(security.SessionCookieName+"=")) {
		t.Fatalf("expected cookie key, got %s", sc)
	}
}

func TestMeController_NoCookie(t *testing.T) {
	setup(t)
	ctx := newCtx("GET", "/me", "")
	adminui.MeController(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401")
	}
}

func TestMeController_InvalidSession(t *testing.T) {
	setup(t)
	ctx := newCtx("GET", "/me", "")
	ctx.Request.Header.SetCookie(security.SessionCookieName, "no")
	adminui.MeController(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401")
	}
}

func TestMeController_OK(t *testing.T) {
	setup(t)
	u := &security.BasicUser{Username: "zzz", Password: "x", Role: security.RoleAdmin}
	security.CreateBasicUser(u)
	s, _ := security.CreateSession("zzz", security.RoleAdmin, time.Hour)
	ctx := newCtx("GET", "/me", "")
	ctx.Request.Header.SetCookie(security.SessionCookieName, s.ID)
	adminui.MeController(ctx)
	if ctx.Response.StatusCode() != fasthttp.StatusOK {
		t.Fatalf("expected 200")
	}
	var m map[string]interface{}
	json.Unmarshal(ctx.Response.Body(), &m)
	if m["username"] != "zzz" {
		t.Fatalf("wrong username")
	}
	if m["role"] != string(security.RoleAdmin) {
		t.Fatalf("wrong role")
	}
}
