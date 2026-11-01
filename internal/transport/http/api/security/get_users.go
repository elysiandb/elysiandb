package http_security

import (
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

type usersResponse struct {
	Users []userView `json:"users"`
}

func GetUsersController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		ctx.SetBodyString(`{"error":"forbidden"}`)

		return
	}

	users, err := security.ListBasicUsers()
	if err != nil {
		writeError(ctx, fasthttp.StatusInternalServerError, err)
		return
	}

	response := usersResponse{Users: make([]userView, 0, len(users))}
	for _, user := range users {
		response.Users = append(response.Users, userViewFrom(user))
	}

	writeJSON(ctx, fasthttp.StatusOK, response)
}
