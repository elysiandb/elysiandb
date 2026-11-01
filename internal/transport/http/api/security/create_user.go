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

func (u *UserDto) Validate() error {
	if err := security.ValidateUsername(u.User); err != nil {
		return err
	}

	if u.Role != "" && security.Role(u.Role) != security.RoleAdmin && security.Role(u.Role) != security.RoleUser {
		return security.ErrInvalidRole
	}

	if err := security.ValidatePassword(u.Password); err != nil {
		return err
	}

	if security.BasicUserExists(u.User) {
		return security.ErrUserAlreadyExists
	}

	return nil
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

	if err := user.Validate(); err != nil {
		writeError(ctx, createUserErrorStatus(err), err)
		return
	}

	err := security.CreateBasicUser(user.ToBasicUser())
	if err != nil {
		writeError(ctx, fasthttp.StatusNotFound, err)
		return
	}

	acl.InitACL()

	ctx.SetStatusCode(fasthttp.StatusOK)
}
