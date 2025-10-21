package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func UpdateByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	body := ctx.PostBody()

	if handleSingleUpdate(ctx, entity, id, body) {
		finalizeUpdate(ctx, entity)
		return
	}

	sendBadRequest(ctx)
}

func UpdateListController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	body := ctx.PostBody()

	if handleBatchUpdate(ctx, entity, body) {
		finalizeUpdate(ctx, entity)
		return
	}

	sendBadRequest(ctx)
}

func handleSingleUpdate(ctx *fasthttp.RequestCtx, entity, id string, body []byte) bool {
	var single map[string]interface{}
	if err := json.Unmarshal(body, &single); err != nil || len(single) == 0 {
		return false
	}
	data := api_storage.UpdateEntityById(entity, id, single)
	response, _ := json.Marshal(data)
	sendJSONResponse(ctx, response)
	return true
}

func handleBatchUpdate(ctx *fasthttp.RequestCtx, entity string, body []byte) bool {
	var list []map[string]interface{}
	if err := json.Unmarshal(body, &list); err != nil || len(list) == 0 {
		return false
	}
	data := api_storage.UpdateListOfEntities(entity, list)
	response, _ := json.Marshal(data)
	sendJSONResponse(ctx, response)
	return true
}

func sendJSONResponse(ctx *fasthttp.RequestCtx, response []byte) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}

func sendBadRequest(ctx *fasthttp.RequestCtx) {
	ctx.SetStatusCode(fasthttp.StatusBadRequest)
	ctx.SetBodyString("Invalid JSON")
}

func finalizeUpdate(ctx *fasthttp.RequestCtx, entity string) {
	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
