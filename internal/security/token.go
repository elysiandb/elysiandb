package security

import (
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

var CheckTokenAuthentication = func(ctx *fasthttp.RequestCtx) bool {
	cfg := globals.GetConfig()
	expectedToken := cfg.Security.Authentication.Token

	providedToken := string(ctx.Request.Header.Peek("Authorization"))
	if providedToken == "" {
		return false
	}

	const bearerPrefix = "Bearer "
	if len(providedToken) <= len(bearerPrefix) || providedToken[:len(bearerPrefix)] != bearerPrefix {
		return false
	}

	providedToken = providedToken[len(bearerPrefix):]

	return providedToken == expectedToken
}
