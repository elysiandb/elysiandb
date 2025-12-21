package api

import (
	"encoding/json"

	"github.com/google/uuid"
	"github.com/taymour/elysiandb/internal/acl"
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

	var schemaData map[string]any

	if engine.IsEngineMongoDB() {
		schemaData = mongodb.GetEntitySchema(entity)
	} else {
		schemaData = nil
	}

	if globals.GetConfig().Api.Schema.Strict && schema.IsManualSchema(entity, schemaData) {
		var tmp interface{}
		if json.Unmarshal(body, &tmp) == nil {
			if obj, ok := tmp.(map[string]interface{}); ok {
				if errs := schema.ValidateEntity(entity, obj, schemaData); len(errs) > 0 {
					b, _ := json.Marshal(errs)
					ctx.SetStatusCode(fasthttp.StatusBadRequest)
					ctx.Response.Header.Set("Content-Type", "application/json")
					ctx.SetBody(b)

					return
				}
			}

			if arr, ok := tmp.([]interface{}); ok {
				for _, it := range arr {
					if obj, ok := it.(map[string]interface{}); ok {
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
		finalizeCreate(entity)
		return
	}

	if handleEntityList(ctx, entity, body) {
		finalizeCreate(entity)
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

	if security.UserAuthenticationIsEnabled() {
		data[acl.UsernameField] = security.GetCurrentUsername()
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
	var list []map[string]interface{}
	if err := json.Unmarshal(body, &list); err != nil || len(list) == 0 {
		return false
	}

	for i := range list {
		id, hasId := list[i]["id"].(string)
		if !hasId || id == "" {
			list[i]["id"] = uuid.New().String()
		}

		if security.UserAuthenticationIsEnabled() {
			list[i][acl.UsernameField] = security.GetCurrentUsername()
		}
	}

	validationErrors := engine.WriteListOfEntities(entity, list)
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

func finalizeCreate(entity string) {
	if globals.GetConfig().Api.Cache.Enabled {
		cache.CacheStore.Purge(entity)
	}

	acl.InitACL()
}
