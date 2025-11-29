package controller

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/stat"
	"github.com/valyala/fasthttp"
)

func StatsController(ctx *fasthttp.RequestCtx) {
	stat.Stats.EntitiesCount = api_storage.CountAllEntities()

	ctx.SetContentType("application/json")
	_, _ = ctx.Write([]byte(stat.Stats.ToJson()))
}
