package api

import (
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

func ExistsController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	ctx.Response.Header.Set("Content-Type", "application/json")

	exists := engine.EntityExists(entity, id)
	response := []byte(`{"exists": false}`)
	if exists {
		response = []byte(`{"exists": true}`)
	}

	ctx.SetBody(response)
}
