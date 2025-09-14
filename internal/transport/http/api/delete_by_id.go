package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func DeleteByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	api_storage.DeleteEntityById(entity, id)

	ctx.SetStatusCode(fasthttp.StatusNoContent)
}
