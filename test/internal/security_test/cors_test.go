package security_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func TestCORSWithOrigin(t *testing.T) {
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

func TestCORSNoOrigin(t *testing.T) {
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
