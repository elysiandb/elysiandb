package http_security

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

var userDto struct {
	Role string `json:"role"`
}

func ChangeUserRoleController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	if err := json.Unmarshal(ctx.PostBody(), &userDto); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)
		return
	}

	role := userDto.Role

	err := security.ChangeUserRole(username, role)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}
