package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func DestroyController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	api_storage.DeleteAllEntities(entity)

	ctx.SetStatusCode(fasthttp.StatusNoContent)
}
