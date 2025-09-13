package api

import (
	"encoding/json"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func GetByIdController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	id := ctx.UserValue("id").(string)
	data := api_storage.ReadEntityById(entity, id)

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
