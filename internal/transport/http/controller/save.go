package controller

import (
	"net/http"

	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func SaveController(ctx *fasthttp.RequestCtx) {

	ctx.SetContentType("application/json")
	ctx.SetStatusCode(http.StatusNoContent)

	storage.WriteToDB()
	observe.Audit(ctx, "manual flush")
}
