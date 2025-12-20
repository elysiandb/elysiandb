package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/schema"
	"github.com/valyala/fasthttp"
)

func GetSchemaController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)

	if !engine.EntityTypeExists(entity) {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.Response.Header.Set("Content-Type", "application/json")
		ctx.SetBodyString(`{"error":"entity not found"}`)
		return
	}

	schema := engine.ReadEntityById(schema.SchemaEntity, entity)
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
