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

func TestAuthenticate_UserAuth_Success(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	called := false
	calledUserAuth := false

	orig := security.UserAuth
	security.UserAuth = func(next fasthttp.RequestHandler) fasthttp.RequestHandler {
		return func(ctx *fasthttp.RequestCtx) {
			calledUserAuth = true
			next(ctx)
		}
	}
	defer func() { security.UserAuth = orig }()

	handler := security.Authenticate(func(ctx *fasthttp.RequestCtx) {
		called = true
	})

	ctx := &fasthttp.RequestCtx{}
	handler(ctx)

	if !calledUserAuth {
		t.Fatalf("user auth wrapper should be invoked")
	}
	if !called {
		t.Fatalf("final handler should be called when user auth enabled")
	}
}

func TestAuthenticate_UserAuth_Fail(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	called := false

	orig := security.UserAuth
	security.UserAuth = func(next fasthttp.RequestHandler) fasthttp.RequestHandler {
		return func(ctx *fasthttp.RequestCtx) {
			ctx.Response.SetStatusCode(fasthttp.StatusUnauthorized)
		}
	}
	defer func() { security.UserAuth = orig }()

	handler := security.Authenticate(func(ctx *fasthttp.RequestCtx) {
		called = true
	})

	ctx := &fasthttp.RequestCtx{}
	handler(ctx)

	if called {
		t.Fatalf("handler should not be called when user auth fails")
	}
	if ctx.Response.StatusCode() != fasthttp.StatusUnauthorized {
		t.Fatalf("expected 401 status when user auth fails")
	}
}

func TestAuthenticationIsEnabled(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	globals.SetConfig(cfg)

	if !security.AuthenticationIsEnabled() {
		t.Fatalf("expected authentication enabled")
	}

	cfg.Security.Authentication.Enabled = false
	globals.SetConfig(cfg)

	if security.AuthenticationIsEnabled() {
		t.Fatalf("expected authentication disabled")
	}
}

func TestBasicAuthenticationIsEnabled(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "basic"
	globals.SetConfig(cfg)

	if !security.BasicAuthenticationIsEnabled() {
		t.Fatalf("basic auth should be enabled")
	}

	cfg.Security.Authentication.Mode = "token"
	globals.SetConfig(cfg)

	if security.BasicAuthenticationIsEnabled() {
		t.Fatalf("basic auth should be disabled")
	}
}

func TestTokenAuthenticationIsEnabled(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "token"
	globals.SetConfig(cfg)

	if !security.TokenAuthenticationIsEnabled() {
		t.Fatalf("token auth should be enabled")
	}

	cfg.Security.Authentication.Mode = "basic"
	globals.SetConfig(cfg)

	if security.TokenAuthenticationIsEnabled() {
		t.Fatalf("token auth should be disabled")
	}
}

func TestUserAuthenticationIsEnabled(t *testing.T) {
	cfg := &configuration.Config{}
	cfg.Security.Authentication.Enabled = true
	cfg.Security.Authentication.Mode = "user"
	globals.SetConfig(cfg)

	if !security.UserAuthenticationIsEnabled() {
		t.Fatalf("user auth should be enabled")
	}

	cfg.Security.Authentication.Mode = "basic"
	globals.SetConfig(cfg)

	if security.UserAuthenticationIsEnabled() {
		t.Fatalf("user auth should be disabled")
	}
}
