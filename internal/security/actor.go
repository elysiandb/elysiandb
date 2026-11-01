package security

import (
	"encoding/base64"
	"strings"

	"github.com/valyala/fasthttp"
)

func Actor(ctx *fasthttp.RequestCtx) string {
	if username := GetCurrentUsername(ctx); username != "" {
		return username
	}

	if BasicAuthenticationIsEnabled() {
		if username := basicUsername(ctx); username != "" {
			return username
		}
	}

	if TokenAuthenticationIsEnabled() {
		return "token"
	}

	if !AuthenticationIsEnabled() {
		return "anonymous"
	}

	return "unknown"
}

func basicUsername(ctx *fasthttp.RequestCtx) string {
	header := string(ctx.Request.Header.Peek("Authorization"))
	if !strings.HasPrefix(header, "Basic ") {
		return ""
	}

	payload, err := base64.StdEncoding.DecodeString(strings.TrimPrefix(header, "Basic "))
	if err != nil {
		return ""
	}

	username, _, ok := strings.Cut(string(payload), ":")
	if !ok {
		return ""
	}

	return username
}
