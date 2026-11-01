package security

import (
	"strings"
	"time"

	"github.com/valyala/fasthttp"
)

func RequestIsSecure(ctx *fasthttp.RequestCtx) bool {
	if ctx.IsTLS() {
		return true
	}

	proto, _, _ := strings.Cut(string(ctx.Request.Header.Peek("X-Forwarded-Proto")), ",")

	return strings.EqualFold(strings.TrimSpace(proto), "https")
}

func SetSessionCookie(ctx *fasthttp.RequestCtx, session *Session) {
	writeSessionCookie(ctx, session.ID, time.Time{})
}

func ClearSessionCookie(ctx *fasthttp.RequestCtx) {
	writeSessionCookie(ctx, "", time.Unix(0, 0))
}

func writeSessionCookie(ctx *fasthttp.RequestCtx, value string, expire time.Time) {
	cookie := fasthttp.AcquireCookie()
	defer fasthttp.ReleaseCookie(cookie)

	cookie.SetKey(SessionCookieName)
	cookie.SetValue(value)
	cookie.SetHTTPOnly(true)
	cookie.SetSameSite(fasthttp.CookieSameSiteStrictMode)
	cookie.SetPath("/")
	cookie.SetSecure(RequestIsSecure(ctx))
	if !expire.IsZero() {
		cookie.SetExpire(expire)
	}

	ctx.Response.Header.SetCookie(cookie)
}
