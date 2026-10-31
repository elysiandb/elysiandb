package security_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func setAllowedOrigins(origins ...string) {
	cfg := &configuration.Config{}
	cfg.Server.HTTP.CORS.AllowedOrigins = origins
	globals.SetConfig(cfg)
}

func TestCORSWithOrigin(t *testing.T) {
	setAllowedOrigins("http://example.com/")
	called := false
	next := func(ctx *fasthttp.RequestCtx) {
		called = true
	}

	h := security.CORS(next)

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Origin", "http://example.com")
	ctx.Request.Header.SetMethod("GET")

	h(ctx)

	if !called {
		t.Fatalf("next handler not called")
	}

	if string(ctx.Response.Header.Peek("Access-Control-Allow-Origin")) != "http://example.com" {
		t.Fatalf("allow origin header not set")
	}
	if string(ctx.Response.Header.Peek("Access-Control-Allow-Credentials")) != "true" {
		t.Fatalf("credentials header not set")
	}
	if string(ctx.Response.Header.Peek("Access-Control-Allow-Methods")) == "" {
		t.Fatalf("methods header missing")
	}
}

func TestCORSOriginNotAllowedByDefault(t *testing.T) {
	setAllowedOrigins()
	called := false
	h := security.CORS(func(ctx *fasthttp.RequestCtx) { called = true })

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Origin", "https://evil.example")
	ctx.Request.Header.SetMethod("GET")

	h(ctx)

	if !called {
		t.Fatalf("next handler not called")
	}

	for _, header := range []string{"Access-Control-Allow-Origin", "Access-Control-Allow-Credentials", "Access-Control-Allow-Methods"} {
		if v := ctx.Response.Header.Peek(header); v != nil {
			t.Fatalf("%s should not be set for an origin outside the allow-list, got %q", header, v)
		}
	}
}

func TestCORSOriginNotInAllowList(t *testing.T) {
	setAllowedOrigins("https://app.example")
	h := security.CORS(func(ctx *fasthttp.RequestCtx) {})

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Origin", "https://evil.example")
	ctx.Request.Header.SetMethod("OPTIONS")

	h(ctx)

	if v := ctx.Response.Header.Peek("Access-Control-Allow-Origin"); v != nil {
		t.Fatalf("origin outside the allow-list should not be reflected, got %q", v)
	}
}

func TestCORSWildcardAllowsAnyOrigin(t *testing.T) {
	setAllowedOrigins("*")
	h := security.CORS(func(ctx *fasthttp.RequestCtx) {})

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Origin", "https://any.example")
	ctx.Request.Header.SetMethod("GET")

	h(ctx)

	if got := string(ctx.Response.Header.Peek("Access-Control-Allow-Origin")); got != "https://any.example" {
		t.Fatalf("expected the origin to be reflected with '*', got %q", got)
	}
}

func TestCORSNoOrigin(t *testing.T) {
	setAllowedOrigins("http://example.com")
	called := false
	next := func(ctx *fasthttp.RequestCtx) {
		called = true
	}

	h := security.CORS(next)

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.SetMethod("GET")

	h(ctx)

	if !called {
		t.Fatalf("next handler not called")
	}

	if v := ctx.Response.Header.Peek("Access-Control-Allow-Origin"); v != nil {
		t.Fatalf("origin header should not be set")
	}
}

func TestCORSOptions(t *testing.T) {
	setAllowedOrigins("http://example.com")
	called := false
	next := func(ctx *fasthttp.RequestCtx) {
		called = true
	}

	h := security.CORS(next)

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.SetMethod("OPTIONS")
	ctx.Request.Header.Set("Origin", "http://example.com")

	h(ctx)

	if called {
		t.Fatalf("next handler should not be called on OPTIONS")
	}

	if ctx.Response.StatusCode() != 204 {
		t.Fatalf("unexpected status code: %d", ctx.Response.StatusCode())
	}
}
