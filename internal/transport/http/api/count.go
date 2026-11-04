package api

import (
	"fmt"

	"github.com/taymour/elysiandb/internal/acl"
	api_storage "github.com/taymour/elysiandb/internal/api"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func CountController(ctx *fasthttp.RequestCtx) {
	entity := ctx.UserValue("entity").(string)
	ctx.Response.Header.Set("Content-Type", "application/json")

	username := security.GetCurrentUsername(ctx)

	var count int64
	if true {
		switch mode := acl.ReadModeFor(username, entity); mode {
		case acl.ReadAll:
			count = int64(api_storage.CountEntities(entity))
		default:
			res := api_storage.Plan(api_storage.ListQuery{Entity: entity})
			count = int64(countPlanned(res, true, mode, username))
		}
	} else {
		data := api_storage.ListEntities(entity, 0, 0, "", true, nil, "", "")
		count = int64(len(acl.FilterListOfEntities(username, entity, data)))
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody([]byte(`{"count":` + fmt.Sprintf("%d", count) + `}`))
}
