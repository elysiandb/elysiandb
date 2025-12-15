package http_security

import (
	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

func DeleteUserByUsernameController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if canManage, err := security.CurrentUserCanManageUser(ctx, username); err != nil || !canManage {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	security.DeleteBasicUser(username)
	acl.DeleteUserACls(username)
	ctx.SetStatusCode(fasthttp.StatusOK)
}
