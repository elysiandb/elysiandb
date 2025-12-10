package adminui

import (
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func AdminAuth(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		id := string(ctx.Request.Header.Cookie(security.SessionCookieName))
		if id == "" {
			ctx.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		session, err := security.GetSession(id)
		if err != nil || session == nil {
			ctx.SetStatusCode(fasthttp.StatusUnauthorized)
			return
		}

		if session.Role != security.RoleAdmin {
			ctx.SetStatusCode(fasthttp.StatusForbidden)
			return
		}

		ctx.SetUserValue("username", session.Username)
		ctx.SetUserValue("role", session.Role)

		next(ctx)
	}
}
