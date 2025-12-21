package api

import (
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func DeleteByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)

	data := engine.ReadEntityById(entity, id)
	if data == nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		return
	}

	if !acl.CanDeleteEntity(entity, data) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	engine.DeleteEntityById(entity, id)

	ctx.SetStatusCode(fasthttp.StatusNoContent)

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
