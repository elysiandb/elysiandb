package security_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/configuration"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func TestAuthenticate_NoAuthentication(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = false

	called := false
	handler := security.Authenticate(func(ctx *fasthttp.RequestCtx) {
		called = true
	})

	ctx := &fasthttp.RequestCtx{}
	handler(ctx)

	if !called {
		t.Fatalf("handler should be called when authentication disabled")
	}
}

func TestAuthenticate_BasicAuth_Success(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "basic"

	ctx := &fasthttp.RequestCtx{}
	called := false

	orig := security.CheckBasicAuthentication
	security.CheckBasicAuthentication = func(c *fasthttp.RequestCtx) bool { return true }
	defer func() { security.CheckBasicAuthentication = orig }()

	handler := security.Authenticate(func(c *fasthttp.RequestCtx) {
		called = true
	})

	handler(ctx)

	if !called {
		t.Fatalf("handler should be called when basic auth succeeds")
	}
}

func TestAuthenticate_BasicAuth_Fail(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "basic"

	ctx := &fasthttp.RequestCtx{}
	called := false

	orig := security.CheckBasicAuthentication
	security.CheckBasicAuthentication = func(c *fasthttp.RequestCtx) bool { return false }
	defer func() { security.CheckBasicAuthentication = orig }()

	handler := security.Authenticate(func(c *fasthttp.RequestCtx) {
		called = true
	})

	handler(ctx)

	if called {
		t.Fatalf("handler should not be called when basic auth fails")
	}
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 unauthorized")
	}
}

func TestAuthenticate_TokenAuth_Success(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "token"
	globals.GetConfig().Security.Authentication.Token = "secret123"

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Bearer secret123")
	called := false

	handler := security.Authenticate(func(c *fasthttp.RequestCtx) {
		called = true
	})

	handler(ctx)

	if !called {
		t.Fatalf("handler should be called when token auth succeeds")
	}
}

func TestAuthenticate_TokenAuth_Fail(t *testing.T) {
	globals.SetConfig(&configuration.Config{})
	globals.GetConfig().Security.Authentication.Enabled = true
	globals.GetConfig().Security.Authentication.Mode = "token"
	globals.GetConfig().Security.Authentication.Token = "secret123"

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Bearer wrong")
	called := false

	handler := security.Authenticate(func(c *fasthttp.RequestCtx) {
		called = true
	})

	handler(ctx)

	if called {
		t.Fatalf("handler should not be called when token auth fails")
	}
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 unauthorized")
	}
}
