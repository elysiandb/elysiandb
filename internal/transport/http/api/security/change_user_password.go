package http_security

import (
	"encoding/json"
	"errors"

	"github.com/taymour/elysiandb/internal/security"
	"github.com/valyala/fasthttp"
)

type passwordDto struct {
	Password *string `json:"password"`
}

func ChangeUserPasswordController(ctx *fasthttp.RequestCtx) {
	ctx.Response.Header.Set("Content-Type", "application/json")
	username := ctx.UserValue("user_name").(string)

	if canManage, err := security.CurrentUserCanManageUser(ctx, username); err != nil || !canManage {
		ctx.SetStatusCode(fasthttp.StatusForbidden)
		return
	}

	var body passwordDto
	if err := json.Unmarshal(ctx.PostBody(), &body); err != nil || body.Password == nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		ctx.SetBodyString(`{"error":"invalid request body"}`)
		return
	}

	if err := security.ValidatePassword(*body.Password); err != nil {
		writeError(ctx, fasthttp.StatusBadRequest, err)
		return
	}

	keptSessionID := ""
	if currentSession, _ := security.CurrentSession(ctx); currentSession != nil && currentSession.Username == username {
		keptSessionID = currentSession.ID
	}

	err := security.ChangeUserPasswordKeepingSession(username, *body.Password, keptSessionID)
	if err != nil {
		writeError(ctx, fasthttp.StatusNotFound, err)
		return
	}

	ctx.SetStatusCode(fasthttp.StatusOK)
}

func createUserErrorStatus(err error) int {
	switch {
	case errors.Is(err, security.ErrInvalidUsername), errors.Is(err, security.ErrPasswordTooShort), errors.Is(err, security.ErrInvalidRole):
		return fasthttp.StatusBadRequest
	case errors.Is(err, security.ErrUserAlreadyExists):
		return fasthttp.StatusConflict
	default:
		return fasthttp.StatusNotFound
	}
}
