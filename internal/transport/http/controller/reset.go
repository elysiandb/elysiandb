package controller

import (
	"net/http"

	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func ResetController(ctx *fasthttp.RequestCtx) {

	ctx.SetContentType("application/json")
	ctx.SetStatusCode(http.StatusOK)

	storage.ResetStore()
	observe.Audit(ctx, "store reset")
}
