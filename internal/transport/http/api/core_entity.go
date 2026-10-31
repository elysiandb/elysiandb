package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

const coreEntityForbiddenBody = `{"error":"core entities cannot be accessed through the generic entity routes"}`

func RejectCoreEntity(next fasthttp.RequestHandler) fasthttp.RequestHandler {
	return func(ctx *fasthttp.RequestCtx) {
		entity, _ := ctx.UserValue("entity").(string)
		if engine.IsCoreEntityType(entity) {
			sendCoreEntityForbidden(ctx)
			return
		}

		next(ctx)
	}
}

func referencesCoreEntity(entityTypes []string) bool {
	for _, entityType := range entityTypes {
		if engine.IsCoreEntityType(entityType) {
			return true
		}
	}

	return false
}

func RejectCoreEntityReferences(ctx *fasthttp.RequestCtx, payload any) bool {
	if !referencesCoreEntity(api_storage.NestedEntityTypes(payload)) {
		return false
	}

	sendCoreEntityForbidden(ctx)

	return true
}

func sendCoreEntityForbidden(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusForbidden)
	ctx.SetBodyString(coreEntityForbiddenBody)
}
