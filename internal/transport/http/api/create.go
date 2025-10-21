package api

import (
	"encoding/json"

	"github.com/google/uuid"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/valyala/fasthttp"
)

func CreateController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	body := ctx.PostBody()

	if handleSingleEntity(ctx, entity, body) {
		finalizeCreate(ctx, entity)
		return
	}

	if handleEntityList(ctx, entity, body) {
		finalizeCreate(ctx, entity)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusBadRequest)
	ctx.SetBodyString("Invalid JSON")
}

func handleSingleEntity(ctx *fasthttp.RequestCtx, entity string, body []byte) bool {
	var data map[string]interface{}
	if err := json.Unmarshal(body, &data); err != nil || len(data) == 0 {
		return false
	}
	id, hasId := data["id"].(string)
	if !hasId || id == "" {
		data["id"] = uuid.New().String()
	}
	errors := api_storage.WriteEntity(entity, data)
	if len(errors) > 0 {
		response, _ := json.Marshal(errors)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBody(response)
		return true
	}

	response, _ := json.Marshal(data)
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)

	return true
}

func handleEntityList(ctx *fasthttp.RequestCtx, entity string, body []byte) bool {
	var list []map[string]interface{}
	if err := json.Unmarshal(body, &list); err != nil || len(list) == 0 {
		return false
	}
	for i := range list {
		id, hasId := list[i]["id"].(string)
		if !hasId || id == "" {
			list[i]["id"] = uuid.New().String()
		}
	}

	validationErrors := api_storage.WriteListOfEntities(entity, list)
	hasErrors := false
	for _, errs := range validationErrors {
		if len(errs) > 0 {
			hasErrors = true
			break
		}
	}

	if hasErrors {
		response, _ := json.Marshal(validationErrors)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBody(response)

		return true
	}

	response, _ := json.Marshal(list)
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
	return true
}

func finalizeCreate(ctx *fasthttp.RequestCtx, entity string) {
	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
