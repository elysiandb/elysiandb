package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func CreateIndexController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	field := ctx.UserValue("field").(string)

	api_storage.CreateIndexesForField(entity, field)

	ctx.SetStatusCode(fasthttp.StatusNoContent)
}
