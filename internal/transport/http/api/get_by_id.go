package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/cache"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/globals"
	"github.com/taymour/elysiandb/internal/hook"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func GetByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	username := security.GetCurrentUsername(ctx)
	fieldsParam := string(ctx.QueryArgs().Peek("fields"))
	fields := api_storage.ParseFieldsParam(fieldsParam)
	includesParam := string(ctx.QueryArgs().Peek("includes"))

	cacheable := globals.GetConfig().Api.Cache.Enabled && !hook.EntityHasHooks(entity) && len(fields) == 0 && includesParam == ""

	if cacheable {
		if v := cache.CacheStore.GetById(entity, id); v != nil && canReadCachedEntity(username, entity, v) {
			ctx.Response.Header.Set("Content-Type", "application/json")
			ctx.Response.Header.Set("X-Elysian-Cache", "HIT")
			ctx.SetStatusCode(fasthttp.StatusOK)
			ctx.SetBody(v)
			return
		}
	}

	data := engine.ReadEntityById(entity, id)
	if data == nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
		ctx.SetBodyString("Entity not found")

		return
	}

	if !acl.CanReadEntity(username, entity, data) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
		ctx.SetBodyString("Access denied")

		return
	}

	if includesParam != "" {
		list := []map[string]any{data}
		data = engine.ApplyIncludes(list, includesParam)[0]
	}

	if len(fields) > 0 {
		data = engine.FilterFields(data, fields)
	}

	if globals.GetConfig().Api.Hooks.Enabled && hook.EntityHasPostReadHooks(entity) {
		data = hook.ApplyPostReadHooksForEntity(username, entity, data)
	}

	response, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
		ctx.SetBodyString("Error processing request")
		return
	}

	if cacheable {
		cache.CacheStore.SetById(entity, id, response)
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.Response.Header.Set("X-Elysian-Cache", "MISS")
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}

func canReadCachedEntity(username, entity string, cached []byte) bool {
	if !security.UserAuthenticationIsEnabled() {
		return true
	}

	var data map[string]any
	if err := json.Unmarshal(cached, &data); err != nil {
		return false
	}

	return acl.CanReadEntity(username, entity, data)
}
