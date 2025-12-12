package http_security

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

var req struct {
	Password string `json:"password"`
}

func ChangeUserPasswordController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if canManage, err := security.CurrentUserCanManageUser(ctx, username); err != nil || !canManage {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	if err := json.Unmarshal(ctx.PostBody(), &req); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)
		return
	}

	password := req.Password

	err := security.ChangeUserPassword(username, password)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
