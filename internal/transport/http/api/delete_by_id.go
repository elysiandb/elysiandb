package api

import (
	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func DeleteByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)

	data := api_storage.ReadEntityById(entity, id)
	if data == nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		return
	}

	if !acl.CanDeleteEntity(entity, data) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	api_storage.DeleteEntityById(entity, id)

	ctx.SetStatusCode(fasthttp.StatusNoContent)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
