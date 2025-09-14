package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func UpdateByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)

	var updated map[string]interface{}
	if err := json.Unmarshal(ctx.PostBody(), &updated); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString("Invalid JSON")
		return
	}

	data := api_storage.UpdateEntityById(entity, id, updated)

	response, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString("Error processing request")
		return
	}

	ctx.Response.Header.Set("Content-Type", "application/json")

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}
