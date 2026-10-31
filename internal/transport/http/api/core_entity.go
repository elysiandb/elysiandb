package api

import (
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

func sendCoreEntityForbidden(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusForbidden)
	ctx.SetBodyString(coreEntityForbiddenBody)
}
