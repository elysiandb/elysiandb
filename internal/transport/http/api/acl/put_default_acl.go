package http_acl

import (
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/engine"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
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

	if engine.IsCoreEntityType(entity) {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"core entities have no ACL: users, ACLs and hooks are managed by admin users only"}`)
		return
	}

	if err := acl.ResetACLEntityToDefault(entity, username); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"unable to reset acl"}`)
		return
	}

	observe.Audit(ctx, "acl reset to default", "user", username, "entity", entity)
	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBodyString(`{"status":"ACL reset to default"}`)
}
