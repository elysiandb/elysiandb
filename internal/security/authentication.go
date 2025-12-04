package security

import (
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func AuthenticationIsEnabled() bool {
	cfg := globals.GetConfig()
	return cfg.Security.Authentication.Enabled
}

func BasicAuthenticationIsEnabled() bool {
	cfg := globals.GetConfig()
	return cfg.Security.Authentication.Enabled && cfg.Security.Authentication.Mode == "basic"
}

func Authenticate(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		if !AuthenticationIsEnabled() {
			requestHandler(ctx)
			return
		}

		if BasicAuthenticationIsEnabled() && !CheckBasicAuthentication(ctx) {
			ctx.Response.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		requestHandler(ctx)
	}
}
