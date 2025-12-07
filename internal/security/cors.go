package security

import "github.com/valyala/fasthttp"

func CORS(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		origin := string(ctx.Request.Header.Peek("Origin"))

		if origin != "" {
			ctx.Response.Header.Set("Access-Control-Allow-Origin", origin)
			ctx.Response.Header.Set("Vary", "Origin")
			ctx.Response.Header.Set("Access-Control-Allow-Credentials", "true")
			ctx.Response.Header.Set("Access-Control-Allow-Headers", "Content-Type, Authorization")
			ctx.Response.Header.Set("Access-Control-Allow-Methods", "GET, POST, PUT, DELETE, OPTIONS")
		}

		if string(ctx.Method()) == fasthttp.MethodOptions {
			ctx.SetStatusCode(204)
			return
		}

		next(ctx)
	}
}
