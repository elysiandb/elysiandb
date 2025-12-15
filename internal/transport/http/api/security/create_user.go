package http_security

import (
	"encoding/json"

	"github.com/taymour/elysiandb/internal/acl"
	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

type UserDto struct {
	User     string `json:"username"`
	Password string `json:"password"`
	Role     string `json:"role"`
}

func (u *UserDto) ToBasicUser() *security.BasicUser {
	return &security.BasicUser{
		Username: u.User,
		Password: u.Password,
		Role:     security.Role(u.Role),
	}
}

func CreateUserController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")

	if !security.CurrentUserIsAdmin(ctx) {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	var user UserDto
	if err := json.Unmarshal(ctx.PostBody(), &user); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)

		return
	}

	err := security.CreateBasicUser(user.ToBasicUser())
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusNotFound)
		ctx.SetBodyString(`{"error":"` + err.Error() + `"}`)

		return
	}

	acl.InitACL()

	ctx.SetStatusCode(fasthttp.StatusOK)
}
