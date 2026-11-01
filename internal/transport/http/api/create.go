package api

import (
	"encoding/json"

	"github.com/google/uuid"
	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/mongodb"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func CreateController(ctx *fasthttp.RequestCtx) {
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
		var tmp any
		if json.Unmarshal(body, &tmp) == nil {
			if obj, ok := tmp.(map[string]any); ok {
				if errs := schema.ValidateEntity(entity, obj, schemaData); len(errs) > 0 {
					b, _ := json.Marshal(errs)
					ctx.SetStatusCode(fasthttp.StatusBadRequest)
					ctx.Response.Header.Set("Content-Type", "application/json")
					ctx.SetBody(b)

					return
				}
			}

			if arr, ok := tmp.([]any); ok {
				for _, it := range arr {
					if obj, ok := it.(map[string]any); ok {
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
		}
	}

	if handleSingleEntity(ctx, entity, body) {
		finalizeCreate(entity, nestedTypes)
		return
	}

	if handleEntityList(ctx, entity, body) {
		finalizeCreate(entity, nestedTypes)
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

	username := security.GetCurrentUsername(ctx)
	if !acl.CanWriteEntity(username, entity, data) {
		sendForbidden(ctx)
		return true
	}

	id, hasId := data["id"].(string)
	if !hasId || id == "" {
		data["id"] = uuid.New().String()
	}

	if security.UserAuthenticationIsEnabled() {
		data[acl.UsernameField] = username
	}

	errors := engine.WriteEntity(entity, data)
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
	var list []map[string]any
	if err := json.Unmarshal(body, &list); err != nil || len(list) == 0 {
		return false
	}

	for _, item := range list {
		if item == nil {
			return false
		}
	}

	username := security.GetCurrentUsername(ctx)
	if !acl.CanWriteListOfEntities(username, entity, list) {
		sendForbidden(ctx)
		return true
	}

	for i := range list {
		id, hasId := list[i]["id"].(string)
		if !hasId || id == "" {
			list[i]["id"] = uuid.New().String()
		}

		if security.UserAuthenticationIsEnabled() {
			list[i][acl.UsernameField] = username
		}
	}

	validationErrors, valid := simulateListWrite(entity, list)
	if valid {
		validationErrors = engine.WriteListOfEntities(entity, list)
	}

	if hasValidationErrors(validationErrors) {
		sendListValidationErrors(ctx, validationErrors)

		return true
	}

	response, _ := json.Marshal(list)
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)

	return true
}

func simulateListWrite(entity string, list []map[string]any) ([][]schema.ValidationError, bool) {
	simulation := api_storage.NewWriteSimulation(engine.GetEntitySchema, engine.ReadEntityById)

	validationErrors := make([][]schema.ValidationError, 0, len(list))
	for _, item := range list {
		validationErrors = append(validationErrors, simulation.Write(entity, item))
	}

	return validationErrors, !hasValidationErrors(validationErrors)
}

func hasValidationErrors(validationErrors [][]schema.ValidationError) bool {
	for _, errs := range validationErrors {
		if len(errs) > 0 {
			return true
		}
	}

	return false
}

func sendListValidationErrors(ctx *fasthttp.RequestCtx, validationErrors [][]schema.ValidationError) {
	response, _ := json.Marshal(validationErrors)
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusBadRequest)
	ctx.SetBody(response)
}

func nestedEntityTypesInBody(body []byte) []string {
	var data any
	if err := json.Unmarshal(body, &data); err != nil {
		return nil
	}

	return api_storage.NestedEntityTypes(data)
}

func purgeCache(entity string, nestedTypes []string) {
	if !globals.GetConfig().Api.Cache.Enabled {
		return
	}

	cache.CacheStore.Purge(entity)
	for _, nested := range nestedTypes {
		cache.CacheStore.Purge(nested)
	}
}

func sendForbidden(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusForbidden)
	ctx.SetBodyString(`{"error":"forbidden"}`)
}

func finalizeCreate(entity string, nestedTypes []string) {
	purgeCache(entity, nestedTypes)

	acl.InitACL()
}
