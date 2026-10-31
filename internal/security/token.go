package security

import (
	"crypto/subtle"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

var CheckTokenAuthentication = func(ctx *fasthttp.RequestCtx) bool {
	providedToken := string(ctx.Request.Header.Peek("Authorization"))
	if providedToken == "" {
		return false
	}

	const bearerPrefix = "Bearer "
	if len(providedToken) <= len(bearerPrefix) || providedToken[:len(bearerPrefix)] != bearerPrefix {
		return false
	}

	return TokenMatches(providedToken[len(bearerPrefix):])
}

func TokenMatches(providedToken string) bool {
	expectedToken := globals.GetConfig().Security.Authentication.Token
	if expectedToken == "" {
		return false
	}

	return subtle.ConstantTimeCompare([]byte(providedToken), []byte(expectedToken)) == 1
}
