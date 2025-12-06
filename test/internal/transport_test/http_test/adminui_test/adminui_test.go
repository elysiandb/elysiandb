package adminui_test

import (
	"encoding/base64"
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/transport/http/adminui"
	"github.com/valyala/fasthttp"
)

func TestAdminUIAuth_Disabled(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.AdminUI.Enabled = false
	globals.SetConfig(cfg)

	called := false
	handler := adminui.AdminUIAuth(func(ctx *fasthttp.RequestCtx) {
		called = true
	})

	ctx := &fasthttp.RequestCtx{}
	handler(ctx)

	if !called {
		t.Fatalf("handler should run when admin UI disabled")
	}
}

func TestAdminUIAuth_NoHeader(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.AdminUI.Enabled = true
	cfg.AdminUI.Username = "admin"
	cfg.AdminUI.Password = "pass"
	globals.SetConfig(cfg)

	handler := adminui.AdminUIAuth(func(ctx *fasthttp.RequestCtx) {})

	ctx := &fasthttp.RequestCtx{}
	handler(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 for missing header")
	}
}

func TestAdminUIAuth_InvalidBase64(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.AdminUI.Enabled = true
	cfg.AdminUI.Username = "admin"
	cfg.AdminUI.Password = "pass"
	globals.SetConfig(cfg)

	handler := adminui.AdminUIAuth(func(ctx *fasthttp.RequestCtx) {})

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Basic !!!invalid!!!")
	handler(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 for invalid base64")
	}
}

func TestAdminUIAuth_InvalidFormat(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.AdminUI.Enabled = true
	cfg.AdminUI.Username = "admin"
	cfg.AdminUI.Password = "pass"
	globals.SetConfig(cfg)

	payload := base64.StdEncoding.EncodeToString([]byte("onlyusername"))
	header := "Basic " + payload

	handler := adminui.AdminUIAuth(func(ctx *fasthttp.RequestCtx) {})

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", header)
	handler(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 for missing password")
	}
}

func TestAdminUIAuth_WrongCredentials(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.AdminUI.Enabled = true
	cfg.AdminUI.Username = "admin"
	cfg.AdminUI.Password = "pass"
	globals.SetConfig(cfg)

	payload := base64.StdEncoding.EncodeToString([]byte("admin:wrong"))
	header := "Basic " + payload

	handler := adminui.AdminUIAuth(func(ctx *fasthttp.RequestCtx) {})

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", header)
	handler(ctx)

	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 for wrong credentials")
	}
}

func TestAdminUIAuth_Success(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.AdminUI.Enabled = true
	cfg.AdminUI.Username = "admin"
	cfg.AdminUI.Password = "pass"
	globals.SetConfig(cfg)

	payload := base64.StdEncoding.EncodeToString([]byte("admin:pass"))
	header := "Basic " + payload

	called := false
	handler := adminui.AdminUIAuth(func(ctx *fasthttp.RequestCtx) {
		called = true
	})

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", header)
	handler(ctx)

	if !called {
		t.Fatalf("handler should be executed on valid credentials")
	}
}
