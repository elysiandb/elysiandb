package controller

import (
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func GetConfigController(ctx *fasthttp.RequestCtx) {
	cfg := globals.GetConfig()

	ctx.Response.Header.Set("Content-Type", "application/json")
	_, _ = ctx.Write([]byte(cfg.ToJson()))
}
