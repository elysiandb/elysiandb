package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func GetEntityTypesNamesController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	entityTypes := api_storage.ListPublicEntityTypes()

	responseBytes, err := json.Marshal(map[string]interface{}{
		"entities": entityTypes,
	})
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"failed to marshal response"}`))
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(responseBytes)
}
