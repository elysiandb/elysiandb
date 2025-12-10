package security

import (
	"github.com/valyala/fasthttp"
)

const SessionCookieName = "edb_session"

var UserAuth func(next fasthttp.RequestHandler) fasthttp.RequestHandler = func(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		id := string(ctx.Request.Header.Cookie(SessionCookieName))
		if id == "" {
			ctx.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		session, err := GetSession(id)
		if err != nil || session == nil {
			ctx.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		ctx.SetUserValue("username", session.Username)
		ctx.SetUserValue("role", session.Role)

		next(ctx)
	}
}
