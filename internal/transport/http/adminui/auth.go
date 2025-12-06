package adminui

import (
	"encoding/base64"
	"strings"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func AdminUIAuth(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		cfg := globals.GetConfig()

		if !cfg.AdminUI.Enabled {
			next(ctx)
			return
		}

		auth := string(ctx.Request.Header.Peek("Authorization"))
		if auth == "" || !strings.HasPrefix(auth, "Basic ") {
			unauthorized(ctx)
			return
		}

		payload, err := base64.StdEncoding.DecodeString(auth[len("Basic "):])
		if err != nil {
			unauthorized(ctx)
			return
		}

		parts := strings.SplitN(string(payload), ":", 2)
		if len(parts) != 2 {
			unauthorized(ctx)
			return
		}

		username, password := parts[0], parts[1]

		if username == cfg.AdminUI.Username && password == cfg.AdminUI.Password {
			next(ctx)
			return
		}

		unauthorized(ctx)
	}
}

func unauthorized(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("WWW-Authenticate", `Basic realm="ElysianDB Admin"`)
	ctx.SetStatusCode(fasthttp.StatusUnauthorized)
	ctx.SetBodyString("Unauthorized")
}
