package controller

import (
	"net/http"

	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func ResetController(ctx *fasthttp.RequestCtx) {
	if globals.GetConfig().Stats.Enabled {
		stat.Stats.IncrementTotalRequests()
	}

	ctx.SetContentType("application/json")
	ctx.SetStatusCode(http.StatusOK)

	storage.ResetStore()
	observe.Audit(ctx, "store reset")
}
