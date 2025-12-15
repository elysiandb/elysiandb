package http_acl

import (
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func SetDefaultACLForUsernameAndEntityController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)
		return
	}

	entity := ctx.UserValue("entity").(string)
	username := ctx.UserValue("user_name").(string)

	if err := acl.ResetACLEntityToDefault(entity, username); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"unable to reset acl"}`)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBodyString(`{"status":"ACL reset to default"}`)
}
