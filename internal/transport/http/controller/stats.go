package controller

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/taymour/elysiandb/internal/storage"
	"github.com/valyala/fasthttp"
)

func StatsController(ctx *fasthttp.RequestCtx) {
	keys, withTTL := storage.CountKeys()
	stat.Stats.SetKeysCount(keys)
	stat.Stats.SetExpirationKeysCount(withTTL)
	stat.Stats.EntitiesCount = api_storage.CountAllEntities()

	ctx.SetContentType("application/json")
	_, _ = ctx.Write([]byte(stat.Stats.ToJson()))
}
