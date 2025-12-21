package http_acl

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func GetAllACLForUsername(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	username := ctx.UserValue("user_name").(string)

	var acls []map[string]any

	entities := engine.ListPublicEntityTypes()

	for _, entity := range entities {
		acl := acl.GetACLEntityForUsername(entity, username)
		if acl == nil {
			continue
		}

		acls = append(acls, acl.ToDataMap())
	}

	response, err := json.Marshal(acls)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"internal server error"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}
