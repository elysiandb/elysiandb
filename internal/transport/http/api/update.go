package api

import (
	"encoding/json"
	"errors"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/valyala/fasthttp"
)

func UpdateByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	body := ctx.PostBody()

	schemaData := engine.GetEntitySchema(entity)

	if globals.GetConfig().Api.Schema.Strict && schema.IsManualSchema(entity, schemaData) {
		var obj map[string]any
		if json.Unmarshal(body, &obj) == nil {
			if errs := schema.ValidateEntity(entity, obj, schemaData); len(errs) > 0 {
				b, _ := json.Marshal(errs)
				ctx.SetStatusCode(fasthttp.StatusBadRequest)
				ctx.Response.Header.Set("Content-Type", "application/json")
				ctx.SetBody(b)

				return
			}
		}
	}

	if handleSingleUpdate(ctx, entity, id, body) {
		finalizeUpdate(entity)
		return
	}

	sendBadRequest(ctx)
}

func UpdateListController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	body := ctx.PostBody()

	schemaData := engine.GetEntitySchema(entity)

	if globals.GetConfig().Api.Schema.Strict && schema.IsManualSchema(entity, schemaData) {
		var arr []map[string]any
		if json.Unmarshal(body, &arr) == nil {
			for _, obj := range arr {
				if errs := schema.ValidateEntity(entity, obj, schemaData); len(errs) > 0 {
					b, _ := json.Marshal(errs)
					ctx.SetStatusCode(fasthttp.StatusBadRequest)
					ctx.Response.Header.Set("Content-Type", "application/json")
					ctx.SetBody(b)

					return
				}
			}
		}
	}

	if handleBatchUpdate(ctx, entity, body) {
		finalizeUpdate(entity)
		return
	}

	sendBadRequest(ctx)
}

func handleSingleUpdate(ctx *fasthttp.RequestCtx, entity, id string, body []byte) bool {
	var single map[string]any
	if err := json.Unmarshal(body, &single); err != nil || len(single) == 0 {
		return false
	}

	if err := checkUpdate(entity, id, single); err != nil {
		sendWriteError(ctx, err)
		return true
	}

	data := engine.UpdateEntityById(entity, id, single)

	response, _ := json.Marshal(data)
	sendJSONResponse(ctx, response)

	return true
}

func handleBatchUpdate(ctx *fasthttp.RequestCtx, entity string, body []byte) bool {
	var list []map[string]any
	if err := json.Unmarshal(body, &list); err != nil || len(list) == 0 {
		return false
	}

	for _, item := range list {
		id, _ := item["id"].(string)
		if err := checkUpdate(entity, id, item); err != nil {
			sendWriteError(ctx, err)
			return true
		}
	}

	data := engine.UpdateListOfEntities(entity, list)
	response, _ := json.Marshal(data)
	sendJSONResponse(ctx, response)

	return true
}

func checkUpdate(entity, id string, data map[string]any) error {
	if err := acl.CheckEntityUpdate(entity, id, data); err != nil {
		return err
	}

	return acl.CheckSubEntityWrites(data)
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

func sendWriteError(ctx *fasthttp.RequestCtx, err error) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if errors.Is(err, acl.ErrForbidden) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	response, _ := json.Marshal(map[string]string{"error": err.Error()})
	ctx.SetStatusCode(fasthttp.StatusBadRequest)
	ctx.SetBody(response)
}

func finalizeUpdate(entity string) {
	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}
}
