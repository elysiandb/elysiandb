package adminui

import (
	"encoding/json"
	"strconv"

	"github.com/taymour/elysiandb/internal/security"
	"github.com/taymour/elysiandb/internal/transport/http/observe"
	"github.com/valyala/fasthttp"
)

type loginRequest struct {
	Username string `json:"username"`
	Password string `json:"password"`
}

type userResponse struct {
	Username string        `json:"username"`
	Role     security.Role `json:"role"`
}

func LoginController(ctx *fasthttp.RequestCtx) {
	var req loginRequest
	if err := json.Unmarshal(ctx.PostBody(), &req); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		return
	}

	source := security.LoginSourceFromRequest(ctx)
	user, retryAfter, ok := security.AuthenticateUserFrom(req.Username, req.Password, source)
	if retryAfter > 0 {
		ctx.Response.Header.Set("Retry-After", strconv.Itoa(security.RetryAfterSeconds(retryAfter)))
		ctx.SetStatusCode(fasthttp.StatusTooManyRequests)
		return
	}

	if ok && user != nil && user.Role != security.RoleAdmin {
		security.LogFailedLogin(req.Username, source, security.LoginFailureNotAdmin)
	}

	if !ok || user == nil || user.Role != security.RoleAdmin {
		ctx.SetStatusCode(fasthttp.StatusUnauthorized)
		return
	}

	session, err := security.CreateSession(user.Username, user.Role, security.SessionTTL)
	if err != nil {
		observe.SetError(ctx, err)
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	security.SetSessionCookie(ctx, session)
	security.LogLogin(user.Username, source)

	resp := userResponse{
		Username: user.Username,
		Role:     user.Role,
	}

	b, _ := json.Marshal(resp)
	ctx.SetContentType("application/json")
	ctx.SetBody(b)
}

func LogoutController(ctx *fasthttp.RequestCtx) {
	id := string(ctx.Request.Header.Cookie(security.SessionCookieName))
	if id != "" {
		_ = security.DeleteSession(id)
	}

	security.LogLogout(security.GetCurrentUsername(ctx))
	security.ClearSessionCookie(ctx)

	ctx.SetStatusCode(fasthttp.StatusNoContent)
}

func MeController(ctx *fasthttp.RequestCtx) {
	id := string(ctx.Request.Header.Cookie(security.SessionCookieName))
	if id == "" {
		ctx.SetStatusCode(fasthttp.StatusUnauthorized)
		return
	}

	session, err := security.GetSession(id)
	if err != nil || session == nil {
		ctx.SetStatusCode(fasthttp.StatusUnauthorized)
		return
	}

	resp := userResponse{
		Username: session.Username,
		Role:     session.Role,
	}

	b, _ := json.Marshal(resp)
	ctx.SetContentType("application/json")
	ctx.SetBody(b)
}
