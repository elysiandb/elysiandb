package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func GetByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	fieldsParam := string(ctx.QueryArgs().Peek("fields"))
	fields := api_storage.ParseFieldsParam(fieldsParam)
	includesParam := string(ctx.QueryArgs().Peek("includes"))

	if len(fields) == 0 && globals.GetConfig().Api.Cache.Enabled {
		if v := cache.CacheStore.GetById(entity, id); v != nil {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.Response.Header.Set("X-Elysian-Cache", "HIT")
			ctx.SetStatusCode(fasthttp.StatusOK)
			ctx.SetBody(v)
			return
		}
	}

	data := api_storage.ReadEntityById(entity, id)
	if data != nil && includesParam != "" {
		list := []map[string]interface{}{data}
		data = api_storage.ApplyIncludes(list, includesParam)[0]
	}

	if len(fields) > 0 {
		data = api_storage.FilterFields(data, fields)
	}

	response, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
		ctx.SetBodyString("Error processing request")
		return
	}

	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.SetById(entity, id, response)
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}
