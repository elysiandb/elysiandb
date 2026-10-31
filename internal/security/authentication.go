package security

import (
	"strings"

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

func TokenAuthenticationIsEnabled() bool {
	cfg := globals.GetConfig()
	return cfg.Security.Authentication.Enabled && cfg.Security.Authentication.Mode == "token"
}

func UserAuthenticationIsEnabled() bool {
	cfg := globals.GetConfig()
	return cfg.Security.Authentication.Enabled && cfg.Security.Authentication.Mode == "user"
}

func CredentialsAreRequired() bool {
	return BasicAuthenticationIsEnabled() || TokenAuthenticationIsEnabled() || UserAuthenticationIsEnabled()
}

func AuthenticateCredentials(credentials string) bool {
	if !CredentialsAreRequired() {
		return true
	}

	credentials = strings.TrimSpace(credentials)

	if TokenAuthenticationIsEnabled() {
		return TokenMatches(credentials)
	}

	username, password, ok := strings.Cut(credentials, " ")
	if !ok {
		return false
	}

	user, ok := AuthenticateUser(username, password)
	if !ok {
		return false
	}

	return !UserAuthenticationIsEnabled() || user.Role == RoleAdmin
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

		if TokenAuthenticationIsEnabled() && !CheckTokenAuthentication(ctx) {
			ctx.Response.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		if UserAuthenticationIsEnabled() {
			UserAuth(requestHandler)(ctx)
			return
		}

		requestHandler(ctx)
	}
}

func AuthenticateAdmin(requestHandler fasthttp.RequestHandler) fasthttp.RequestHandler {
	return Authenticate(func(ctx *fasthttp.RequestCtx) {
		if UserAuthenticationIsEnabled() && ctx.UserValue("role") != RoleAdmin {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.SetStatusCode(fasthttp.StatusForbidden)
			ctx.SetBodyString(`{"error":"admin role required"}`)

			return
		}

		requestHandler(ctx)
	})
}
