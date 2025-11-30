package api

import (
	"fmt"

	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func CountController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	ctx.Response.Header.Set("Content-Type", "application/json")

	count, err := api_storage.CountEntities(entity)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBody([]byte(`{"error":"` + err.Error() + `"}`))
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody([]byte(`{"count":` + fmt.Sprintf("%d", count) + `}`))
}
