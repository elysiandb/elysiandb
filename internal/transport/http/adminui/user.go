package adminui

import (
	"encoding/json"
	"strings"
	"time"

	"github.com/taymour/elysiandb/internal/security"
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

func requestIsSecure(ctx *fasthttp.RequestCtx) bool {
	if ctx.IsTLS() {
		return true
	}

	proto, _, _ := strings.Cut(string(ctx.Request.Header.Peek("X-Forwarded-Proto")), ",")

	return strings.EqualFold(strings.TrimSpace(proto), "https")
}

func LoginController(ctx *fasthttp.RequestCtx) {
	var req loginRequest
	if err := json.Unmarshal(ctx.PostBody(), &req); err != nil {
		ctx.SetStatusCode(fasthttp.StatusBadRequest)
		return
	}

	user, ok := security.AuthenticateUser(req.Username, req.Password)
	if !ok || user == nil || user.Role != security.RoleAdmin {
		ctx.SetStatusCode(fasthttp.StatusUnauthorized)
		return
	}

	session, err := security.CreateSession(user.Username, user.Role, 24*time.Hour)
	if err != nil {
		ctx.SetStatusCode(fasthttp.StatusInternalServerError)
		return
	}

	cookie := fasthttp.AcquireCookie()
	cookie.SetKey(security.SessionCookieName)
	cookie.SetValue(session.ID)
	cookie.SetHTTPOnly(true)
	cookie.SetSameSite(fasthttp.CookieSameSiteStrictMode)
	cookie.SetPath("/")
	cookie.SetSecure(requestIsSecure(ctx))
	ctx.Response.Header.SetCookie(cookie)
	fasthttp.ReleaseCookie(cookie)

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

	cookie := fasthttp.AcquireCookie()
	cookie.SetKey(security.SessionCookieName)
	cookie.SetValue("")
	cookie.SetPath("/")
	cookie.SetExpire(time.Unix(0, 0))
	cookie.SetHTTPOnly(true)
	cookie.SetSameSite(fasthttp.CookieSameSiteStrictMode)
	cookie.SetSecure(requestIsSecure(ctx))
	ctx.Response.Header.SetCookie(cookie)
	fasthttp.ReleaseCookie(cookie)

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
