package http_acl

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func GetACLForUsernameAndEntityController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	entity := ctx.UserValue("entity").(string)
	username := ctx.UserValue("user_name").(string)

	acl := acl.GetACLEntityForUsername(entity, username)
	if acl == nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		return
	}

	data := acl.ToDataMap()

	response, err := json.Marshal(data)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		ctx.SetBodyString(`{"error":"internal server error"}`)

		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBody(response)
}
