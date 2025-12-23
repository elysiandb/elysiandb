package api

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/engine"
	"github.com/valyala/fasthttp"
)

func GetEntityTypesController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	entityTypes := engine.ListPublicEntityTypes()
	entitySchemas := make([]string, 0, len(entityTypes))
	for _, entityType := range entityTypes {
		schema := engine.GetEntitySchema(entityType)
		schemaBytes, err := json.Marshal(schema)
		if err != nil {
			ctx.SetStatusCode(fasthttp.StatusInternalServerError)
			ctx.SetBody([]byte(`{"error":"failed to marshal entity schema"}`))
			return
		}

		entitySchemas = append(entitySchemas, string(schemaBytes))
	}

	responseBytes, err := json.Marshal(map[string]any{
		"entities": entitySchemas,
	})

	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"failed to marshal response"}`))
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(responseBytes)
}
