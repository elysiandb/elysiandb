package http_acl

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func UpdateACLForUsernameAndEntityController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)
		return
	}

	entity := ctx.UserValue("entity").(string)
	username := ctx.UserValue("user_name").(string)

	var payload struct {
		Permissions map[string]bool `json:"permissions"`
	}

	if err := json.Unmarshal(ctx.PostBody(), &payload); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid json body"}`)
		return
	}

	perms := acl.NewPermissions()
	for k, v := range payload.Permissions {
		if p, ok := acl.StringToPermission(k); ok {
			perms[p] = v
		}
	}

	if err := acl.UpdateACLEntityForUsername(entity, username, perms); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"unable to update acl"}`)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
	ctx.SetBodyString(`{"status":"ACL updated successfully"}`)
}
