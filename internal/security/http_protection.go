package security

import (
	"bytes"
	"net/url"
	"strings"

	"github.com/valyala/fasthttp"
)

const (
	AdminUIContentSecurityPolicy = "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; img-src 'self' data: blob:; font-src 'self' data:; connect-src 'self'; worker-src 'self' blob:; object-src 'none'; base-uri 'none'; form-action 'self'; frame-ancestors 'none'"
	APIContentSecurityPolicy     = "default-src 'none'; frame-ancestors 'none'"
)

var adminUIPathPrefix = []byte("/admin")

func Protect(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return CORS(RejectCrossOriginWrites(SecurityHeaders(next)))
}

func SecurityHeaders(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		next(ctx)

		header := &ctx.Response.Header
		header.Set("X-Content-Type-Options", "nosniff")
		header.Set("X-Frame-Options", "DENY")
		header.Set("Referrer-Policy", "no-referrer")

		if isAdminUIPath(ctx.Path()) {
			header.Set("Content-Security-Policy", AdminUIContentSecurityPolicy)
			return
		}

		header.Set("Content-Security-Policy", APIContentSecurityPolicy)
		if len(header.Peek("Cache-Control")) == 0 {
			header.Set("Cache-Control", "no-store")
		}
	}
}

func isAdminUIPath(path []byte) bool {
	if !bytes.HasPrefix(path, adminUIPathPrefix) {
		return false
	}

	return len(path) == len(adminUIPathPrefix) || path[len(adminUIPathPrefix)] == '/'
}

func RejectCrossOriginWrites(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		if CrossOriginWriteIsRejected(ctx) {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.SetStatusCode(fasthttp.StatusForbidden)
			ctx.SetBodyString(`{"error":"cross-origin request rejected"}`)
			return
		}

		next(ctx)
	}
}

func CrossOriginWriteIsRejected(ctx *fasthttp.RequestCtx) bool {
	switch string(ctx.Method()) {
	case fasthttp.MethodGet, fasthttp.MethodHead, fasthttp.MethodOptions:
		return false
	}

	origin := string(ctx.Request.Header.Peek("Origin"))
	if origin != "" && OriginIsAllowed(origin) {
		return false
	}

	switch string(ctx.Request.Header.Peek("Sec-Fetch-Site")) {
	case "":
	case "same-origin", "none":
		return false
	default:
		return true
	}

	if origin == "" {
		return false
	}

	parsed, err := url.Parse(origin)
	if err != nil || parsed.Host == "" {
		return true
	}

	return !strings.EqualFold(parsed.Host, string(ctx.Host()))
}
