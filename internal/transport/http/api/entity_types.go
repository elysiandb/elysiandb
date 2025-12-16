package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func GetEntityTypesController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	entityTypes := api_storage.ListEntityTypes()
	entitySchemas := make([]string, 0, len(entityTypes))
	for _, entityType := range entityTypes {
		schema := api_storage.GetEntitySchema(entityType)
		schemaBytes, err := json.Marshal(schema)
		if err != nil {
			ctx.SetStatusCode(fasthttp.StatusInternalServerError)
			ctx.SetBody([]byte(`{"error":"failed to marshal entity schema"}`))
			return
		}

		entitySchemas = append(entitySchemas, string(schemaBytes))
	}
	responseBytes, err := json.Marshal(map[string]interface{}{
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
