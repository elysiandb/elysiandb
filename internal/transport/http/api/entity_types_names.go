package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"

	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func GetEntityTypesNamesController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	entityTypes := api_storage.ListPublicEntityTypes()

	responseBytes, err := json.Marshal(map[string]any{
		"entities": entityTypes,
	})
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBody([]byte(`{"error":"failed to marshal response"}`))
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(responseBytes)
}
