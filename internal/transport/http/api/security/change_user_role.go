package http_security

import (
	"encoding/json"
	"errors"

	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

type userRoleDto struct {
	Role string `json:"role"`
}

func ChangeUserRoleController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	var userDto userRoleDto
	if err := json.Unmarshal(ctx.PostBody(), &userDto); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)
		return
	}

	err := security.ChangeUserRole(username, userDto.Role)
	if err != nil {
		ctx.SetStatusCode(userErrorStatus(err))
		body, _ := json.Marshal(map[string]string{"error": err.Error()})
		ctx.SetBody(body)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}

func userErrorStatus(err error) int {
	switch {
	case errors.Is(err, security.ErrInvalidRole):
		return fasthttp.StatusBadRequest
	case errors.Is(err, security.ErrDefaultAdminProtected):
		return fasthttp.StatusForbidden
	case errors.Is(err, security.ErrLastAdmin):
		return fasthttp.StatusConflict
	default:
		return fasthttp.StatusNotFound
	}
}
