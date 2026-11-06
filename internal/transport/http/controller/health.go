package controller

import (
	"net/http"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func HealthController(ctx *fasthttp.RequestCtx) {
	if globals.IsStarting() {
		ctx.SetStatusCode(http.StatusServiceUnavailable)
		return
	}

	ctx.SetStatusCode(http.StatusOK)
}
