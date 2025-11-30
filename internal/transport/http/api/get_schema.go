package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/valyala/fasthttp"
)

func GetSchemaController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)

	if !api_storage.EntityTypeExists(entity) {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetBodyString(`{"error":"entity not found"}`)
		return
	}

	schema := api_storage.ReadEntityById(schema.SchemaEntity, entity)
	if schema == nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetBodyString(`{"error":"schema not found"}`)
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/json")
	ctx.SetStatusCode(fasthttp.StatusOK)
	data, err := json.Marshal(schema)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"failed to marshal schema"}`)
		return
	}

	ctx.SetBody(data)
}
