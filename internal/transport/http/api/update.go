package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func UpdateByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	body := ctx.PostBody()

	nestedTypes := nestedEntityTypesInBody(body)
	if referencesCoreEntity(nestedTypes) {
		sendCoreEntityForbidden(ctx)
		return
	}

	var schemaData map[string]any

	if engine.IsEngineMongoDB() {
		schemaData = mongodb.GetEntitySchema(entity)
	} else {
		schemaData = nil
	}

	if globals.GetConfig().Api.Schema.Strict && schema.IsManualSchema(entity, schemaData) {
		var obj map[string]any
		if json.Unmarshal(body, &obj) == nil {
			if errs := schema.ValidateEntityUpdate(entity, engine.ReadEntityById(entity, id), obj, schemaData); len(errs) > 0 {
				b, _ := json.Marshal(errs)
				ctx.SetStatusCode(fasthttp.StatusBadRequest)
				ctx.Response.Header.Set("Content-Type", "application/json")
				ctx.SetBody(b)

				return
			}
		}
	}

	if handleSingleUpdate(ctx, entity, id, body) {
		purgeCache(entity, nestedTypes)
		return
	}

	sendBadRequest(ctx)
}

func UpdateListController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	body := ctx.PostBody()

	nestedTypes := nestedEntityTypesInBody(body)
	if referencesCoreEntity(nestedTypes) {
		sendCoreEntityForbidden(ctx)
		return
	}

	var schemaData map[string]any

	if engine.IsEngineMongoDB() {
		schemaData = mongodb.GetEntitySchema(entity)
	} else {
		schemaData = nil
	}

	if globals.GetConfig().Api.Schema.Strict && schema.IsManualSchema(entity, schemaData) {
		var arr []map[string]any
		if json.Unmarshal(body, &arr) == nil {
			for _, obj := range arr {
				id, _ := obj["id"].(string)
				if errs := schema.ValidateEntityUpdate(entity, engine.ReadEntityById(entity, id), obj, schemaData); len(errs) > 0 {
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
		purgeCache(entity, nestedTypes)
		return
	}

	sendBadRequest(ctx)
}

func handleSingleUpdate(ctx *fasthttp.RequestCtx, entity, id string, body []byte) bool {
	var single map[string]any
	if err := json.Unmarshal(body, &single); err != nil || len(single) == 0 {
		return false
	}

	if !acl.CanUpdateEntity(security.GetCurrentUsername(ctx), entity, id, single) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBody([]byte(`{"error":"forbidden"}`))

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

	if !acl.CanUpdateListOfEntities(security.GetCurrentUsername(ctx), entity, list) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBody([]byte(`{"error":"forbidden"}`))

		return true
	}

	data := engine.UpdateListOfEntities(entity, list)
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
