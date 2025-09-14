package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func DeleteIndexController(ctx *fasthttp.RequestCtx) {
	api_storage.DeleteIndexesForField(
		ctx.UserValue("entity").(string),
		ctx.UserValue("field").(string),
	)

	ctx.SetStatusCode(fasthttp.StatusNoContent)
}
