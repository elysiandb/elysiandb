package security_test

import (
	"testing"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func TestCheckTokenAuthentication_ValidToken(t *testing.T) {
	globals.GetConfig().Security.Authentication.Token = "secret123"

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Bearer secret123")

	if !security.CheckTokenAuthentication(ctx) {
		t.Fatalf("expected authentication to succeed")
	}
}

func TestCheckTokenAuthentication_InvalidToken(t *testing.T) {
	globals.GetConfig().Security.Authentication.Token = "secret123"

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Bearer wrong")

	if security.CheckTokenAuthentication(ctx) {
		t.Fatalf("expected authentication to fail with invalid token")
	}
}

func TestCheckTokenAuthentication_MissingHeader(t *testing.T) {
	globals.GetConfig().Security.Authentication.Token = "secret123"

	ctx := &fasthttp.RequestCtx{}

	if security.CheckTokenAuthentication(ctx) {
		t.Fatalf("expected authentication to fail without header")
	}
}

func TestCheckTokenAuthentication_NoBearerPrefix(t *testing.T) {
	globals.GetConfig().Security.Authentication.Token = "secret123"

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Token secret123")

	if security.CheckTokenAuthentication(ctx) {
		t.Fatalf("expected authentication to fail without Bearer prefix")
	}
}

func TestCheckTokenAuthentication_EmptyToken(t *testing.T) {
	globals.GetConfig().Security.Authentication.Token = ""

	ctx := &fasthttp.RequestCtx{}
	ctx.Request.Header.Set("Authorization", "Bearer ")

	if security.CheckTokenAuthentication(ctx) {
		t.Fatalf("expected authentication to fail when expected token is empty")
	}
}
