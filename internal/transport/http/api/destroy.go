package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func DestroyController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	api_storage.DeleteAllEntities(entity)

	ctx.SetStatusCode(fasthttp.StatusNoContent)

	if globals.GetConfig().ApiCache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
