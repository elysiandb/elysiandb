package http_security

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

func DeleteUserByUsernameController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if canManage, err := security.CurrentUserCanManageUser(ctx, username); err != nil || !canManage {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	if err := security.DeleteBasicUser(username); err != nil {
		ctx.SetStatusCode(userErrorStatus(err))
		body, _ := json.Marshal(map[string]string{"error": err.Error()})
		ctx.SetBody(body)
		return
	}

	acl.DeleteUserACls(username)
	observe.Audit(ctx, "user deleted", "user", username)
	ctx.SetStatusCode(fasthttp.StatusOK)
}
