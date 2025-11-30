package api

import (
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func ExistsController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	ctx.Response.Header.Set("Content-Type", "application/json")

	exists := api_storage.EntityExists(entity, id)
	response := []byte(`{"exists": false}`)
	if !exists {
		response = []byte(`{"exists": true}`)
	}

	ctx.SetBody(response)
}
