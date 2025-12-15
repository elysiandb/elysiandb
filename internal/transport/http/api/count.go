package api

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/valyala/fasthttp"
)

func CountController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	ctx.Response.Header.Set("Content-Type", "application/json")

	data := api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "")
	data = acl.FilterListOfEntities(entity, data)

	count := int64(len(data))

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody([]byte(`{"count":` + fmt.Sprintf("%d", count) + `}`))
}
