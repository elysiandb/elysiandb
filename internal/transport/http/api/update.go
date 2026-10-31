package api

import (
	"encoding/json"
	"slices"

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

	stored := engine.ReadEntityById(entity, id)
	if stored == nil {
		sendEntityNotFound(ctx, nil)
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
			if errs := schema.ValidateEntityUpdate(entity, stored, obj, schemaData); len(errs) > 0 {
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

	var list []map[string]any
	if err := json.Unmarshal(body, &list); err != nil || len(list) == 0 {
		sendBadRequest(ctx)
		return
	}

	stored, missing := readEntitiesToUpdate(entity, list)
	if len(missing) > 0 {
		sendEntityNotFound(ctx, missing)
		return
	}

	var schemaData map[string]any

	if engine.IsEngineMongoDB() {
		schemaData = mongodb.GetEntitySchema(entity)
	} else {
		schemaData = nil
	}

	if globals.GetConfig().Api.Schema.Strict && schema.IsManualSchema(entity, schemaData) {
		for _, obj := range list {
			id, _ := obj["id"].(string)
			if errs := schema.ValidateEntityUpdate(entity, stored[id], obj, schemaData); len(errs) > 0 {
				b, _ := json.Marshal(errs)
				ctx.SetStatusCode(fasthttp.StatusBadRequest)
				ctx.Response.Header.Set("Content-Type", "application/json")
				ctx.SetBody(b)

				return
			}
		}
	}

	handleBatchUpdate(ctx, entity, list)
	purgeCache(entity, nestedTypes)
}

func readEntitiesToUpdate(entity string, list []map[string]any) (map[string]map[string]any, []string) {
	stored := make(map[string]map[string]any, len(list))
	missing := make([]string, 0)

	for _, obj := range list {
		id, ok := obj["id"].(string)
		if !ok || id == "" || stored[id] != nil || slices.Contains(missing, id) {
			continue
		}

		data := engine.ReadEntityById(entity, id)
		if data == nil {
			missing = append(missing, id)
			continue
		}

		stored[id] = data
	}

	return stored, missing
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

func handleBatchUpdate(ctx *fasthttp.RequestCtx, entity string, list []map[string]any) {
	if !acl.CanUpdateListOfEntities(security.GetCurrentUsername(ctx), entity, list) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBody([]byte(`{"error":"forbidden"}`))

		return
	}

	data := engine.UpdateListOfEntities(entity, list)
	response, _ := json.Marshal(data)
	sendJSONResponse(ctx, response)
}

func sendEntityNotFound(ctx *fasthttp.RequestCtx, ids []string) {
	body := map[string]any{"error": "entity not found"}
	if ids != nil {
		body["ids"] = ids
	}

	response, _ := json.Marshal(body)
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusNotFound)
	ctx.SetBody(response)
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
